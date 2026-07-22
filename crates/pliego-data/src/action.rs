// SPDX-License-Identifier: Apache-2.0

use crate::loader::canonical_json_bytes;
use crate::receipt::DataDurationBucket;
use crate::{
    CacheTag, DataContext, DataError, DataOperation, DataOutcome, DataReceipt, DataSizeBucket,
    ResourceLease, ResourceRequirement, validate_stable_id,
};
use crate::{
    IdempotencyDecision, IdempotencyError, IdempotencyKey, IdempotencyManager,
    IdempotencyPartition, IdempotencyPolicy, IdempotencyStore, InvalidationEvent,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub type ActionFuture<Output, FieldErrors> = Pin<
    Box<
        dyn Future<Output = Result<ActionResponse<Output, FieldErrors>, DataError>>
            + Send
            + 'static,
    >,
>;

pub trait Action<Input, Output, FieldErrors>: Send + Sync + 'static {
    fn execute(&self, context: ActionContext, input: Input) -> ActionFuture<Output, FieldErrors>;
}

impl<Input, Output, FieldErrors, Function, OutputFuture> Action<Input, Output, FieldErrors>
    for Function
where
    Function: Fn(ActionContext, Input) -> OutputFuture + Send + Sync + 'static,
    OutputFuture:
        Future<Output = Result<ActionResponse<Output, FieldErrors>, DataError>> + Send + 'static,
{
    fn execute(&self, context: ActionContext, input: Input) -> ActionFuture<Output, FieldErrors> {
        Box::pin(self(context, input))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionMediaType {
    FormUrlencoded,
    Json,
    MultipartFormData,
}

impl ActionMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FormUrlencoded => "application/x-www-form-urlencoded",
            Self::Json => "application/json",
            Self::MultipartFormData => "multipart/form-data",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionContentEncoding {
    Identity,
    Gzip,
}

impl ActionContentEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Gzip => "gzip",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OriginPolicy {
    SameOrigin,
    ExplicitlyNonBrowser,
}

impl OriginPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SameOrigin => "same-origin",
            Self::ExplicitlyNonBrowser => "explicitly-non-browser",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CsrfPolicy {
    SameOrigin,
    SessionBoundToken,
}

impl CsrfPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SameOrigin => "same-origin",
            Self::SessionBoundToken => "session-bound-token",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvalidationConsistency {
    Eventual,
    ReadYourWrites,
}

impl InvalidationConsistency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eventual => "eventual",
            Self::ReadYourWrites => "read-your-writes",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionInvalidationIntent {
    cache_policy_id: String,
    tags: Vec<CacheTag>,
    consistency: InvalidationConsistency,
}

impl ActionInvalidationIntent {
    pub fn tags(
        cache_policy_id: impl Into<String>,
        tags: impl IntoIterator<Item = CacheTag>,
    ) -> Result<Self, DataError> {
        let cache_policy_id = cache_policy_id.into();
        if !validate_stable_id(&cache_policy_id) {
            return Err(DataError::InvalidStableId(cache_policy_id));
        }
        let mut tags = tags.into_iter().collect::<Vec<_>>();
        tags.sort();
        tags.dedup();
        if tags.is_empty() || tags.len() > 32 {
            return Err(DataError::InvalidActionState(
                "an invalidation intent requires between 1 and 32 tags".to_owned(),
            ));
        }
        Ok(Self {
            cache_policy_id,
            tags,
            consistency: InvalidationConsistency::Eventual,
        })
    }

    pub fn read_your_writes(mut self) -> Self {
        self.consistency = InvalidationConsistency::ReadYourWrites;
        self
    }

    pub fn cache_policy_id(&self) -> &str {
        &self.cache_policy_id
    }

    pub fn tags_value(&self) -> &[CacheTag] {
        &self.tags
    }

    pub fn consistency(&self) -> InvalidationConsistency {
        self.consistency
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionPolicy {
    id: String,
    semantic_revision: u32,
    input_schema_id: String,
    field_error_schema_id: String,
    output_schema_id: String,
    accepted_media_types: Vec<ActionMediaType>,
    accepted_content_encodings: Vec<ActionContentEncoding>,
    max_encoded_bytes: usize,
    max_decoded_bytes: usize,
    max_form_fields: usize,
    max_output_bytes: usize,
    origin_policy: OriginPolicy,
    csrf_policy: CsrfPolicy,
    require_authentication: bool,
    require_authorization: bool,
    resources: Vec<ResourceRequirement>,
    post_commit_grace_ms: u64,
    idempotency_policy_id: Option<String>,
    invalidation_intents: Vec<ActionInvalidationIntent>,
}

impl ActionPolicy {
    pub fn new(
        id: impl Into<String>,
        semantic_revision: u32,
        input_schema_id: impl Into<String>,
        field_error_schema_id: impl Into<String>,
        output_schema_id: impl Into<String>,
    ) -> Result<Self, DataError> {
        let id = id.into();
        let input_schema_id = input_schema_id.into();
        let field_error_schema_id = field_error_schema_id.into();
        let output_schema_id = output_schema_id.into();
        for value in [
            &id,
            &input_schema_id,
            &field_error_schema_id,
            &output_schema_id,
        ] {
            if !validate_stable_id(value) {
                return Err(DataError::InvalidStableId(value.clone()));
            }
        }
        if semantic_revision == 0 {
            return Err(DataError::InvalidActionState(
                "semantic revision must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            id,
            semantic_revision,
            input_schema_id,
            field_error_schema_id,
            output_schema_id,
            accepted_media_types: vec![ActionMediaType::FormUrlencoded],
            accepted_content_encodings: vec![ActionContentEncoding::Identity],
            max_encoded_bytes: 256 * 1_024,
            max_decoded_bytes: 256 * 1_024,
            max_form_fields: 256,
            max_output_bytes: 256 * 1_024,
            origin_policy: OriginPolicy::SameOrigin,
            csrf_policy: CsrfPolicy::SessionBoundToken,
            require_authentication: true,
            require_authorization: true,
            resources: Vec::new(),
            post_commit_grace_ms: 2_000,
            idempotency_policy_id: None,
            invalidation_intents: Vec::new(),
        })
    }

    pub fn accept_media_type(mut self, media_type: ActionMediaType) -> Self {
        if !self.accepted_media_types.contains(&media_type) {
            self.accepted_media_types.push(media_type);
            self.accepted_media_types.sort_unstable();
        }
        self
    }

    pub fn accept_content_encoding(mut self, encoding: ActionContentEncoding) -> Self {
        if !self.accepted_content_encodings.contains(&encoding) {
            self.accepted_content_encodings.push(encoding);
            self.accepted_content_encodings.sort_unstable();
        }
        self
    }

    pub fn max_encoded_bytes(mut self, maximum: usize) -> Result<Self, DataError> {
        if maximum == 0 || maximum > 64 * 1_024 * 1_024 {
            return Err(DataError::InvalidActionState(
                "max encoded bytes must be between 1 and 67108864".to_owned(),
            ));
        }
        self.max_encoded_bytes = maximum;
        Ok(self)
    }

    pub fn max_decoded_bytes(mut self, maximum: usize) -> Result<Self, DataError> {
        if maximum == 0 || maximum > 64 * 1_024 * 1_024 {
            return Err(DataError::InvalidActionState(
                "max decoded bytes must be between 1 and 67108864".to_owned(),
            ));
        }
        self.max_decoded_bytes = maximum;
        Ok(self)
    }

    pub fn max_output_bytes(mut self, maximum: usize) -> Result<Self, DataError> {
        if maximum == 0 || maximum > 16 * 1_024 * 1_024 {
            return Err(DataError::InvalidActionState(
                "max output bytes must be between 1 and 16777216".to_owned(),
            ));
        }
        self.max_output_bytes = maximum;
        Ok(self)
    }

    pub fn max_form_fields(mut self, maximum: usize) -> Result<Self, DataError> {
        if maximum == 0 || maximum > 4_096 {
            return Err(DataError::InvalidActionState(
                "max form fields must be between 1 and 4096".to_owned(),
            ));
        }
        self.max_form_fields = maximum;
        Ok(self)
    }

    pub fn origin_policy(mut self, policy: OriginPolicy) -> Self {
        self.origin_policy = policy;
        self
    }

    pub fn csrf_policy(mut self, policy: CsrfPolicy) -> Self {
        self.csrf_policy = policy;
        self
    }

    pub fn require_authentication(mut self, required: bool) -> Self {
        self.require_authentication = required;
        self
    }

    pub fn require_authorization(mut self, required: bool) -> Self {
        self.require_authorization = required;
        self
    }

    pub fn resource(mut self, requirement: ResourceRequirement) -> Result<Self, DataError> {
        if self
            .resources
            .iter()
            .any(|current| current.id() == requirement.id())
        {
            return Err(DataError::DuplicateResource(requirement.id().to_owned()));
        }
        if self.resources.len() >= 32 {
            return Err(DataError::InvalidActionState(
                "an action may require at most 32 resources".to_owned(),
            ));
        }
        self.resources.push(requirement);
        Ok(self)
    }

    pub fn post_commit_grace(mut self, duration: Duration) -> Result<Self, DataError> {
        let millis = u64::try_from(duration.as_millis()).map_err(|_| {
            DataError::InvalidActionState("post-commit grace is too large".to_owned())
        })?;
        if millis == 0 || millis > 30_000 {
            return Err(DataError::InvalidActionState(
                "post-commit grace must be between 1 and 30000 ms".to_owned(),
            ));
        }
        self.post_commit_grace_ms = millis;
        Ok(self)
    }

    pub fn idempotency(mut self, policy: &IdempotencyPolicy) -> Self {
        self.idempotency_policy_id = Some(policy.id().to_owned());
        self
    }

    pub fn invalidation(mut self, intent: ActionInvalidationIntent) -> Result<Self, DataError> {
        if self
            .invalidation_intents
            .iter()
            .any(|current| current.cache_policy_id == intent.cache_policy_id)
        {
            return Err(DataError::InvalidActionState(format!(
                "duplicate invalidation policy {}",
                intent.cache_policy_id
            )));
        }
        if self.invalidation_intents.len() >= 16 {
            return Err(DataError::InvalidActionState(
                "an action may declare at most 16 invalidation intents".to_owned(),
            ));
        }
        self.invalidation_intents.push(intent);
        self.invalidation_intents
            .sort_by(|left, right| left.cache_policy_id.cmp(&right.cache_policy_id));
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn semantic_revision(&self) -> u32 {
        self.semantic_revision
    }

    pub fn accepts(&self, media_type: ActionMediaType) -> bool {
        self.accepted_media_types.contains(&media_type)
    }

    pub fn accepts_content_encoding(&self, encoding: ActionContentEncoding) -> bool {
        self.accepted_content_encodings.contains(&encoding)
    }

    pub fn accepted_media_types(&self) -> &[ActionMediaType] {
        &self.accepted_media_types
    }

    pub fn accepted_content_encodings(&self) -> &[ActionContentEncoding] {
        &self.accepted_content_encodings
    }

    pub fn max_encoded_bytes_value(&self) -> usize {
        self.max_encoded_bytes
    }

    pub fn max_decoded_bytes_value(&self) -> usize {
        self.max_decoded_bytes
    }

    pub fn max_output_bytes_value(&self) -> usize {
        self.max_output_bytes
    }

    pub fn max_form_fields_value(&self) -> usize {
        self.max_form_fields
    }

    pub fn post_commit_grace_ms(&self) -> u64 {
        self.post_commit_grace_ms
    }

    pub fn resource_requirements(&self) -> &[ResourceRequirement] {
        &self.resources
    }

    pub fn idempotency_policy_id(&self) -> Option<&str> {
        self.idempotency_policy_id.as_deref()
    }

    pub fn invalidation_intents(&self) -> &[ActionInvalidationIntent] {
        &self.invalidation_intents
    }

    pub fn origin_policy_value(&self) -> OriginPolicy {
        self.origin_policy
    }

    pub fn csrf_policy_value(&self) -> CsrfPolicy {
        self.csrf_policy
    }

    pub fn requires_authentication(&self) -> bool {
        self.require_authentication
    }

    pub fn requires_authorization(&self) -> bool {
        self.require_authorization
    }

    pub fn contract_digest(&self) -> String {
        let mut digest = sha2::Sha256::new();
        digest.update(b"pliego-action-policy-v1\0");
        for value in [
            self.id.as_bytes(),
            &self.semantic_revision.to_be_bytes(),
            self.input_schema_id.as_bytes(),
            self.field_error_schema_id.as_bytes(),
            self.output_schema_id.as_bytes(),
            &self.max_encoded_bytes.to_be_bytes(),
            &self.max_decoded_bytes.to_be_bytes(),
            &self.max_form_fields.to_be_bytes(),
            &self.max_output_bytes.to_be_bytes(),
            &self.post_commit_grace_ms.to_be_bytes(),
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value);
        }
        for media_type in &self.accepted_media_types {
            digest.update(media_type.as_str().as_bytes());
            digest.update([0]);
        }
        for encoding in &self.accepted_content_encodings {
            digest.update(encoding.as_str().as_bytes());
            digest.update([0]);
        }
        digest.update([self.origin_policy as u8, self.csrf_policy as u8]);
        digest.update([
            u8::from(self.require_authentication),
            u8::from(self.require_authorization),
        ]);
        let mut resources = self.resources.iter().collect::<Vec<_>>();
        resources.sort_by(|left, right| left.id().cmp(right.id()));
        for resource in resources {
            digest.update(resource.id().as_bytes());
            digest.update([0]);
            for capability in resource.capabilities().iter() {
                digest.update(capability.as_bytes());
                digest.update([0]);
            }
        }
        if let Some(id) = &self.idempotency_policy_id {
            digest.update(id.as_bytes());
        }
        for intent in &self.invalidation_intents {
            digest.update(intent.cache_policy_id.as_bytes());
            digest.update([intent.consistency as u8]);
            for tag in &intent.tags {
                digest.update(tag.as_str().as_bytes());
                digest.update([0]);
            }
        }
        encode_hex(&digest.finalize())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionAdmission {
    media_type: ActionMediaType,
    decoded_bytes: usize,
    same_origin: bool,
    csrf_verified: bool,
    authenticated: bool,
    authorized: bool,
}

impl ActionAdmission {
    pub fn new(media_type: ActionMediaType, decoded_bytes: usize) -> Self {
        Self {
            media_type,
            decoded_bytes,
            same_origin: false,
            csrf_verified: false,
            authenticated: false,
            authorized: false,
        }
    }

    pub fn same_origin(mut self, verified: bool) -> Self {
        self.same_origin = verified;
        self
    }

    pub fn csrf_verified(mut self, verified: bool) -> Self {
        self.csrf_verified = verified;
        self
    }

    pub fn authenticated(mut self, verified: bool) -> Self {
        self.authenticated = verified;
        self
    }

    pub fn authorized(mut self, verified: bool) -> Self {
        self.authorized = verified;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionCommitState {
    NotStarted,
    PreCommit,
    Committing,
    Committed,
    Failed,
    OutcomeUnknown,
    CompensationRequired,
}

#[derive(Clone)]
pub struct ActionCommitHandle {
    state: Arc<Mutex<ActionCommitState>>,
}

impl ActionCommitHandle {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ActionCommitState::NotStarted)),
        }
    }

    pub fn state(&self) -> ActionCommitState {
        *lock(&self.state)
    }

    pub fn enter_pre_commit(&self) -> Result<(), DataError> {
        self.transition(ActionCommitState::NotStarted, ActionCommitState::PreCommit)
    }

    pub fn begin_commit(&self) -> Result<(), DataError> {
        let mut state = lock(&self.state);
        match *state {
            ActionCommitState::NotStarted | ActionCommitState::PreCommit => {
                *state = ActionCommitState::Committing;
                Ok(())
            }
            current => Err(invalid_transition(current, ActionCommitState::Committing)),
        }
    }

    pub fn committed(&self) -> Result<(), DataError> {
        self.transition(ActionCommitState::Committing, ActionCommitState::Committed)
    }

    pub fn failed(&self) -> Result<(), DataError> {
        let mut state = lock(&self.state);
        match *state {
            ActionCommitState::NotStarted
            | ActionCommitState::PreCommit
            | ActionCommitState::Committing => {
                *state = ActionCommitState::Failed;
                Ok(())
            }
            current => Err(invalid_transition(current, ActionCommitState::Failed)),
        }
    }

    pub fn outcome_unknown(&self) -> Result<(), DataError> {
        self.transition(
            ActionCommitState::Committing,
            ActionCommitState::OutcomeUnknown,
        )
    }

    pub fn compensation_required(&self) -> Result<(), DataError> {
        self.transition(
            ActionCommitState::Committed,
            ActionCommitState::CompensationRequired,
        )
    }

    fn transition(
        &self,
        expected: ActionCommitState,
        next: ActionCommitState,
    ) -> Result<(), DataError> {
        let mut state = lock(&self.state);
        if *state != expected {
            return Err(invalid_transition(*state, next));
        }
        *state = next;
        Ok(())
    }
}

fn invalid_transition(from: ActionCommitState, to: ActionCommitState) -> DataError {
    DataError::InvalidActionState(format!("{from:?} -> {to:?}"))
}

#[derive(Clone)]
pub struct ActionContext {
    data: DataContext,
    policy: Arc<ActionPolicy>,
    commit: ActionCommitHandle,
}

impl ActionContext {
    pub fn data(&self) -> &DataContext {
        &self.data
    }

    pub fn policy(&self) -> &ActionPolicy {
        &self.policy
    }

    pub fn commit(&self) -> &ActionCommitHandle {
        &self.commit
    }

    pub fn record_invalidation(&self, event: InvalidationEvent) -> Result<(), DataError> {
        if self.commit.state() != ActionCommitState::Committed {
            return Err(DataError::InvalidActionState(
                "cache invalidation requires a committed action".to_owned(),
            ));
        }
        let intent = self
            .policy
            .invalidation_intents
            .iter()
            .find(|intent| intent.cache_policy_id == event.policy_id)
            .ok_or_else(|| {
                DataError::InvalidActionState(format!(
                    "cache policy {} is not declared by action {}",
                    event.policy_id, self.policy.id
                ))
            })?;
        if !event.matches_tags(intent.tags_value()) {
            return Err(DataError::InvalidActionState(format!(
                "cache invalidation target does not match action {} intent for {}",
                self.policy.id, event.policy_id
            )));
        }
        if intent.consistency == InvalidationConsistency::ReadYourWrites {
            event.require_acknowledgements().map_err(|error| {
                DataError::ActionFailure(format!("cache acknowledgement barrier failed: {error}"))
            })?;
        }
        self.data.record_invalidation(event);
        Ok(())
    }

    pub fn resource<T>(&self, id: &str) -> Result<ResourceLease<T>, DataError>
    where
        T: Send + Sync + 'static,
    {
        let requirement = self
            .policy
            .resources
            .iter()
            .find(|requirement| requirement.id() == id)
            .ok_or_else(|| DataError::ResourceUnavailable(id.to_owned()))?;
        self.data.resource(requirement)
    }
}

impl Debug for ActionContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActionContext")
            .field("action_id", &self.policy.id)
            .field("semantic_revision", &self.policy.semantic_revision)
            .field("commit_state", &self.commit.state())
            .field("route_id", &self.data.identity().route_id())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionNavigation {
    Stay,
    SeeOther(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ActionResponse<Output, FieldErrors> {
    Success {
        output: Output,
        navigation: ActionNavigation,
    },
    Invalid {
        field_errors: FieldErrors,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionFailure {
    error: DataError,
    commit_state: ActionCommitState,
}

impl ActionFailure {
    pub fn error(&self) -> &DataError {
        &self.error
    }

    pub fn commit_state(&self) -> ActionCommitState {
        self.commit_state
    }
}

impl Display for ActionFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({:?})", self.error, self.commit_state)
    }
}

impl std::error::Error for ActionFailure {}

pub struct ActionIdempotency<'a, Store> {
    manager: &'a IdempotencyManager<Store>,
    key: &'a IdempotencyKey,
    partition: &'a IdempotencyPartition,
    deployment_epoch: u32,
}

impl<'a, Store> ActionIdempotency<'a, Store>
where
    Store: IdempotencyStore,
{
    pub fn new(
        manager: &'a IdempotencyManager<Store>,
        key: &'a IdempotencyKey,
        partition: &'a IdempotencyPartition,
        deployment_epoch: u32,
    ) -> Result<Self, DataError> {
        if deployment_epoch == 0 {
            return Err(DataError::InvalidActionState(
                "idempotency deployment epoch must be non-zero".to_owned(),
            ));
        }
        Ok(Self {
            manager,
            key,
            partition,
            deployment_epoch,
        })
    }
}

struct ActionReceiptObservation<'a> {
    started: Instant,
    outcome: DataOutcome,
    output_bytes: usize,
    commit: &'a ActionCommitHandle,
    error: Option<&'a DataError>,
    deduplicated: bool,
}

impl DataContext {
    pub async fn act<Input, Output, FieldErrors, Implementation>(
        &self,
        policy: &ActionPolicy,
        admission: &ActionAdmission,
        implementation: &Implementation,
        input: Input,
    ) -> Result<ActionResponse<Output, FieldErrors>, ActionFailure>
    where
        Input: Send + 'static,
        Output: Serialize + Send + 'static,
        FieldErrors: Serialize + Send + 'static,
        Implementation: Action<Input, Output, FieldErrors>,
    {
        let started = Instant::now();
        let commit = ActionCommitHandle::new();
        if let Err(error) = self.require_action_policy(policy) {
            return Err(ActionFailure {
                error,
                commit_state: commit.state(),
            });
        }
        let admitted = admit(policy, admission);
        if let Err(error) = admitted {
            self.record_action_receipt(
                policy,
                ActionReceiptObservation {
                    started,
                    outcome: DataOutcome::Rejected,
                    output_bytes: 0,
                    commit: &commit,
                    error: Some(&error),
                    deduplicated: false,
                },
            );
            return Err(ActionFailure {
                error,
                commit_state: commit.state(),
            });
        }
        if self.is_closed() {
            return Err(ActionFailure {
                error: DataError::ContextClosed,
                commit_state: commit.state(),
            });
        }

        let context = ActionContext {
            data: self.clone(),
            policy: Arc::new(policy.clone()),
            commit: commit.clone(),
        };
        let future = implementation.execute(context, input);
        tokio::pin!(future);
        let cancellation = self.cancellation().clone();
        let deadline = tokio::time::Instant::from_std(self.deadline());

        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                finish_cancelled_action(policy, &commit, &mut future).await
            }
            _ = tokio::time::sleep_until(deadline) => {
                finish_deadline_action(policy, &commit, &mut future).await
            }
            result = &mut future => result.map_err(|error| ActionFailure {
                error,
                commit_state: commit.state(),
            }),
        };

        match result {
            Ok(response) => {
                let bytes = serde_json::to_vec(&response).map_err(|_| ActionFailure {
                    error: DataError::Serialization,
                    commit_state: commit.state(),
                })?;
                if bytes.len() > policy.max_output_bytes {
                    let error = DataError::ActionOutput {
                        actual: bytes.len(),
                        maximum: policy.max_output_bytes,
                    };
                    self.record_action_receipt(
                        policy,
                        ActionReceiptObservation {
                            started,
                            outcome: DataOutcome::Failed,
                            output_bytes: bytes.len(),
                            commit: &commit,
                            error: Some(&error),
                            deduplicated: false,
                        },
                    );
                    return Err(ActionFailure {
                        error,
                        commit_state: commit.state(),
                    });
                }
                self.record_action_receipt(
                    policy,
                    ActionReceiptObservation {
                        started,
                        outcome: DataOutcome::Success,
                        output_bytes: bytes.len(),
                        commit: &commit,
                        error: None,
                        deduplicated: false,
                    },
                );
                Ok(response)
            }
            Err(failure) => {
                let outcome = if matches!(failure.error, DataError::Cancelled | DataError::Deadline)
                {
                    DataOutcome::Cancelled
                } else {
                    DataOutcome::Failed
                };
                self.record_action_receipt(
                    policy,
                    ActionReceiptObservation {
                        started,
                        outcome,
                        output_bytes: 0,
                        commit: &commit,
                        error: Some(&failure.error),
                        deduplicated: false,
                    },
                );
                Err(failure)
            }
        }
    }

    fn record_action_receipt(
        &self,
        policy: &ActionPolicy,
        observation: ActionReceiptObservation<'_>,
    ) {
        self.record_receipt(DataReceipt {
            contract: "dev.pliegors.data-receipt/v1".to_owned(),
            operation: DataOperation::Action,
            operation_id: policy.id.clone(),
            semantic_revision: policy.semantic_revision,
            outcome: observation.outcome,
            duration_bucket: DataDurationBucket::from_duration(observation.started.elapsed()),
            output_size_bucket: DataSizeBucket::from_bytes(observation.output_bytes),
            deduplicated: observation.deduplicated,
            cancel_reason: self.cancel_reason(),
            diagnostic_code: observation
                .error
                .map(|error| format!("{}:{:?}", error.code(), observation.commit.state())),
        });
    }

    pub async fn act_idempotent<Input, Output, FieldErrors, Implementation, Store>(
        &self,
        policy: &ActionPolicy,
        admission: &ActionAdmission,
        implementation: &Implementation,
        input: Input,
        idempotency: ActionIdempotency<'_, Store>,
    ) -> Result<ActionResponse<Output, FieldErrors>, ActionFailure>
    where
        Input: Clone + Serialize + Send + 'static,
        Output: DeserializeOwned + Serialize + Send + 'static,
        FieldErrors: DeserializeOwned + Serialize + Send + 'static,
        Implementation: Action<Input, Output, FieldErrors>,
        Store: IdempotencyStore,
    {
        self.require_action_policy(policy)
            .map_err(|error| ActionFailure {
                error,
                commit_state: ActionCommitState::NotStarted,
            })?;
        if policy.idempotency_policy_id() != Some(idempotency.manager.policy().id()) {
            return Err(ActionFailure {
                error: DataError::InvalidActionState(
                    "action and idempotency policies do not match".to_owned(),
                ),
                commit_state: ActionCommitState::NotStarted,
            });
        }
        let input_bytes = canonical_json_bytes(&input).map_err(|error| ActionFailure {
            error,
            commit_state: ActionCommitState::NotStarted,
        })?;
        let input_digest = encode_hex(&sha2::Sha256::digest(&input_bytes));
        let decision = idempotency
            .manager
            .begin(
                policy.id(),
                policy.semantic_revision(),
                idempotency.deployment_epoch,
                idempotency.key,
                idempotency.partition,
                &input_digest,
            )
            .await
            .map_err(|error| ActionFailure {
                error: idempotency_data_error(error),
                commit_state: ActionCommitState::NotStarted,
            })?;
        match decision {
            IdempotencyDecision::Replay(bytes) => {
                let response = serde_json::from_slice(&bytes).map_err(|_| ActionFailure {
                    error: DataError::Serialization,
                    commit_state: ActionCommitState::Committed,
                })?;
                self.record_action_receipt(
                    policy,
                    ActionReceiptObservation {
                        started: Instant::now(),
                        outcome: DataOutcome::Success,
                        output_bytes: bytes.len(),
                        commit: &ActionCommitHandle {
                            state: Arc::new(Mutex::new(ActionCommitState::Committed)),
                        },
                        error: None,
                        deduplicated: true,
                    },
                );
                Ok(response)
            }
            IdempotencyDecision::Execute(permit) => {
                match self.act(policy, admission, implementation, input).await {
                    Ok(response) => {
                        let bytes = serde_json::to_vec(&response).map_err(|_| ActionFailure {
                            error: DataError::Serialization,
                            commit_state: ActionCommitState::Committed,
                        })?;
                        permit
                            .complete(bytes)
                            .await
                            .map_err(|error| ActionFailure {
                                error: idempotency_data_error(error),
                                commit_state: ActionCommitState::Committed,
                            })?;
                        Ok(response)
                    }
                    Err(failure) => {
                        match failure.commit_state() {
                            ActionCommitState::NotStarted
                            | ActionCommitState::PreCommit
                            | ActionCommitState::Failed => {
                                let _ = permit.abandon().await;
                            }
                            ActionCommitState::Committing
                            | ActionCommitState::Committed
                            | ActionCommitState::OutcomeUnknown
                            | ActionCommitState::CompensationRequired => {
                                let _ = permit.mark_unknown().await;
                            }
                        }
                        Err(failure)
                    }
                }
            }
        }
    }
}

fn idempotency_data_error(error: IdempotencyError) -> DataError {
    match error {
        IdempotencyError::InputConflict => DataError::ActionIdempotencyConflict,
        IdempotencyError::InProgress => DataError::ActionInProgress,
        IdempotencyError::OutcomeUnknown => DataError::ActionOutcomeUnknown,
        IdempotencyError::ResultTooLarge { actual, maximum } => {
            DataError::ActionOutput { actual, maximum }
        }
        other => DataError::ActionFailure(other.to_string()),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn admit(policy: &ActionPolicy, admission: &ActionAdmission) -> Result<(), DataError> {
    if !policy.accepts(admission.media_type) {
        return Err(DataError::ActionAdmission(
            "media type is not declared by the action".to_owned(),
        ));
    }
    if admission.decoded_bytes > policy.max_decoded_bytes {
        return Err(DataError::ActionAdmission(format!(
            "decoded input reached {} bytes; maximum is {}",
            admission.decoded_bytes, policy.max_decoded_bytes
        )));
    }
    if policy.origin_policy == OriginPolicy::SameOrigin && !admission.same_origin {
        return Err(DataError::ActionAdmission(
            "same-origin proof is required".to_owned(),
        ));
    }
    if policy.csrf_policy == CsrfPolicy::SessionBoundToken && !admission.csrf_verified {
        return Err(DataError::ActionAdmission(
            "session-bound CSRF proof is required".to_owned(),
        ));
    }
    if policy.require_authentication && !admission.authenticated {
        return Err(DataError::ActionAdmission(
            "authentication is required".to_owned(),
        ));
    }
    if policy.require_authorization && !admission.authorized {
        return Err(DataError::ActionAdmission(
            "authorization is required".to_owned(),
        ));
    }
    Ok(())
}

async fn finish_cancelled_action<Output, FieldErrors>(
    policy: &ActionPolicy,
    commit: &ActionCommitHandle,
    future: &mut ActionFuture<Output, FieldErrors>,
) -> Result<ActionResponse<Output, FieldErrors>, ActionFailure> {
    finish_interrupted_action(policy, commit, future, DataError::Cancelled).await
}

async fn finish_deadline_action<Output, FieldErrors>(
    policy: &ActionPolicy,
    commit: &ActionCommitHandle,
    future: &mut ActionFuture<Output, FieldErrors>,
) -> Result<ActionResponse<Output, FieldErrors>, ActionFailure> {
    finish_interrupted_action(policy, commit, future, DataError::Deadline).await
}

async fn finish_interrupted_action<Output, FieldErrors>(
    policy: &ActionPolicy,
    commit: &ActionCommitHandle,
    future: &mut ActionFuture<Output, FieldErrors>,
    interruption: DataError,
) -> Result<ActionResponse<Output, FieldErrors>, ActionFailure> {
    match commit.state() {
        ActionCommitState::NotStarted
        | ActionCommitState::PreCommit
        | ActionCommitState::Failed => Err(ActionFailure {
            error: interruption,
            commit_state: commit.state(),
        }),
        ActionCommitState::Committing => {
            let _ = commit.outcome_unknown();
            Err(ActionFailure {
                error: DataError::ActionOutcomeUnknown,
                commit_state: commit.state(),
            })
        }
        ActionCommitState::Committed | ActionCommitState::CompensationRequired => {
            match tokio::time::timeout(
                Duration::from_millis(policy.post_commit_grace_ms),
                future.as_mut(),
            )
            .await
            {
                Ok(result) => result.map_err(|error| ActionFailure {
                    error,
                    commit_state: commit.state(),
                }),
                Err(_) => Err(ActionFailure {
                    error: DataError::ActionOutcomeUnknown,
                    commit_state: commit.state(),
                }),
            }
        }
        ActionCommitState::OutcomeUnknown => Err(ActionFailure {
            error: DataError::ActionOutcomeUnknown,
            commit_state: ActionCommitState::OutcomeUnknown,
        }),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
