// SPDX-License-Identifier: Apache-2.0

use crate::validate_stable_id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub type IdempotencyFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdempotencyPolicy {
    id: String,
    retention: Duration,
    max_result_bytes: usize,
}

impl IdempotencyPolicy {
    pub fn new(id: impl Into<String>) -> Result<Self, IdempotencyError> {
        let id = id.into();
        if !validate_stable_id(&id) {
            return Err(IdempotencyError::InvalidPolicy(
                "invalid idempotency policy ID".to_owned(),
            ));
        }
        Ok(Self {
            id,
            retention: Duration::from_secs(24 * 60 * 60),
            max_result_bytes: 256 * 1_024,
        })
    }

    pub fn retention(mut self, retention: Duration) -> Result<Self, IdempotencyError> {
        if retention.is_zero() || retention > Duration::from_secs(7 * 24 * 60 * 60) {
            return Err(IdempotencyError::InvalidPolicy(
                "retention must be between 1 second and 7 days".to_owned(),
            ));
        }
        self.retention = retention;
        Ok(self)
    }

    pub fn max_result_bytes(mut self, maximum: usize) -> Result<Self, IdempotencyError> {
        if maximum == 0 || maximum > 16 * 1_024 * 1_024 {
            return Err(IdempotencyError::InvalidPolicy(
                "max result bytes must be between 1 and 16777216".to_owned(),
            ));
        }
        self.max_result_bytes = maximum;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdempotencyError> {
        let value = value.into();
        if value.len() < 16
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(IdempotencyError::InvalidKey);
        }
        Ok(Self(value))
    }
}

impl Debug for IdempotencyKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IdempotencyKey([REDACTED])")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct IdempotencyPartition(String);

impl IdempotencyPartition {
    pub fn from_identity(identity: &str) -> Result<Self, IdempotencyError> {
        if identity.is_empty() || identity.len() > 512 || identity.chars().any(char::is_control) {
            return Err(IdempotencyError::InvalidPartition);
        }
        Ok(Self(encode_hex(&Sha256::digest(identity.as_bytes()))))
    }
}

impl Debug for IdempotencyPartition {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IdempotencyPartition([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredState {
    InProgress,
    Completed,
    OutcomeUnknown,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdempotencyStoreRecord {
    input_digest: String,
    state: StoredState,
    result: Option<Vec<u8>>,
    expires_at_ms: u64,
}

impl IdempotencyStoreRecord {
    pub fn encode(&self) -> Result<Vec<u8>, IdempotencyError> {
        serde_json::to_vec(self).map_err(|_| IdempotencyError::StoreFailure)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, IdempotencyError> {
        serde_json::from_slice(bytes).map_err(|_| IdempotencyError::StoreFailure)
    }
}

#[derive(Clone)]
struct IdempotencyIdentity {
    scope_digest: String,
    input_digest: String,
    expires_at_ms: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub enum IdempotencyDecision {
    Execute(IdempotencyPermit),
    Replay(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdempotencyBegin {
    Acquired,
    Replay(Vec<u8>),
    InProgress,
    OutcomeUnknown,
    Conflict,
}

pub trait IdempotencyStore: Send + Sync + 'static {
    fn begin(
        &self,
        scope_digest: String,
        input_digest: String,
        expires_at_ms: u64,
    ) -> IdempotencyFuture<Result<IdempotencyBegin, IdempotencyError>>;
    fn complete(
        &self,
        scope_digest: String,
        result: Vec<u8>,
    ) -> IdempotencyFuture<Result<(), IdempotencyError>>;
    fn abandon(&self, scope_digest: String) -> IdempotencyFuture<Result<(), IdempotencyError>>;
    fn mark_unknown(&self, scope_digest: String)
    -> IdempotencyFuture<Result<(), IdempotencyError>>;
}

#[derive(Clone, Default)]
pub struct InMemoryIdempotencyStore {
    entries: Arc<Mutex<BTreeMap<String, IdempotencyStoreRecord>>>,
}

impl InMemoryIdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        lock(&self.entries).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl IdempotencyStore for InMemoryIdempotencyStore {
    fn begin(
        &self,
        scope_digest: String,
        input_digest: String,
        expires_at_ms: u64,
    ) -> IdempotencyFuture<Result<IdempotencyBegin, IdempotencyError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            let now = unix_millis()?;
            let mut entries = lock(&entries);
            if entries
                .get(&scope_digest)
                .is_some_and(|record| now >= record.expires_at_ms)
            {
                entries.remove(&scope_digest);
            }
            let Some(record) = entries.get(&scope_digest) else {
                entries.insert(
                    scope_digest,
                    IdempotencyStoreRecord {
                        input_digest,
                        state: StoredState::InProgress,
                        result: None,
                        expires_at_ms,
                    },
                );
                return Ok(IdempotencyBegin::Acquired);
            };
            if record.input_digest != input_digest {
                return Ok(IdempotencyBegin::Conflict);
            }
            Ok(match record.state {
                StoredState::InProgress => IdempotencyBegin::InProgress,
                StoredState::OutcomeUnknown => IdempotencyBegin::OutcomeUnknown,
                StoredState::Completed => IdempotencyBegin::Replay(
                    record
                        .result
                        .clone()
                        .ok_or(IdempotencyError::StoreFailure)?,
                ),
            })
        })
    }

    fn complete(
        &self,
        scope_digest: String,
        result: Vec<u8>,
    ) -> IdempotencyFuture<Result<(), IdempotencyError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            let mut entries = lock(&entries);
            let record = entries
                .get_mut(&scope_digest)
                .ok_or(IdempotencyError::StoreFailure)?;
            if record.state != StoredState::InProgress {
                return Err(IdempotencyError::StoreConflict);
            }
            record.state = StoredState::Completed;
            record.result = Some(result);
            Ok(())
        })
    }

    fn abandon(&self, scope_digest: String) -> IdempotencyFuture<Result<(), IdempotencyError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            lock(&entries).remove(&scope_digest);
            Ok(())
        })
    }

    fn mark_unknown(
        &self,
        scope_digest: String,
    ) -> IdempotencyFuture<Result<(), IdempotencyError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            let mut entries = lock(&entries);
            let record = entries
                .get_mut(&scope_digest)
                .ok_or(IdempotencyError::StoreFailure)?;
            record.state = StoredState::OutcomeUnknown;
            record.result = None;
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct IdempotencyManager<Store> {
    policy: IdempotencyPolicy,
    store: Arc<Store>,
}

impl<Store> IdempotencyManager<Store>
where
    Store: IdempotencyStore,
{
    pub fn new(policy: IdempotencyPolicy, store: Store) -> Self {
        Self {
            policy,
            store: Arc::new(store),
        }
    }

    pub fn policy(&self) -> &IdempotencyPolicy {
        &self.policy
    }

    pub async fn begin(
        &self,
        action_id: &str,
        action_revision: u32,
        deployment_epoch: u32,
        key: &IdempotencyKey,
        partition: &IdempotencyPartition,
        input_digest: &str,
    ) -> Result<IdempotencyDecision, IdempotencyError> {
        if !validate_stable_id(action_id)
            || action_revision == 0
            || deployment_epoch == 0
            || input_digest.len() != 64
            || !input_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(IdempotencyError::InvalidIdentity);
        }
        let scope_digest = scope_digest(
            &self.policy,
            action_id,
            action_revision,
            deployment_epoch,
            key,
            partition,
        );
        let expires_at_ms = unix_millis()?.saturating_add(duration_millis(self.policy.retention)?);
        let identity = IdempotencyIdentity {
            scope_digest: scope_digest.clone(),
            input_digest: input_digest.to_ascii_lowercase(),
            expires_at_ms,
        };
        match self
            .store
            .begin(
                scope_digest,
                identity.input_digest.clone(),
                identity.expires_at_ms,
            )
            .await?
        {
            IdempotencyBegin::Acquired => Ok(IdempotencyDecision::Execute(IdempotencyPermit {
                identity,
                max_result_bytes: self.policy.max_result_bytes,
                store: self.store.clone(),
            })),
            IdempotencyBegin::Replay(result) => Ok(IdempotencyDecision::Replay(result)),
            IdempotencyBegin::Conflict => Err(IdempotencyError::InputConflict),
            IdempotencyBegin::InProgress => Err(IdempotencyError::InProgress),
            IdempotencyBegin::OutcomeUnknown => Err(IdempotencyError::OutcomeUnknown),
        }
    }
}

pub struct IdempotencyPermit {
    identity: IdempotencyIdentity,
    max_result_bytes: usize,
    store: Arc<dyn IdempotencyStore>,
}

impl IdempotencyPermit {
    pub async fn complete(self, result: Vec<u8>) -> Result<(), IdempotencyError> {
        if result.len() > self.max_result_bytes {
            let _ = self.store.abandon(self.identity.scope_digest.clone()).await;
            return Err(IdempotencyError::ResultTooLarge {
                actual: result.len(),
                maximum: self.max_result_bytes,
            });
        }
        self.store
            .complete(self.identity.scope_digest, result)
            .await
    }

    pub async fn abandon(self) -> Result<(), IdempotencyError> {
        self.store.abandon(self.identity.scope_digest).await
    }

    pub async fn mark_unknown(self) -> Result<(), IdempotencyError> {
        self.store.mark_unknown(self.identity.scope_digest).await
    }
}

impl Debug for IdempotencyPermit {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdempotencyPermit")
            .field("scope_digest", &self.identity.scope_digest)
            .finish_non_exhaustive()
    }
}

impl PartialEq for IdempotencyPermit {
    fn eq(&self, other: &Self) -> bool {
        self.identity.scope_digest == other.identity.scope_digest
    }
}

impl Eq for IdempotencyPermit {}

fn scope_digest(
    policy: &IdempotencyPolicy,
    action_id: &str,
    action_revision: u32,
    deployment_epoch: u32,
    key: &IdempotencyKey,
    partition: &IdempotencyPartition,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-action-idempotency-v1\0");
    for value in [
        policy.id.as_bytes(),
        action_id.as_bytes(),
        &action_revision.to_be_bytes(),
        &deployment_epoch.to_be_bytes(),
        key.0.as_bytes(),
        partition.0.as_bytes(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    encode_hex(&digest.finalize())
}

fn unix_millis() -> Result<u64, IdempotencyError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IdempotencyError::Clock)?
        .as_millis();
    u64::try_from(millis).map_err(|_| IdempotencyError::Clock)
}

fn duration_millis(duration: Duration) -> Result<u64, IdempotencyError> {
    u64::try_from(duration.as_millis())
        .map_err(|_| IdempotencyError::InvalidPolicy("duration is too large".to_owned()))
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdempotencyError {
    InvalidPolicy(String),
    InvalidKey,
    InvalidPartition,
    InvalidIdentity,
    InputConflict,
    InProgress,
    OutcomeUnknown,
    ResultTooLarge { actual: usize, maximum: usize },
    StoreConflict,
    StoreFailure,
    Clock,
}

impl IdempotencyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy(_) => "PLG-ACT-001",
            Self::InvalidKey | Self::InvalidPartition | Self::InvalidIdentity => "PLG-ACT-106",
            Self::InputConflict => "PLG-ACT-409",
            Self::InProgress => "PLG-ACT-425",
            Self::OutcomeUnknown => "PLG-ACT-409",
            Self::ResultTooLarge { .. } => "PLG-ACT-202",
            Self::StoreConflict | Self::StoreFailure | Self::Clock => "PLG-ACT-500",
        }
    }
}

impl Display for IdempotencyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy(message) => {
                write!(formatter, "invalid idempotency policy: {message}")
            }
            Self::InvalidKey => formatter.write_str("invalid idempotency key"),
            Self::InvalidPartition => formatter.write_str("invalid idempotency partition"),
            Self::InvalidIdentity => formatter.write_str("invalid idempotency identity"),
            Self::InputConflict => {
                formatter.write_str("idempotency key was reused with different admitted input")
            }
            Self::InProgress => formatter.write_str("idempotent action is already in progress"),
            Self::OutcomeUnknown => formatter.write_str("idempotent action outcome is unknown"),
            Self::ResultTooLarge { actual, maximum } => write!(
                formatter,
                "idempotency result reached {actual} bytes; maximum is {maximum}"
            ),
            Self::StoreConflict => formatter.write_str("idempotency store conflict"),
            Self::StoreFailure => formatter.write_str("idempotency store failed"),
            Self::Clock => formatter.write_str("idempotency clock is unavailable"),
        }
    }
}

impl std::error::Error for IdempotencyError {}
