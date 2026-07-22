// SPDX-License-Identifier: Apache-2.0

use crate::DataCancellation;
use crate::validate_stable_id;
use futures_util::FutureExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

pub type CacheFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheDomain {
    PublicRuntime,
    PrivateRequest,
    PrivateSession,
}

impl CacheDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PublicRuntime => "public-runtime",
            Self::PrivateRequest => "private-request",
            Self::PrivateSession => "private-session",
        }
    }

    fn requires_partition(self) -> bool {
        !matches!(self, Self::PublicRuntime)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachePolicy {
    id: String,
    semantic_revision: u32,
    namespace: String,
    compatibility_epoch: u32,
    domain: CacheDomain,
    required_vary: BTreeSet<String>,
    fresh_for: Duration,
    stale_for: Duration,
    max_key_bytes: usize,
    max_value_bytes: usize,
    max_tags: usize,
    coalesce_fills: bool,
    fill_timeout: Duration,
}

impl CachePolicy {
    pub fn new(
        id: impl Into<String>,
        semantic_revision: u32,
        namespace: impl Into<String>,
        compatibility_epoch: u32,
        domain: CacheDomain,
    ) -> Result<Self, CacheError> {
        let id = id.into();
        let namespace = namespace.into();
        for value in [&id, &namespace] {
            if !validate_stable_id(value) {
                return Err(CacheError::InvalidPolicy(format!(
                    "invalid stable ID {value:?}"
                )));
            }
        }
        if semantic_revision == 0 || compatibility_epoch == 0 {
            return Err(CacheError::InvalidPolicy(
                "semantic revision and compatibility epoch must be non-zero".to_owned(),
            ));
        }
        Ok(Self {
            id,
            semantic_revision,
            namespace,
            compatibility_epoch,
            domain,
            required_vary: BTreeSet::new(),
            fresh_for: Duration::from_secs(60),
            stale_for: Duration::ZERO,
            max_key_bytes: 8 * 1_024,
            max_value_bytes: 1024 * 1_024,
            max_tags: 32,
            coalesce_fills: true,
            fill_timeout: Duration::from_secs(10),
        })
    }

    pub fn vary(mut self, name: impl Into<String>) -> Result<Self, CacheError> {
        let name = name.into();
        if !validate_stable_id(&name) {
            return Err(CacheError::InvalidPolicy("invalid Vary ID".to_owned()));
        }
        if self.required_vary.len() >= 32 {
            return Err(CacheError::InvalidPolicy(
                "a cache policy may require at most 32 Vary inputs".to_owned(),
            ));
        }
        self.required_vary.insert(name);
        Ok(self)
    }

    pub fn freshness(
        mut self,
        fresh_for: Duration,
        stale_for: Duration,
    ) -> Result<Self, CacheError> {
        if fresh_for.is_zero()
            || fresh_for > Duration::from_secs(365 * 24 * 60 * 60)
            || stale_for > Duration::from_secs(24 * 60 * 60)
        {
            return Err(CacheError::InvalidPolicy(
                "cache freshness or stale window is invalid".to_owned(),
            ));
        }
        self.fresh_for = fresh_for;
        self.stale_for = stale_for;
        Ok(self)
    }

    pub fn bounds(
        mut self,
        max_key_bytes: usize,
        max_value_bytes: usize,
        max_tags: usize,
    ) -> Result<Self, CacheError> {
        if max_key_bytes == 0
            || max_key_bytes > 64 * 1_024
            || max_value_bytes == 0
            || max_value_bytes > 64 * 1_024 * 1_024
            || max_tags == 0
            || max_tags > 256
        {
            return Err(CacheError::InvalidPolicy(
                "cache key, value, or tag bounds are invalid".to_owned(),
            ));
        }
        self.max_key_bytes = max_key_bytes;
        self.max_value_bytes = max_value_bytes;
        self.max_tags = max_tags;
        Ok(self)
    }

    pub fn stampede(
        mut self,
        coalesce_fills: bool,
        fill_timeout: Duration,
    ) -> Result<Self, CacheError> {
        if fill_timeout.is_zero() || fill_timeout > Duration::from_secs(60) {
            return Err(CacheError::InvalidPolicy(
                "cache fill timeout must be between 1 ms and 60 seconds".to_owned(),
            ));
        }
        self.coalesce_fills = coalesce_fills;
        self.fill_timeout = fill_timeout;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn semantic_revision(&self) -> u32 {
        self.semantic_revision
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn compatibility_epoch(&self) -> u32 {
        self.compatibility_epoch
    }

    pub fn domain(&self) -> CacheDomain {
        self.domain
    }

    pub fn contract_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"pliego-cache-policy-v1\0");
        for value in [
            self.id.as_bytes(),
            &self.semantic_revision.to_be_bytes(),
            self.namespace.as_bytes(),
            &self.compatibility_epoch.to_be_bytes(),
            self.domain.as_str().as_bytes(),
            &self.fresh_for.as_millis().to_be_bytes(),
            &self.stale_for.as_millis().to_be_bytes(),
            &self.max_key_bytes.to_be_bytes(),
            &self.max_value_bytes.to_be_bytes(),
            &self.max_tags.to_be_bytes(),
            &[u8::from(self.coalesce_fills)],
            &self.fill_timeout.as_millis().to_be_bytes(),
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value);
        }
        for name in &self.required_vary {
            digest.update(name.as_bytes());
            digest.update([0]);
        }
        encode_hex(&digest.finalize())
    }

    pub fn key(&self, input: CacheKeyInput) -> Result<CacheKey, CacheError> {
        if input.vary.keys().cloned().collect::<BTreeSet<_>>() != self.required_vary {
            return Err(CacheError::MissingVary);
        }
        let partition_matches = match (self.domain, input.partition.as_ref()) {
            (CacheDomain::PublicRuntime, None) => true,
            (CacheDomain::PrivateRequest, Some(partition)) => {
                partition.kind == CachePartitionKind::Request
            }
            (CacheDomain::PrivateSession, Some(partition)) => {
                partition.kind == CachePartitionKind::Session
            }
            _ => false,
        };
        if !partition_matches {
            return Err(CacheError::PartitionMismatch);
        }
        let mut key_bytes = Vec::new();
        for value in [
            self.id.as_bytes(),
            &self.semantic_revision.to_be_bytes(),
            self.namespace.as_bytes(),
            &self.compatibility_epoch.to_be_bytes(),
            self.domain.as_str().as_bytes(),
            input.operation_id.as_bytes(),
            &input.operation_revision.to_be_bytes(),
            input.input_digest.as_bytes(),
        ] {
            key_bytes.extend((value.len() as u64).to_be_bytes());
            key_bytes.extend(value);
        }
        for (name, value) in &input.vary {
            for part in [name.as_bytes(), value.as_bytes()] {
                key_bytes.extend((part.len() as u64).to_be_bytes());
                key_bytes.extend(part);
            }
        }
        if let Some(partition) = &input.partition {
            key_bytes.extend((partition.digest.len() as u64).to_be_bytes());
            key_bytes.extend(partition.digest.as_bytes());
        }
        if key_bytes.len() > self.max_key_bytes {
            return Err(CacheError::KeyTooLarge {
                actual: key_bytes.len(),
                maximum: self.max_key_bytes,
            });
        }
        Ok(CacheKey {
            digest: encode_hex(&Sha256::digest(&key_bytes)),
            policy_id: self.id.clone(),
            policy_revision: self.semantic_revision,
            namespace: self.namespace.clone(),
            compatibility_epoch: self.compatibility_epoch,
            domain: self.domain,
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct CachePartition {
    digest: String,
    kind: CachePartitionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CachePartitionKind {
    Request,
    Session,
}

impl CachePartition {
    pub fn from_identity(identity: &str) -> Result<Self, CacheError> {
        if identity.is_empty()
            || identity.len() > 512
            || identity.chars().any(|value| value.is_control())
        {
            return Err(CacheError::InvalidPartition);
        }
        let mut digest = Sha256::new();
        digest.update(b"pliego-cache-session-partition-v1\0");
        digest.update((identity.len() as u64).to_be_bytes());
        digest.update(identity.as_bytes());
        Ok(Self {
            digest: encode_hex(&digest.finalize()),
            kind: CachePartitionKind::Session,
        })
    }

    pub fn from_request(request_id: &str) -> Result<Self, CacheError> {
        if request_id.is_empty()
            || request_id.len() > 512
            || request_id.chars().any(char::is_control)
        {
            return Err(CacheError::InvalidPartition);
        }
        let mut digest = Sha256::new();
        digest.update(b"pliego-cache-request-partition-v1\0");
        digest.update((request_id.len() as u64).to_be_bytes());
        digest.update(request_id.as_bytes());
        Ok(Self {
            digest: encode_hex(&digest.finalize()),
            kind: CachePartitionKind::Request,
        })
    }

    pub fn from_tenant_and_identity(tenant: &str, identity: &str) -> Result<Self, CacheError> {
        for value in [tenant, identity] {
            if value.is_empty()
                || value.len() > 512
                || value.chars().any(|character| character.is_control())
            {
                return Err(CacheError::InvalidPartition);
            }
        }
        let mut digest = Sha256::new();
        digest.update(b"pliego-cache-tenant-partition-v1\0");
        for value in [tenant.as_bytes(), identity.as_bytes()] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value);
        }
        Ok(Self {
            digest: encode_hex(&digest.finalize()),
            kind: CachePartitionKind::Session,
        })
    }
}

impl Debug for CachePartition {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CachePartition([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheKeyInput {
    operation_id: String,
    operation_revision: u32,
    input_digest: String,
    vary: BTreeMap<String, String>,
    partition: Option<CachePartition>,
}

impl CacheKeyInput {
    pub fn new(
        operation_id: impl Into<String>,
        operation_revision: u32,
        input_digest: impl Into<String>,
    ) -> Result<Self, CacheError> {
        let operation_id = operation_id.into();
        let input_digest = input_digest.into();
        if !validate_stable_id(&operation_id) || operation_revision == 0 {
            return Err(CacheError::InvalidKey(
                "invalid operation identity".to_owned(),
            ));
        }
        if input_digest.len() != 64 || !input_digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CacheError::InvalidKey("invalid input digest".to_owned()));
        }
        Ok(Self {
            operation_id,
            operation_revision,
            input_digest: input_digest.to_ascii_lowercase(),
            vary: BTreeMap::new(),
            partition: None,
        })
    }

    pub fn vary(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, CacheError> {
        let name = name.into();
        let value = value.into();
        if !validate_stable_id(&name)
            || value.len() > 2 * 1_024
            || value.chars().any(|character| character.is_control())
        {
            return Err(CacheError::InvalidKey("invalid Vary input".to_owned()));
        }
        if self.vary.insert(name.clone(), value).is_some() {
            return Err(CacheError::InvalidKey(format!(
                "duplicate Vary input {name}"
            )));
        }
        Ok(self)
    }

    pub fn partition(mut self, partition: CachePartition) -> Self {
        self.partition = Some(partition);
        self
    }
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct CacheKey {
    digest: String,
    policy_id: String,
    policy_revision: u32,
    namespace: String,
    compatibility_epoch: u32,
    domain: CacheDomain,
}

impl CacheKey {
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn domain(&self) -> CacheDomain {
        self.domain
    }
}

impl Debug for CacheKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheKey")
            .field("digest", &self.digest)
            .field("policy_id", &self.policy_id)
            .field("policy_revision", &self.policy_revision)
            .field("namespace", &self.namespace)
            .field("compatibility_epoch", &self.compatibility_epoch)
            .field("domain", &self.domain)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CacheTag(String);

impl CacheTag {
    pub fn new(value: impl Into<String>) -> Result<Self, CacheError> {
        let value = value.into();
        if !validate_stable_id(&value) {
            return Err(CacheError::InvalidTag);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheStoreRecord {
    policy_id: String,
    policy_revision: u32,
    namespace: String,
    compatibility_epoch: u32,
    domain: CacheDomain,
    value: Vec<u8>,
    tags: BTreeSet<CacheTag>,
    created_at_ms: u64,
    fresh_until_ms: u64,
    stale_until_ms: u64,
}

impl CacheStoreRecord {
    pub fn encode(&self) -> Result<Vec<u8>, CacheError> {
        serde_json::to_vec(self).map_err(|_| CacheError::StoreFailure)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CacheError> {
        serde_json::from_slice(bytes).map_err(|_| CacheError::StoreFailure)
    }
}

pub trait CacheStore: Send + Sync + 'static {
    fn get(&self, key: CacheKey) -> CacheFuture<Result<Option<CacheStoreRecord>, CacheError>>;
    fn put(&self, key: CacheKey, record: CacheStoreRecord) -> CacheFuture<Result<(), CacheError>>;
    fn remove(&self, key: CacheKey) -> CacheFuture<Result<bool, CacheError>>;
}

struct InMemoryCacheInner {
    entries: Mutex<BTreeMap<String, CacheStoreRecord>>,
    max_entries: usize,
}

#[derive(Clone)]
pub struct InMemoryCacheStore {
    inner: Arc<InMemoryCacheInner>,
}

impl InMemoryCacheStore {
    pub fn new(max_entries: usize) -> Result<Self, CacheError> {
        if max_entries == 0 || max_entries > 1_000_000 {
            return Err(CacheError::InvalidPolicy(
                "memory cache entry bound is invalid".to_owned(),
            ));
        }
        Ok(Self {
            inner: Arc::new(InMemoryCacheInner {
                entries: Mutex::new(BTreeMap::new()),
                max_entries,
            }),
        })
    }

    pub fn len(&self) -> usize {
        lock(&self.inner.entries).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn invalidate(
        &self,
        namespace: &str,
        compatibility_epoch: u32,
        tags: &BTreeSet<CacheTag>,
        exact_key: Option<&str>,
    ) -> usize {
        let mut entries = lock(&self.inner.entries);
        let before = entries.len();
        entries.retain(|key, record| {
            let exact_match = exact_key.is_some_and(|exact| exact == key);
            let tag_match = !tags.is_empty()
                && record.namespace == namespace
                && record.compatibility_epoch == compatibility_epoch
                && !record.tags.is_disjoint(tags);
            !(exact_match || tag_match)
        });
        before - entries.len()
    }
}

impl CacheStore for InMemoryCacheStore {
    fn get(&self, key: CacheKey) -> CacheFuture<Result<Option<CacheStoreRecord>, CacheError>> {
        let inner = self.inner.clone();
        Box::pin(async move { Ok(lock(&inner.entries).get(key.digest()).cloned()) })
    }

    fn put(&self, key: CacheKey, record: CacheStoreRecord) -> CacheFuture<Result<(), CacheError>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let mut entries = lock(&inner.entries);
            if !entries.contains_key(key.digest()) && entries.len() >= inner.max_entries {
                return Err(CacheError::StoreCapacity);
            }
            entries.insert(key.digest, record);
            Ok(())
        })
    }

    fn remove(&self, key: CacheKey) -> CacheFuture<Result<bool, CacheError>> {
        let inner = self.inner.clone();
        Box::pin(async move { Ok(lock(&inner.entries).remove(key.digest()).is_some()) })
    }
}

pub struct CacheManager<Store> {
    policy: CachePolicy,
    store: Arc<Store>,
    fills: Arc<Mutex<BTreeMap<String, Arc<FillState>>>>,
}

impl<Store> Clone for CacheManager<Store> {
    fn clone(&self) -> Self {
        Self {
            policy: self.policy.clone(),
            store: self.store.clone(),
            fills: self.fills.clone(),
        }
    }
}

struct FillState {
    result: Mutex<Option<Result<(), CacheError>>>,
    notify: Notify,
}

impl<Store> CacheManager<Store>
where
    Store: CacheStore,
{
    pub fn new(policy: CachePolicy, store: Store) -> Self {
        Self {
            policy,
            store: Arc::new(store),
            fills: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn policy(&self) -> &CachePolicy {
        &self.policy
    }

    pub async fn lookup<Value>(&self, key: &CacheKey) -> Result<CacheLookup<Value>, CacheError>
    where
        Value: DeserializeOwned,
    {
        self.validate_key(key)?;
        let Some(record) = self.store.get(key.clone()).await? else {
            return Ok(CacheLookup {
                value: None,
                receipt: self.receipt(key, CacheOutcome::Miss, 0, None),
            });
        };
        if !record_matches(&record, &self.policy) {
            let _ = self.store.remove(key.clone()).await;
            return Err(CacheError::VersionMismatch);
        }
        let now = unix_millis()?;
        if now >= record.stale_until_ms {
            let _ = self.store.remove(key.clone()).await;
            return Ok(CacheLookup {
                value: None,
                receipt: self.receipt(key, CacheOutcome::Miss, 0, None),
            });
        }
        let value = serde_json::from_slice(&record.value).map_err(|_| CacheError::InvalidValue)?;
        let outcome = if self.policy.domain.requires_partition() {
            CacheOutcome::Private
        } else if now < record.fresh_until_ms {
            CacheOutcome::Hit
        } else {
            CacheOutcome::Stale
        };
        Ok(CacheLookup {
            value: Some(value),
            receipt: self.receipt(key, outcome, record.value.len(), None),
        })
    }

    pub async fn insert<Value>(
        &self,
        key: &CacheKey,
        value: &Value,
        tags: impl IntoIterator<Item = CacheTag>,
    ) -> Result<CacheReceipt, CacheError>
    where
        Value: Serialize,
    {
        self.validate_key(key)?;
        let value = serde_json::to_vec(value).map_err(|_| CacheError::InvalidValue)?;
        if value.len() > self.policy.max_value_bytes {
            return Err(CacheError::ValueTooLarge {
                actual: value.len(),
                maximum: self.policy.max_value_bytes,
            });
        }
        let tags = tags.into_iter().collect::<BTreeSet<_>>();
        if tags.len() > self.policy.max_tags {
            return Err(CacheError::TooManyTags {
                actual: tags.len(),
                maximum: self.policy.max_tags,
            });
        }
        let now = unix_millis()?;
        let fresh_until_ms = now.saturating_add(duration_millis(self.policy.fresh_for)?);
        let stale_until_ms = fresh_until_ms.saturating_add(duration_millis(self.policy.stale_for)?);
        let record = CacheStoreRecord {
            policy_id: self.policy.id.clone(),
            policy_revision: self.policy.semantic_revision,
            namespace: self.policy.namespace.clone(),
            compatibility_epoch: self.policy.compatibility_epoch,
            domain: self.policy.domain,
            value,
            tags,
            created_at_ms: now,
            fresh_until_ms,
            stale_until_ms,
        };
        let size = record.value.len();
        self.store.put(key.clone(), record).await?;
        Ok(self.receipt(key, CacheOutcome::Miss, size, None))
    }

    pub async fn get_or_fill<Value, Fill>(
        &self,
        key: &CacheKey,
        tags: Vec<CacheTag>,
        cancellation: DataCancellation,
        deadline: Instant,
        fill: Fill,
    ) -> Result<CacheLookup<Value>, CacheError>
    where
        Value: DeserializeOwned + Serialize + Send + Sync + 'static,
        Fill: Future<Output = Result<Value, CacheError>> + Send + 'static,
    {
        let current = self.lookup(key).await?;
        if current.value.is_some() {
            return Ok(current);
        }
        if Instant::now() >= deadline {
            return Err(CacheError::Deadline);
        }
        if cancellation.is_cancelled() {
            return Err(CacheError::Cancelled);
        }
        if !self.policy.coalesce_fills {
            let value = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(CacheError::Cancelled),
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    return Err(CacheError::Deadline);
                }
                value = fill => value?,
            };
            self.insert(key, &value, tags).await?;
            return self.lookup(key).await;
        }

        let (state, leader) = {
            let mut fills = lock(&self.fills);
            if let Some(existing) = fills.get(key.digest()) {
                (existing.clone(), false)
            } else {
                let state = Arc::new(FillState {
                    result: Mutex::new(None),
                    notify: Notify::new(),
                });
                fills.insert(key.digest().to_owned(), state.clone());
                (state, true)
            }
        };
        if leader {
            let manager = self.clone();
            let state = state.clone();
            let key = key.clone();
            let key_digest = key.digest().to_owned();
            let timeout = self.policy.fill_timeout;
            tokio::spawn(async move {
                let filled =
                    tokio::time::timeout(timeout, AssertUnwindSafe(fill).catch_unwind()).await;
                let result = match filled {
                    Ok(Ok(Ok(value))) => manager.insert(&key, &value, tags).await.map(|_| ()),
                    Ok(Ok(Err(error))) => Err(error),
                    Ok(Err(_)) => Err(CacheError::FillPanicked),
                    Err(_) => Err(CacheError::FillTimeout),
                };
                *lock(&state.result) = Some(result);
                state.notify.notify_waiters();
                let mut fills = lock(&manager.fills);
                if fills
                    .get(&key_digest)
                    .is_some_and(|current| Arc::ptr_eq(current, &state))
                {
                    fills.remove(&key_digest);
                }
            });
        }

        loop {
            let notified = state.notify.notified();
            let result = { lock(&state.result).clone() };
            if let Some(result) = result {
                result?;
                return self.lookup(key).await;
            }
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(CacheError::Cancelled),
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    return Err(CacheError::Deadline);
                }
                _ = notified => {}
            }
        }
    }

    fn validate_key(&self, key: &CacheKey) -> Result<(), CacheError> {
        if key.policy_id != self.policy.id
            || key.policy_revision != self.policy.semantic_revision
            || key.namespace != self.policy.namespace
            || key.compatibility_epoch != self.policy.compatibility_epoch
            || key.domain != self.policy.domain
        {
            return Err(CacheError::VersionMismatch);
        }
        Ok(())
    }

    fn receipt(
        &self,
        key: &CacheKey,
        outcome: CacheOutcome,
        bytes: usize,
        invalidation_sequence: Option<u64>,
    ) -> CacheReceipt {
        CacheReceipt {
            contract: "dev.pliegors.cache-receipt/v1".to_owned(),
            policy_id: self.policy.id.clone(),
            semantic_revision: self.policy.semantic_revision,
            namespace: self.policy.namespace.clone(),
            compatibility_epoch: self.policy.compatibility_epoch,
            key_digest: key.digest.clone(),
            domain: self.policy.domain,
            outcome,
            value_size_bucket: CacheSizeBucket::from_bytes(bytes),
            invalidation_sequence,
        }
    }
}

fn record_matches(record: &CacheStoreRecord, policy: &CachePolicy) -> bool {
    record.policy_id == policy.id
        && record.policy_revision == policy.semantic_revision
        && record.namespace == policy.namespace
        && record.compatibility_epoch == policy.compatibility_epoch
        && record.domain == policy.domain
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheOutcome {
    Hit,
    Miss,
    Stale,
    Bypass,
    Private,
    Invalidated,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheSizeBucket {
    None,
    Under1Kibibyte,
    Under16Kibibytes,
    Under256Kibibytes,
    AtLeast256Kibibytes,
}

impl CacheSizeBucket {
    fn from_bytes(bytes: usize) -> Self {
        match bytes {
            0 => Self::None,
            1..=1_023 => Self::Under1Kibibyte,
            1_024..=16_383 => Self::Under16Kibibytes,
            16_384..=262_143 => Self::Under256Kibibytes,
            _ => Self::AtLeast256Kibibytes,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheReceipt {
    pub contract: String,
    pub policy_id: String,
    pub semantic_revision: u32,
    pub namespace: String,
    pub compatibility_epoch: u32,
    pub key_digest: String,
    pub domain: CacheDomain,
    pub outcome: CacheOutcome,
    pub value_size_bucket: CacheSizeBucket,
    pub invalidation_sequence: Option<u64>,
}

impl CacheReceipt {
    pub fn explain(&self) -> String {
        format!(
            "PLIEGO why cache\ncontract: {}\npolicy: {}@{}\nnamespace: {}\nepoch: {}\ndomain: {}\nkey-sha256: {}\noutcome: {}\nvalue-size: {}\ninvalidation-sequence: {}",
            self.contract,
            self.policy_id,
            self.semantic_revision,
            self.namespace,
            self.compatibility_epoch,
            self.domain.as_str(),
            self.key_digest,
            cache_outcome_label(self.outcome),
            cache_size_label(self.value_size_bucket),
            self.invalidation_sequence
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_owned())
        )
    }
}

fn cache_outcome_label(outcome: CacheOutcome) -> &'static str {
    match outcome {
        CacheOutcome::Hit => "hit",
        CacheOutcome::Miss => "miss",
        CacheOutcome::Stale => "stale",
        CacheOutcome::Bypass => "bypass",
        CacheOutcome::Private => "private",
        CacheOutcome::Invalidated => "invalidated",
        CacheOutcome::Rejected => "rejected",
    }
}

fn cache_size_label(size: CacheSizeBucket) -> &'static str {
    match size {
        CacheSizeBucket::None => "none",
        CacheSizeBucket::Under1Kibibyte => "under-1-kibibyte",
        CacheSizeBucket::Under16Kibibytes => "under-16-kibibytes",
        CacheSizeBucket::Under256Kibibytes => "under-256-kibibytes",
        CacheSizeBucket::AtLeast256Kibibytes => "at-least-256-kibibytes",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheLookup<Value> {
    pub value: Option<Value>,
    pub receipt: CacheReceipt,
}

struct InvalidationCoordinatorInner {
    sequence: AtomicU64,
    replicas: Mutex<Vec<Weak<InMemoryCacheInner>>>,
    replay: Mutex<InvalidationReplayWindow>,
}

#[derive(Default)]
struct InvalidationReplayWindow {
    events: BTreeMap<String, InvalidationEvent>,
    order: VecDeque<String>,
}

const MAX_INVALIDATION_REPLAY_EVENTS: usize = 4_096;

#[derive(Clone)]
pub struct InMemoryInvalidationCoordinator {
    inner: Arc<InvalidationCoordinatorInner>,
}

impl Default for InMemoryInvalidationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryInvalidationCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InvalidationCoordinatorInner {
                sequence: AtomicU64::new(0),
                replicas: Mutex::new(Vec::new()),
                replay: Mutex::new(InvalidationReplayWindow::default()),
            }),
        }
    }

    pub fn register(&self, store: &InMemoryCacheStore) {
        let mut replicas = lock(&self.inner.replicas);
        if replicas
            .iter()
            .filter_map(Weak::upgrade)
            .any(|current| Arc::ptr_eq(&current, &store.inner))
        {
            return;
        }
        replicas.push(Arc::downgrade(&store.inner));
    }

    pub fn invalidate_tags(
        &self,
        policy: &CachePolicy,
        tags: impl IntoIterator<Item = CacheTag>,
        cause_receipt: impl Into<String>,
    ) -> Result<InvalidationEvent, CacheError> {
        let tags = tags.into_iter().collect::<BTreeSet<_>>();
        if tags.is_empty() || tags.len() > policy.max_tags {
            return Err(CacheError::TooManyTags {
                actual: tags.len(),
                maximum: policy.max_tags,
            });
        }
        self.invalidate(policy, tags, None, cause_receipt.into())
    }

    pub fn invalidate_key(
        &self,
        policy: &CachePolicy,
        key: &CacheKey,
        cause_receipt: impl Into<String>,
    ) -> Result<InvalidationEvent, CacheError> {
        if key.policy_id != policy.id || key.compatibility_epoch != policy.compatibility_epoch {
            return Err(CacheError::VersionMismatch);
        }
        self.invalidate(
            policy,
            BTreeSet::new(),
            Some(key.digest.clone()),
            cause_receipt.into(),
        )
    }

    fn invalidate(
        &self,
        policy: &CachePolicy,
        tags: BTreeSet<CacheTag>,
        exact_key: Option<String>,
        cause_receipt: String,
    ) -> Result<InvalidationEvent, CacheError> {
        if !validate_stable_id(&cause_receipt) {
            return Err(CacheError::InvalidCause);
        }
        let replay_id = invalidation_replay_id(policy, &tags, exact_key.as_deref(), &cause_receipt);
        let mut replay = lock(&self.inner.replay);
        if let Some(event) = replay.events.get(&replay_id) {
            return Ok(event.clone());
        }
        let sequence = self.inner.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let replicas = {
            let mut registered = lock(&self.inner.replicas);
            registered.retain(|replica| replica.strong_count() > 0);
            registered
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        let mut removed_entries = 0;
        for inner in &replicas {
            let store = InMemoryCacheStore {
                inner: inner.clone(),
            };
            removed_entries += store.invalidate(
                &policy.namespace,
                policy.compatibility_epoch,
                &tags,
                exact_key.as_deref(),
            );
        }
        let event = InvalidationEvent {
            contract: "dev.pliegors.cache-invalidation/v1".to_owned(),
            sequence,
            policy_id: policy.id.clone(),
            namespace: policy.namespace.clone(),
            compatibility_epoch: policy.compatibility_epoch,
            target_kind: if exact_key.is_some() {
                InvalidationTargetKind::ExactKey
            } else {
                InvalidationTargetKind::Tags
            },
            target_digest: invalidation_target_digest(&tags, exact_key.as_deref()),
            cause_receipt,
            expected_acknowledgements: replicas.len(),
            acknowledged_replicas: replicas.len(),
            removed_entries,
        };
        replay.events.insert(replay_id.clone(), event.clone());
        replay.order.push_back(replay_id);
        while replay.order.len() > MAX_INVALIDATION_REPLAY_EVENTS {
            if let Some(expired) = replay.order.pop_front() {
                replay.events.remove(&expired);
            }
        }
        Ok(event)
    }
}

fn invalidation_target_digest(tags: &BTreeSet<CacheTag>, exact_key: Option<&str>) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-cache-invalidation-target-v1\0");
    match exact_key {
        Some(key) => {
            digest.update(b"exact-key\0");
            digest.update((key.len() as u64).to_be_bytes());
            digest.update(key.as_bytes());
        }
        None => {
            digest.update(b"tags\0");
            for tag in tags {
                digest.update((tag.0.len() as u64).to_be_bytes());
                digest.update(tag.0.as_bytes());
            }
        }
    }
    encode_hex(&digest.finalize())
}

fn invalidation_replay_id(
    policy: &CachePolicy,
    tags: &BTreeSet<CacheTag>,
    exact_key: Option<&str>,
    cause_receipt: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-cache-invalidation-delivery-v1\0");
    for value in [
        policy.id.as_bytes(),
        &policy.semantic_revision.to_be_bytes(),
        policy.namespace.as_bytes(),
        &policy.compatibility_epoch.to_be_bytes(),
        cause_receipt.as_bytes(),
        exact_key.unwrap_or("").as_bytes(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    for tag in tags {
        digest.update((tag.0.len() as u64).to_be_bytes());
        digest.update(tag.0.as_bytes());
    }
    encode_hex(&digest.finalize())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvalidationTargetKind {
    Tags,
    ExactKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvalidationEvent {
    pub contract: String,
    pub sequence: u64,
    pub policy_id: String,
    pub namespace: String,
    pub compatibility_epoch: u32,
    pub target_kind: InvalidationTargetKind,
    pub target_digest: String,
    pub cause_receipt: String,
    pub expected_acknowledgements: usize,
    pub acknowledged_replicas: usize,
    pub removed_entries: usize,
}

impl InvalidationEvent {
    pub fn matches_tags(&self, tags: &[CacheTag]) -> bool {
        let tags = tags.iter().cloned().collect::<BTreeSet<_>>();
        self.target_kind == InvalidationTargetKind::Tags
            && self.target_digest == invalidation_target_digest(&tags, None)
    }

    pub fn acknowledged(&self) -> bool {
        self.acknowledged_replicas == self.expected_acknowledgements
    }

    pub fn require_acknowledgements(&self) -> Result<(), CacheError> {
        if self.acknowledged() {
            Ok(())
        } else {
            Err(CacheError::AcknowledgementBarrier)
        }
    }

    pub fn explain(&self) -> String {
        format!(
            "PLIEGO why cache invalidation\ncontract: {}\npolicy: {}\nnamespace: {}\nepoch: {}\ntarget: {} {}\nsequence: {}\ncause: {}\nacknowledged: {}/{}\nremoved: {}",
            self.contract,
            self.policy_id,
            self.namespace,
            self.compatibility_epoch,
            match self.target_kind {
                InvalidationTargetKind::Tags => "tags",
                InvalidationTargetKind::ExactKey => "exact-key",
            },
            self.target_digest,
            self.sequence,
            self.cause_receipt,
            self.acknowledged_replicas,
            self.expected_acknowledgements,
            self.removed_entries
        )
    }
}

fn unix_millis() -> Result<u64, CacheError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CacheError::Clock)?
        .as_millis();
    u64::try_from(millis).map_err(|_| CacheError::Clock)
}

fn duration_millis(duration: Duration) -> Result<u64, CacheError> {
    u64::try_from(duration.as_millis())
        .map_err(|_| CacheError::InvalidPolicy("duration is too large".to_owned()))
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
pub enum CacheError {
    InvalidPolicy(String),
    InvalidKey(String),
    InvalidPartition,
    InvalidTag,
    InvalidCause,
    MissingVary,
    PartitionMismatch,
    PolicyNotGranted,
    KeyTooLarge { actual: usize, maximum: usize },
    ValueTooLarge { actual: usize, maximum: usize },
    TooManyTags { actual: usize, maximum: usize },
    VersionMismatch,
    InvalidValue,
    StoreCapacity,
    StoreFailure,
    Cancelled,
    Deadline,
    FillPanicked,
    FillTimeout,
    AcknowledgementBarrier,
    Clock,
}

impl CacheError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy(_) => "PLG-CAC-001",
            Self::InvalidKey(_)
            | Self::InvalidPartition
            | Self::MissingVary
            | Self::PartitionMismatch => "PLG-CAC-101",
            Self::PolicyNotGranted => "PLG-CAC-106",
            Self::InvalidTag | Self::InvalidCause | Self::TooManyTags { .. } => "PLG-CAC-102",
            Self::KeyTooLarge { .. } | Self::ValueTooLarge { .. } => "PLG-CAC-103",
            Self::VersionMismatch => "PLG-CAC-409",
            Self::InvalidValue => "PLG-CAC-201",
            Self::Cancelled | Self::Deadline => "PLG-CAC-408",
            Self::AcknowledgementBarrier => "PLG-CAC-409",
            Self::StoreCapacity
            | Self::StoreFailure
            | Self::FillPanicked
            | Self::FillTimeout
            | Self::Clock => "PLG-CAC-500",
        }
    }
}

impl Display for CacheError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy(message) => write!(formatter, "invalid cache policy: {message}"),
            Self::InvalidKey(message) => write!(formatter, "invalid cache key: {message}"),
            Self::InvalidPartition => formatter.write_str("invalid cache partition"),
            Self::InvalidTag => formatter.write_str("invalid cache tag"),
            Self::InvalidCause => formatter.write_str("invalid cache invalidation cause"),
            Self::MissingVary => {
                formatter.write_str("cache key does not contain the exact required Vary inputs")
            }
            Self::PartitionMismatch => {
                formatter.write_str("cache privacy domain and partition do not match")
            }
            Self::PolicyNotGranted => {
                formatter.write_str("cache policy is not granted to this request")
            }
            Self::KeyTooLarge { actual, maximum } => write!(
                formatter,
                "cache key reached {actual} bytes; maximum is {maximum}"
            ),
            Self::ValueTooLarge { actual, maximum } => write!(
                formatter,
                "cache value reached {actual} bytes; maximum is {maximum}"
            ),
            Self::TooManyTags { actual, maximum } => write!(
                formatter,
                "cache entry has {actual} tags; maximum is {maximum}"
            ),
            Self::VersionMismatch => formatter
                .write_str("cache policy, namespace, domain, or compatibility epoch mismatch"),
            Self::InvalidValue => formatter.write_str("cache value is invalid"),
            Self::StoreCapacity => formatter.write_str("cache store capacity is exhausted"),
            Self::StoreFailure => formatter.write_str("cache store failed"),
            Self::Cancelled => formatter.write_str("cache waiter was cancelled"),
            Self::Deadline => formatter.write_str("cache waiter exceeded its deadline"),
            Self::FillPanicked => formatter.write_str("cache fill panicked"),
            Self::FillTimeout => formatter.write_str("cache fill exceeded its deadline"),
            Self::AcknowledgementBarrier => {
                formatter.write_str("cache invalidation acknowledgement barrier was not met")
            }
            Self::Clock => formatter.write_str("cache clock is unavailable"),
        }
    }
}

impl std::error::Error for CacheError {}
