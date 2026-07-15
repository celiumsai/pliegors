// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Transactional reactive projections over a checked event-log tail.

use std::cell::RefCell;
use std::error::Error;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use pliego_log::{CursorError, Hash, LogCursor, SealedEventCatalog};
use pliego_reactive::Memo;

use crate::ReactiveLog;
use crate::codec::{CodecError, MAX_CANONICAL_STATE_BYTES, StateCodec};
use crate::snapshot::{CodecIdentity, ProjectionSnapshot, ReducerIdentity, SnapshotError};

const MAX_FAILURE_MESSAGE_BYTES: usize = 2048;

/// A deterministic application-level reducer rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducerError {
    message: String,
}

impl ReducerError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: bounded_message(message.into()),
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ReducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ReducerError {}

type ReduceFn<S, E> = dyn Fn(&mut S, &E) -> Result<(), ReducerError>;

/// An identified, revisioned, fallible projection reducer.
pub struct Reducer<S, E> {
    identity: ReducerIdentity,
    apply: Rc<ReduceFn<S, E>>,
}

impl<S, E> Clone for Reducer<S, E> {
    fn clone(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            apply: self.apply.clone(),
        }
    }
}

impl<S, E> Reducer<S, E> {
    /// Bind reducer code to an explicit identity before it may materialize state.
    #[must_use]
    pub fn new(
        identity: ReducerIdentity,
        reduce: impl Fn(&mut S, &E) -> Result<(), ReducerError> + 'static,
    ) -> Self {
        Self {
            identity,
            apply: Rc::new(reduce),
        }
    }

    /// Snapshot-bound reducer identity.
    #[must_use]
    pub fn identity(&self) -> &ReducerIdentity {
        &self.identity
    }

    fn apply(&self, state: &mut S, event: &E) -> Result<(), ReducerError> {
        (self.apply)(state, event)
    }
}

/// A fail-closed projection or restore failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    Snapshot(SnapshotError),
    Codec(CodecError),
    Cursor(CursorError),
    SchemaSetMismatch {
        expected: Hash,
        actual: Hash,
    },
    ReducerMismatch {
        expected: Box<ReducerIdentity>,
        actual: Box<ReducerIdentity>,
    },
    CodecMismatch {
        expected: Box<CodecIdentity>,
        actual: Box<CodecIdentity>,
    },
    NonCanonicalState,
    CodecPanicked {
        operation: &'static str,
    },
    Schema {
        sequence: u64,
        message: String,
    },
    SchemaPanicked {
        sequence: u64,
    },
    Reducer {
        sequence: u64,
        message: String,
    },
    ReducerPanicked {
        sequence: u64,
    },
    ConcurrentMutation,
    CounterOverflow,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => write!(f, "snapshot rejected: {error}"),
            Self::Codec(error) => write!(f, "snapshot codec rejected state: {error}"),
            Self::Cursor(error) => write!(f, "history cursor rejected: {error}"),
            Self::SchemaSetMismatch { expected, actual } => write!(
                f,
                "schema-set digest mismatch: expected {}, got {}",
                pliego_log::hex(expected),
                pliego_log::hex(actual)
            ),
            Self::ReducerMismatch { expected, actual } => write!(
                f,
                "reducer mismatch: expected {}@{} config {}, got {}@{} config {}",
                expected.id(),
                expected.revision(),
                pliego_log::hex(expected.config_hash()),
                actual.id(),
                actual.revision(),
                pliego_log::hex(actual.config_hash())
            ),
            Self::CodecMismatch { expected, actual } => {
                write!(
                    f,
                    "codec mismatch: expected {}@{} config {}, got {}@{} config {}",
                    expected.id(),
                    expected.revision(),
                    pliego_log::hex(expected.config_hash()),
                    actual.id(),
                    actual.revision(),
                    pliego_log::hex(actual.config_hash())
                )
            }
            Self::NonCanonicalState => {
                f.write_str("state codec did not round-trip state canonically")
            }
            Self::CodecPanicked { operation } => {
                write!(f, "state codec panicked during {operation}")
            }
            Self::Schema { sequence, message } => {
                write!(f, "schema rejected event {sequence}: {message}")
            }
            Self::SchemaPanicked { sequence } => {
                write!(f, "schema resolver panicked at event {sequence}")
            }
            Self::Reducer { sequence, message } => {
                write!(f, "reducer rejected event {sequence}: {message}")
            }
            Self::ReducerPanicked { sequence } => {
                write!(f, "reducer panicked at event {sequence}")
            }
            Self::ConcurrentMutation => {
                f.write_str("projection checkpoint changed during transactional reduction")
            }
            Self::CounterOverflow => f.write_str("projection event counter overflow"),
        }
    }
}

impl Error for ProjectionError {}

impl From<SnapshotError> for ProjectionError {
    fn from(error: SnapshotError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<CodecError> for ProjectionError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<CursorError> for ProjectionError {
    fn from(error: CursorError) -> Self {
        Self::Cursor(error)
    }
}

struct Stable<S> {
    state: S,
    state_bytes: Vec<u8>,
    history: LogCursor,
    folded: u64,
}

struct StableSeed<'a, S> {
    state: S,
    expected_state_bytes: Option<&'a [u8]>,
    history: LogCursor,
}

#[derive(Clone, PartialEq, Eq)]
struct ProjectionRead {
    history: LogCursor,
    error: Option<ProjectionError>,
}

/// THE PliegoRS projection primitive: a transactional, incremental reactive fold.
///
/// Each wake copies stable state to a candidate, resolves and reduces the whole
/// checked tail, and publishes `state + LogCursor` only after every event
/// succeeds. Resolver/reducer `Err` and panic both leave the stable checkpoint
/// unchanged. There is no public constructor from a `(state, position)` tuple.
pub struct Projection<S: 'static, E: 'static> {
    memo: Memo<ProjectionRead>,
    stable: Rc<RefCell<Stable<S>>>,
    schema_set_digest: Hash,
    reducer_identity: ReducerIdentity,
    codec_identity: CodecIdentity,
    _event: std::marker::PhantomData<fn() -> E>,
}

impl<S: 'static, E: 'static> Drop for Projection<S, E> {
    fn drop(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.memo.dispose()));
    }
}

impl<S, E> Projection<S, E>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
{
    /// Create a projection at genesis.
    pub fn new(
        log: ReactiveLog,
        initial: S,
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        let history = log.with(|raw| raw.cursor_at(0))?;
        debug_assert_eq!(history.position, 0);
        let codec_identity = checked_codec_identity(&codec)?;
        Self::from_stable(
            log,
            StableSeed {
                state: initial,
                expected_state_bytes: None,
                history,
            },
            catalog,
            reducer,
            codec,
            codec_identity,
        )
    }

    /// Restore an integrity-checked snapshot, validate every bound contract,
    /// decode/re-encode canonical state, then transactionally fold only the tail.
    pub fn restore(
        log: ReactiveLog,
        snapshot: ProjectionSnapshot,
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        let codec_identity = checked_codec_identity(&codec)?;
        let actual_schema = catalog.schema_set_digest();
        if snapshot.schema_set_digest() != &actual_schema {
            return Err(ProjectionError::SchemaSetMismatch {
                expected: *snapshot.schema_set_digest(),
                actual: actual_schema,
            });
        }
        if snapshot.reducer() != reducer.identity() {
            return Err(ProjectionError::ReducerMismatch {
                expected: Box::new(snapshot.reducer().clone()),
                actual: Box::new(reducer.identity().clone()),
            });
        }
        if snapshot.codec() != &codec_identity {
            return Err(ProjectionError::CodecMismatch {
                expected: Box::new(snapshot.codec().clone()),
                actual: Box::new(codec_identity),
            });
        }
        log.with(|raw| raw.tail(snapshot.history()).map(|_| ()))?;
        let actual_history = *snapshot.history();
        let state = codec_decode(&codec, snapshot.state_bytes())?;
        let projection = Self::from_stable(
            log,
            StableSeed {
                state,
                expected_state_bytes: Some(snapshot.state_bytes()),
                history: actual_history,
            },
            catalog,
            reducer,
            codec,
            snapshot.codec().clone(),
        )?;
        if let Err(error) = projection.try_get() {
            projection.dispose();
            return Err(error);
        }
        Ok(projection)
    }

    /// Decode and restore an untrusted snapshot envelope.
    pub fn restore_bytes(
        log: ReactiveLog,
        bytes: &[u8],
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        let snapshot = ProjectionSnapshot::decode(bytes)?;
        Self::restore(log, snapshot, catalog, reducer, codec)
    }

    fn from_stable(
        log: ReactiveLog,
        seed: StableSeed<'_, S>,
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
        codec_identity: CodecIdentity,
    ) -> Result<Self, ProjectionError> {
        let StableSeed {
            state,
            expected_state_bytes,
            history,
        } = seed;
        let state_bytes = canonical_state_bytes(&codec, &state)?;
        if expected_state_bytes.is_some_and(|expected| expected != state_bytes) {
            return Err(ProjectionError::NonCanonicalState);
        }
        let schema_set_digest = catalog.schema_set_digest();
        let reducer_identity = reducer.identity().clone();
        let codec: Rc<dyn StateCodec<S>> = Rc::new(codec);
        let stable = Rc::new(RefCell::new(Stable {
            state,
            state_bytes,
            history,
            folded: 0,
        }));
        let memo_stable = stable.clone();
        let catalog = Rc::new(catalog);
        let memo_catalog = catalog.clone();
        let memo_reducer = reducer.clone();
        let memo_codec = codec.clone();
        let memo = Memo::new(move || {
            match synchronize(
                log,
                &memo_stable,
                memo_catalog.as_ref(),
                &memo_reducer,
                memo_codec.as_ref(),
            ) {
                Ok(history) => ProjectionRead {
                    history,
                    error: None,
                },
                Err(error) => ProjectionRead {
                    history: memo_stable.borrow().history,
                    error: Some(error),
                },
            }
        });
        Ok(Self {
            memo,
            stable,
            schema_set_digest,
            reducer_identity,
            codec_identity,
            _event: std::marker::PhantomData,
        })
    }

    /// Settle the reactive node and return state or its fail-closed error.
    pub fn try_get(&self) -> Result<S, ProjectionError> {
        self.settle()?;
        Ok(self.stable.borrow().state.clone())
    }

    fn settle(&self) -> Result<(), ProjectionError> {
        let read = self.memo.get();
        match read.error {
            Some(error) => Err(error),
            None => {
                let stable = self.stable.borrow();
                if stable.history != read.history {
                    return Err(ProjectionError::ConcurrentMutation);
                }
                Ok(())
            }
        }
    }

    /// Tracked state read. Invalid history or reducer failure panics loudly;
    /// callers handling untrusted history should prefer [`Projection::try_get`].
    #[must_use]
    pub fn get(&self) -> S {
        self.try_get()
            .unwrap_or_else(|error| panic!("projection failed closed: {error}"))
    }

    /// Explicitly settle the tail. This is identical to `try_get` but names the
    /// durability boundary for non-UI callers.
    pub fn sync(&self) -> Result<S, ProjectionError> {
        self.try_get()
    }

    /// Create a complete, contract-bound checkpoint at the current log head.
    pub fn snapshot(&self) -> Result<ProjectionSnapshot, ProjectionError> {
        self.settle()?;
        let (history, bytes) = {
            let stable = self.stable.borrow();
            (stable.history, stable.state_bytes.clone())
        };
        ProjectionSnapshot::create(
            history,
            self.schema_set_digest,
            self.reducer_identity.clone(),
            self.codec_identity.clone(),
            bytes,
        )
        .map_err(ProjectionError::from)
    }

    /// Exact stable history checkpoint. Failing tails do not advance it.
    pub fn history(&self) -> Result<LogCursor, ProjectionError> {
        self.settle()?;
        Ok(self.stable.borrow().history)
    }

    /// Number of events committed by this process (restored prefix excluded).
    #[must_use]
    pub fn events_folded(&self) -> u64 {
        self.stable.borrow().folded
    }

    /// Last committed state without settling a currently rejected tail.
    /// Intended for diagnostics and recovery UIs; normal reactive reads use
    /// [`Projection::try_get`].
    #[must_use]
    pub fn stable_state(&self) -> S {
        self.stable.borrow().state.clone()
    }

    /// Last committed full cursor without retrying a rejected tail.
    #[must_use]
    pub fn stable_history(&self) -> LogCursor {
        self.stable.borrow().history
    }

    /// Stop the reactive projection and release graph ownership.
    pub fn dispose(&self) {
        self.memo.dispose();
    }
}

fn synchronize<S, E>(
    log: ReactiveLog,
    stable: &Rc<RefCell<Stable<S>>>,
    catalog: &SealedEventCatalog<E>,
    reducer: &Reducer<S, E>,
    codec: &dyn StateCodec<S>,
) -> Result<LogCursor, ProjectionError>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
{
    let (mut candidate, expected) = {
        let stable = stable.borrow();
        (stable.state.clone(), stable.history)
    };
    let next = log.with(|raw| {
        raw.tail(&expected).map_err(ProjectionError::from)?;
        Ok::<_, ProjectionError>(raw.cursor())
    })?;
    if next.position == expected.position {
        return Ok(expected);
    }
    let event_count = next
        .position
        .checked_sub(expected.position)
        .ok_or(ProjectionError::ConcurrentMutation)?;
    for sequence in expected.position..next.position {
        let stored = log.with(|raw| raw.get(sequence).cloned());
        let stored = stored.ok_or(ProjectionError::ConcurrentMutation)?;
        let sequence = stored.seq();
        let resolved = catch_unwind(AssertUnwindSafe(|| catalog.decode(&stored)))
            .map_err(|_| ProjectionError::SchemaPanicked { sequence })?
            .map_err(|error| ProjectionError::Schema {
                sequence,
                message: bounded_message(error.to_string()),
            })?;
        catch_unwind(AssertUnwindSafe(|| {
            reducer.apply(&mut candidate, &resolved)
        }))
        .map_err(|_| ProjectionError::ReducerPanicked { sequence })?
        .map_err(|error| ProjectionError::Reducer {
            sequence,
            message: error.message,
        })?;
    }
    let candidate_bytes = canonical_state_bytes(codec, &candidate)?;
    {
        let stable = stable.borrow();
        if candidate == stable.state && candidate_bytes != stable.state_bytes {
            return Err(ProjectionError::NonCanonicalState);
        }
    }
    let mut stable = stable.borrow_mut();
    if stable.history != expected {
        return Err(ProjectionError::ConcurrentMutation);
    }
    let folded = stable
        .folded
        .checked_add(event_count)
        .ok_or(ProjectionError::CounterOverflow)?;
    let previous_state = std::mem::replace(&mut stable.state, candidate);
    stable.state_bytes = candidate_bytes;
    stable.history = next;
    stable.folded = folded;
    drop(stable);
    drop(previous_state);
    Ok(next)
}

fn canonical_state_bytes<S>(
    codec: &dyn StateCodec<S>,
    state: &S,
) -> Result<Vec<u8>, ProjectionError>
where
    S: PartialEq + 'static,
{
    let bytes = codec_encode(codec, state)?;
    let decoded = codec_decode(codec, &bytes)?;
    if &decoded != state || codec_encode(codec, &decoded)? != bytes {
        return Err(ProjectionError::NonCanonicalState);
    }
    Ok(bytes)
}

fn codec_encode<S: 'static>(
    codec: &dyn StateCodec<S>,
    state: &S,
) -> Result<Vec<u8>, ProjectionError> {
    let bytes = catch_unwind(AssertUnwindSafe(|| codec.encode(state)))
        .map_err(|_| ProjectionError::CodecPanicked {
            operation: "encode",
        })?
        .map_err(ProjectionError::Codec)?;
    if bytes.len() > MAX_CANONICAL_STATE_BYTES {
        return Err(ProjectionError::Codec(CodecError::TooLarge {
            actual: bytes.len(),
            maximum: MAX_CANONICAL_STATE_BYTES,
        }));
    }
    Ok(bytes)
}

fn codec_decode<S: 'static>(codec: &dyn StateCodec<S>, bytes: &[u8]) -> Result<S, ProjectionError> {
    catch_unwind(AssertUnwindSafe(|| codec.decode(bytes)))
        .map_err(|_| ProjectionError::CodecPanicked {
            operation: "decode",
        })?
        .map_err(ProjectionError::Codec)
}

fn checked_codec_identity<S: 'static>(
    codec: &dyn StateCodec<S>,
) -> Result<CodecIdentity, ProjectionError> {
    let identity = catch_unwind(AssertUnwindSafe(|| codec.identity())).map_err(|_| {
        ProjectionError::CodecPanicked {
            operation: "identity",
        }
    })?;
    Ok(identity)
}

fn bounded_message(mut message: String) -> String {
    if message.len() <= MAX_FAILURE_MESSAGE_BYTES {
        return message;
    }
    let mut end = MAX_FAILURE_MESSAGE_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message
}
