// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![forbid(unsafe_code)]

//! Transactional projections over Pliego's typed, hash-chained event log.
//!
//! [`Projection`] is the framework's single materialized-fold primitive. It is
//! reactive and incremental, but publishes state and its complete [`LogCursor`]
//! atomically only after schema resolution and the entire reducer batch succeed.
//! Snapshot restore is additive: it validates every contract binding, rebuilds
//! canonical state, and folds only the checked history tail.

mod codec;
mod projection;
mod scheduler;
mod snapshot;
mod state_root;

use pliego_log::{EventSchema, Log};
use pliego_reactive::Signal;
use serde::{Serialize, de::DeserializeOwned};

pub use codec::{
    CANONICAL_JSON_CODEC_ID, CanonicalJsonCodec, CodecError, MAX_CANONICAL_STATE_BYTES,
    MAX_CANONICAL_STATE_DEPTH, MAX_CANONICAL_STATE_NODES, StateCodec,
};
pub use pliego_log::LogCursor;
pub use projection::{Projection, ProjectionError, Reducer, ReducerError};
pub use scheduler::{
    CausalError, CausalErrorKind, CausalHandle, CausalLimits, CausalScheduler, DispatchOutcome,
    DispatchTicket, DrainReport, MAX_CONFIGURED_EVENTS_PER_DRAIN,
    MAX_CONFIGURED_EVENTS_PER_TRANSACTION, MAX_CONFIGURED_TRANSACTIONS_PER_DRAIN, ProjectionPoint,
    RuntimePhase, TRANSACTION_RECEIPT_CONTRACT_V1, TransactionFailure, TransactionId,
    TransactionOutcome, TransactionReceipt,
};
pub use snapshot::{
    CodecIdentity, MAX_CONTRACT_ID_BYTES, MAX_PROJECTION_SNAPSHOT_BYTES, ProjectionSnapshot,
    ReducerIdentity, SNAPSHOT_FORMAT_V1, SnapshotError,
};
pub use state_root::{STATE_ROOT_DOMAIN_V1, STATE_ROOT_FORMAT_V1, StateRoot};

/// Compatibility name for the one projection implementation; it does not
/// introduce a second fold or snapshot contract.
pub type Fold<S, E> = Projection<S, E>;

/// The append-only log as a reactive graph root.
#[derive(Clone, Copy)]
pub struct ReactiveLog {
    inner: Signal<Log>,
}

impl ReactiveLog {
    /// A new empty reactive log.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Signal::new(Log::new()),
        }
    }

    /// Wrap an existing checked log for restore or synchronization.
    #[must_use]
    pub fn from_log(log: Log) -> Self {
        Self {
            inner: Signal::new(log),
        }
    }

    /// Append a typed event. Serialization and schema identity are supplied by
    /// the event type rather than free-form call-site strings.
    pub fn append_typed<T>(&self, event: &T) -> Result<(), pliego_log::LogError>
    where
        T: EventSchema + Serialize + DeserializeOwned + PartialEq,
    {
        let mut result = None;
        self.inner.update(|log| {
            result = Some(log.append_typed(event).map(|_| ()));
        });
        result.expect("reactive log update callback ran")
    }

    /// Tracked access used internally by projections and by timeline tooling.
    pub fn with<R>(&self, read: impl FnOnce(&Log) -> R) -> R {
        self.inner.with(read)
    }

    /// Number of stored events.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.inner.with(Log::len)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn clone_untracked(&self) -> Log {
        self.inner.get_untracked()
    }

    pub(crate) fn publish(&self, log: Log) {
        self.inner.set(log);
    }
}

impl Default for ReactiveLog {
    fn default() -> Self {
        Self::new()
    }
}
