// SPDX-License-Identifier: Apache-2.0

use crate::DataRequestValues;
use crate::cancellation::DataCancellationControl;
use crate::loader::LoaderCell;
use crate::receipt::{DataDurationBucket, DataOperation, DataOutcome, DataSizeBucket};
use crate::resource::acquire;
use crate::{
    CacheError, CacheKey, CacheLookup, CacheManager, CacheReceipt, CacheStore, CacheTag,
    CapabilitySet, DataCancelReason, DataCancellation, DataError, DataPolicyGrants, DataReceipt,
    InvalidationEvent, ResourceGrant, ResourceLease, ResourceRegistry, ResourceRequirement,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

type Cleanup = Box<dyn FnOnce(DataCancelReason) -> Result<(), String> + Send + 'static>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataIdentity {
    request_id: String,
    route_id: String,
    deployment_id: String,
}

impl DataIdentity {
    pub fn new(
        request_id: impl Into<String>,
        route_id: impl Into<String>,
        deployment_id: impl Into<String>,
    ) -> Result<Self, DataIdentityError> {
        let request_id = request_id.into();
        let route_id = route_id.into();
        let deployment_id = deployment_id.into();
        validate_identity("request_id", &request_id)?;
        validate_identity("route_id", &route_id)?;
        validate_identity("deployment_id", &deployment_id)?;
        Ok(Self {
            request_id,
            route_id,
            deployment_id,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn route_id(&self) -> &str {
        &self.route_id
    }

    pub fn deployment_id(&self) -> &str {
        &self.deployment_id
    }
}

fn validate_identity(field: &'static str, value: &str) -> Result<(), DataIdentityError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(DataIdentityError {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataIdentityError {
    field: &'static str,
    value: String,
}

impl Display for DataIdentityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid {} {:?}", self.field, self.value)
    }
}

impl std::error::Error for DataIdentityError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataContextOptions {
    pub max_receipts: usize,
    pub max_cleanups: usize,
}

impl Default for DataContextOptions {
    fn default() -> Self {
        Self {
            max_receipts: 128,
            max_cleanups: 64,
        }
    }
}

impl DataContextOptions {
    fn validate(&self) -> Result<(), DataError> {
        if self.max_receipts == 0 || self.max_receipts > 4_096 {
            return Err(DataError::InvalidLoaderPolicy(
                "max_receipts must be between 1 and 4096".to_owned(),
            ));
        }
        if self.max_cleanups == 0 || self.max_cleanups > 1_024 {
            return Err(DataError::InvalidLoaderPolicy(
                "max_cleanups must be between 1 and 1024".to_owned(),
            ));
        }
        Ok(())
    }
}

pub(crate) struct ContextState {
    closed: AtomicBool,
}

impl ContextState {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

pub(crate) struct DataContextInner {
    pub(crate) identity: DataIdentity,
    pub(crate) deadline: Instant,
    pub(crate) cancellation: DataCancellation,
    pub(crate) state: Arc<ContextState>,
    pub(crate) resources: ResourceRegistry,
    values: DataRequestValues,
    pub(crate) grants: BTreeMap<String, CapabilitySet>,
    pub(crate) loader_cells: Mutex<BTreeMap<String, LoaderCell>>,
    receipts: Mutex<Vec<DataReceipt>>,
    cache_receipts: Mutex<Vec<CacheReceipt>>,
    invalidation_events: Mutex<Vec<InvalidationEvent>>,
    cleanups: Mutex<Vec<Cleanup>>,
    cancel_reason: Mutex<Option<DataCancelReason>>,
    options: DataContextOptions,
    policy_grants: Option<DataPolicyGrants>,
}

#[derive(Clone)]
pub struct DataContext {
    pub(crate) inner: Arc<DataContextInner>,
}

#[derive(Clone)]
pub struct DataContextControl {
    inner: Arc<DataContextInner>,
    cancellation: DataCancellationControl,
}

impl DataContext {
    pub fn open(
        identity: DataIdentity,
        deadline: Instant,
        resources: ResourceRegistry,
        grants: impl IntoIterator<Item = ResourceGrant>,
        values: DataRequestValues,
        options: DataContextOptions,
    ) -> Result<(Self, DataContextControl), DataError> {
        Self::open_with_policy_grants(identity, resources, grants, values, deadline, options, None)
    }

    pub fn open_sealed(
        identity: DataIdentity,
        deadline: Instant,
        resources: ResourceRegistry,
        grants: impl IntoIterator<Item = ResourceGrant>,
        values: DataRequestValues,
        policy_grants: DataPolicyGrants,
        options: DataContextOptions,
    ) -> Result<(Self, DataContextControl), DataError> {
        Self::open_with_policy_grants(
            identity,
            resources,
            grants,
            values,
            deadline,
            options,
            Some(policy_grants),
        )
    }

    fn open_with_policy_grants(
        identity: DataIdentity,
        resources: ResourceRegistry,
        grants: impl IntoIterator<Item = ResourceGrant>,
        values: DataRequestValues,
        deadline: Instant,
        options: DataContextOptions,
        policy_grants: Option<DataPolicyGrants>,
    ) -> Result<(Self, DataContextControl), DataError> {
        options.validate()?;
        if deadline <= Instant::now() {
            return Err(DataError::Deadline);
        }
        let mut grant_map = BTreeMap::new();
        for grant in grants {
            if grant_map
                .insert(grant.id().to_owned(), grant.capabilities().clone())
                .is_some()
            {
                return Err(DataError::DuplicateResource(grant.id().to_owned()));
            }
        }
        let (cancellation, cancellation_control) = DataCancellation::channel();
        let inner = Arc::new(DataContextInner {
            identity,
            deadline,
            cancellation,
            state: Arc::new(ContextState {
                closed: AtomicBool::new(false),
            }),
            resources,
            values,
            grants: grant_map,
            loader_cells: Mutex::new(BTreeMap::new()),
            receipts: Mutex::new(Vec::new()),
            cache_receipts: Mutex::new(Vec::new()),
            invalidation_events: Mutex::new(Vec::new()),
            cleanups: Mutex::new(Vec::new()),
            cancel_reason: Mutex::new(None),
            options,
            policy_grants,
        });
        Ok((
            Self {
                inner: inner.clone(),
            },
            DataContextControl {
                inner,
                cancellation: cancellation_control,
            },
        ))
    }

    pub(crate) fn require_loader_policy(
        &self,
        policy: &crate::LoaderPolicy,
    ) -> Result<(), DataError> {
        if self
            .inner
            .policy_grants
            .as_ref()
            .is_some_and(|grants| !grants.permits_loader(policy))
        {
            return Err(DataError::PolicyNotGranted(policy.id().to_owned()));
        }
        Ok(())
    }

    pub(crate) fn require_action_policy(
        &self,
        policy: &crate::ActionPolicy,
    ) -> Result<(), DataError> {
        if self
            .inner
            .policy_grants
            .as_ref()
            .is_some_and(|grants| !grants.permits_action(policy))
        {
            return Err(DataError::PolicyNotGranted(policy.id().to_owned()));
        }
        Ok(())
    }

    fn require_cache_policy(&self, policy: &crate::CachePolicy) -> Result<(), CacheError> {
        if self
            .inner
            .policy_grants
            .as_ref()
            .is_some_and(|grants| !grants.permits_cache(policy))
        {
            return Err(CacheError::PolicyNotGranted);
        }
        Ok(())
    }

    pub fn identity(&self) -> &DataIdentity {
        &self.inner.identity
    }

    pub fn deadline(&self) -> Instant {
        self.inner.deadline
    }

    pub fn cancellation(&self) -> &DataCancellation {
        &self.inner.cancellation
    }

    pub fn values(&self) -> &DataRequestValues {
        &self.inner.values
    }

    pub fn is_closed(&self) -> bool {
        self.inner.state.is_closed()
    }

    pub fn cancel_reason(&self) -> Option<DataCancelReason> {
        *lock(&self.inner.cancel_reason)
    }

    pub fn resource<T>(
        &self,
        requirement: &ResourceRequirement,
    ) -> Result<ResourceLease<T>, DataError>
    where
        T: Send + Sync + 'static,
    {
        let started = Instant::now();
        let result = acquire(
            &self.inner.resources,
            &self.inner.grants,
            requirement,
            self.inner.state.clone(),
            self.inner.cancellation.clone(),
            self.inner.deadline,
        );
        let (outcome, code) = match &result {
            Ok(_) => (DataOutcome::Success, None),
            Err(error) => (DataOutcome::Rejected, Some(error.code().to_owned())),
        };
        self.record_receipt(DataReceipt {
            contract: "dev.pliegors.data-receipt/v1".to_owned(),
            operation: DataOperation::ResourceLease,
            operation_id: requirement.id().to_owned(),
            semantic_revision: 1,
            outcome,
            duration_bucket: DataDurationBucket::from_duration(started.elapsed()),
            output_size_bucket: DataSizeBucket::None,
            deduplicated: false,
            cancel_reason: self.cancel_reason(),
            diagnostic_code: code,
        });
        result
    }

    pub fn register_cleanup<F>(&self, cleanup: F) -> Result<(), DataError>
    where
        F: FnOnce(DataCancelReason) -> Result<(), String> + Send + 'static,
    {
        if self.is_closed() {
            return Err(DataError::ContextClosed);
        }
        let mut cleanups = lock(&self.inner.cleanups);
        if cleanups.len() >= self.inner.options.max_cleanups {
            return Err(DataError::CleanupLimit(self.inner.options.max_cleanups));
        }
        cleanups.push(Box::new(cleanup));
        Ok(())
    }

    pub fn receipts(&self) -> Vec<DataReceipt> {
        lock(&self.inner.receipts).clone()
    }

    pub fn cache_receipts(&self) -> Vec<CacheReceipt> {
        lock(&self.inner.cache_receipts).clone()
    }

    pub fn invalidation_events(&self) -> Vec<InvalidationEvent> {
        lock(&self.inner.invalidation_events).clone()
    }

    pub(crate) fn record_invalidation(&self, event: InvalidationEvent) {
        let mut events = lock(&self.inner.invalidation_events);
        if events.len() < self.inner.options.max_receipts {
            events.push(event);
        }
    }

    pub async fn cache_lookup<Store, Value>(
        &self,
        manager: &CacheManager<Store>,
        key: &CacheKey,
    ) -> Result<CacheLookup<Value>, CacheError>
    where
        Store: CacheStore,
        Value: serde::de::DeserializeOwned,
    {
        if self.is_closed() {
            return Err(CacheError::StoreFailure);
        }
        self.require_cache_policy(manager.policy())?;
        let lookup = manager.lookup(key).await?;
        self.record_cache_receipt(lookup.receipt.clone());
        Ok(lookup)
    }

    pub async fn cache_insert<Store, Value>(
        &self,
        manager: &CacheManager<Store>,
        key: &CacheKey,
        value: &Value,
        tags: impl IntoIterator<Item = CacheTag>,
    ) -> Result<CacheReceipt, CacheError>
    where
        Store: CacheStore,
        Value: serde::Serialize,
    {
        if self.is_closed() {
            return Err(CacheError::StoreFailure);
        }
        self.require_cache_policy(manager.policy())?;
        let receipt = manager.insert(key, value, tags).await?;
        self.record_cache_receipt(receipt.clone());
        Ok(receipt)
    }

    pub async fn cache_get_or_fill<Store, Value, Fill>(
        &self,
        manager: &CacheManager<Store>,
        key: &CacheKey,
        tags: Vec<CacheTag>,
        fill: Fill,
    ) -> Result<CacheLookup<Value>, CacheError>
    where
        Store: CacheStore,
        Value: serde::de::DeserializeOwned + serde::Serialize + Send + Sync + 'static,
        Fill: Future<Output = Result<Value, CacheError>> + Send + 'static,
    {
        if self.is_closed() {
            return Err(CacheError::StoreFailure);
        }
        self.require_cache_policy(manager.policy())?;
        let lookup = manager
            .get_or_fill(
                key,
                tags,
                self.cancellation().clone(),
                self.deadline(),
                fill,
            )
            .await?;
        self.record_cache_receipt(lookup.receipt.clone());
        Ok(lookup)
    }

    pub(crate) fn record_receipt(&self, receipt: DataReceipt) {
        let mut receipts = lock(&self.inner.receipts);
        if receipts.len() < self.inner.options.max_receipts {
            receipts.push(receipt);
        }
    }

    fn record_cache_receipt(&self, receipt: CacheReceipt) {
        let mut receipts = lock(&self.inner.cache_receipts);
        if receipts.len() < self.inner.options.max_receipts {
            receipts.push(receipt);
        }
    }
}

impl Debug for DataContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DataContext")
            .field("request_id", &self.inner.identity.request_id)
            .field("route_id", &self.inner.identity.route_id)
            .field("deployment_id", &self.inner.identity.deployment_id)
            .field("closed", &self.is_closed())
            .field(
                "resource_grants",
                &self.inner.grants.keys().collect::<Vec<_>>(),
            )
            .field("receipt_count", &lock(&self.inner.receipts).len())
            .field(
                "cache_receipt_count",
                &lock(&self.inner.cache_receipts).len(),
            )
            .field(
                "invalidation_event_count",
                &lock(&self.inner.invalidation_events).len(),
            )
            .finish()
    }
}

impl DataContextControl {
    pub fn cancel(&self, reason: DataCancelReason) {
        let mut current = lock(&self.inner.cancel_reason);
        if current.is_none() {
            *current = Some(reason);
        }
        drop(current);
        self.cancellation.cancel();
    }

    pub fn close(&self) {
        if self.inner.state.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let reason = {
            let mut current = lock(&self.inner.cancel_reason);
            let reason = current.unwrap_or(DataCancelReason::ScopeClosed);
            if current.is_none() {
                *current = Some(reason);
            }
            reason
        };
        self.cancellation.cancel();
        let mut cleanups = {
            let mut registered = lock(&self.inner.cleanups);
            std::mem::take(&mut *registered)
        };
        while let Some(cleanup) = cleanups.pop() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cleanup(reason)));
        }
        lock(&self.inner.loader_cells).clear();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[allow(dead_code)]
fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
