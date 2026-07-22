// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Provider-neutral request data contracts for PliegoRS.
//!
//! This crate owns typed resources, loader execution, cancellation, bounds,
//! and redacted receipts. HTTP transport and route dispatch remain in
//! `pliego-runtime`; provider SDKs remain behind application integrations.

mod action;
mod authority;
mod cache;
mod cancellation;
mod context;
mod csrf;
mod error;
mod idempotency;
mod loader;
mod outbound;
mod receipt;
mod resource;
mod secret;
mod session;
mod values;

pub use action::{
    Action, ActionAdmission, ActionCommitHandle, ActionCommitState, ActionContentEncoding,
    ActionContext, ActionFailure, ActionFuture, ActionIdempotency, ActionInvalidationIntent,
    ActionMediaType, ActionNavigation, ActionPolicy, ActionResponse, CsrfPolicy,
    InvalidationConsistency, OriginPolicy,
};
pub use authority::DataPolicyGrants;
pub use cache::{
    CacheDomain, CacheError, CacheFuture, CacheKey, CacheKeyInput, CacheLookup, CacheManager,
    CacheOutcome, CachePartition, CachePolicy, CacheReceipt, CacheSizeBucket, CacheStore,
    CacheStoreRecord, CacheTag, InMemoryCacheStore, InMemoryInvalidationCoordinator,
    InvalidationEvent, InvalidationTargetKind,
};
pub use cancellation::{DataCancelReason, DataCancellation};
pub use context::{
    DataContext, DataContextControl, DataContextOptions, DataIdentity, DataIdentityError,
};
pub use csrf::{CsrfError, CsrfManager, CsrfToken};
pub use error::DataError;
pub use idempotency::{
    IdempotencyDecision, IdempotencyError, IdempotencyFuture, IdempotencyKey, IdempotencyManager,
    IdempotencyPartition, IdempotencyPermit, IdempotencyPolicy, IdempotencyStore,
    IdempotencyStoreRecord, InMemoryIdempotencyStore,
};
pub use loader::{Loader, LoaderContext, LoaderFuture, LoaderPolicy};
pub use outbound::{
    OutboundDnsResolver, OutboundFuture, OutboundHttpError, OutboundHttpGuard, OutboundHttpPermit,
    OutboundHttpPolicy, SystemDnsResolver,
};
pub use receipt::{DataDurationBucket, DataOperation, DataOutcome, DataReceipt, DataSizeBucket};
pub use resource::{
    CapabilitySet, ResourceGrant, ResourceLease, ResourceRegistry, ResourceRegistryBuilder,
    ResourceRequirement, ResourceSpec,
};
pub use secret::{SecretError, SecretHandle};
pub use session::{
    CreatedSession, InMemorySessionStore, LoadedSession, SameSitePolicy, SessionCookie,
    SessionCookiePolicy, SessionError, SessionFuture, SessionManager, SessionPolicy, SessionStore,
    SessionToken, StoredSession,
};
pub use values::DataRequestValues;

pub(crate) const MAX_STABLE_ID_BYTES: usize = 96;

pub(crate) fn validate_stable_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_STABLE_ID_BYTES {
        return false;
    }
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.ends_with('-')
        && !value.contains("--")
}
