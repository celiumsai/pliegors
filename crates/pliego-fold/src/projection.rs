// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Transactional reactive projections over a checked event-log tail.

use std::cell::RefCell;
use std::error::Error;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use pliego_log::{CursorError, Event, Hash, LogCursor, SealedEventCatalog};
use pliego_reactive::Memo;

use crate::ReactiveLog;
use crate::codec::{CodecError, StateCodec};
use crate::snapshot::{ProjectionSnapshot, ReducerIdentity, SnapshotError, validate_contract_id};

const MAX_FAILURE_MESSAGE_BYTES: usize = 2048;

/// A schema/upcast rejection normalized for the projection boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventResolveError {
    message: String,
}

impl EventResolveError {
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

impl fmt::Display for EventResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for EventResolveError {}

/// Resolves stored events into the exact reducer input and identifies the
/// accepted schema/upcaster graph.
pub trait EventResolver<E>: 'static {
    /// Digest of every accepted `(kind, version, schema_id)` and upcast edge.
    fn schema_set_digest(&self) -> Hash;

    /// Validate and upcast one stored event.
    fn resolve(&self, event: &Event) -> Result<E, EventResolveError>;
}

impl<E: 'static> EventResolver<E> for SealedEventCatalog<E> {
    fn schema_set_digest(&self) -> Hash {
        SealedEventCatalog::schema_set_digest(self)
    }

    fn resolve(&self, event: &Event) -> Result<E, EventResolveError> {
        SealedEventCatalog::decode(self, event)
            .map_err(|error| EventResolveError::new(error.to_string()))
    }
}

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
        expected: String,
        actual: String,
    },
    NonCanonicalState,
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
                "reducer mismatch: expected {}@{}, got {}@{}",
                expected.id(),
                expected.revision(),
                actual.id(),
                actual.revision()
            ),
            Self::CodecMismatch { expected, actual } => {
                write!(f, "codec mismatch: expected {expected}, got {actual}")
            }
            Self::NonCanonicalState => {
                f.write_str("decoded snapshot state does not re-encode byte-for-byte")
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
    history: LogCursor,
    folded: u64,
}

#[derive(Clone, PartialEq)]
struct ProjectionRead<S> {
    state: S,
    error: Option<ProjectionError>,
}

/// THE PliegoRS projection primitive: a transactional, incremental reactive fold.
///
/// Each wake copies stable state to a candidate, resolves and reduces the whole
/// checked tail, and publishes `state + LogCursor` only after every event
/// succeeds. Resolver/reducer `Err` and panic both leave the stable checkpoint
/// unchanged. There is no public constructor from a `(state, position)` tuple.
pub struct Projection<S: 'static, E: 'static> {
    memo: Memo<ProjectionRead<S>>,
    stable: Rc<RefCell<Stable<S>>>,
    schema_set_digest: Hash,
    reducer_identity: ReducerIdentity,
    codec: Rc<dyn StateCodec<S>>,
    _event: std::marker::PhantomData<fn() -> E>,
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
        resolver: impl EventResolver<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        let history = log.with(|raw| raw.cursor_at(0))?;
        debug_assert_eq!(history.position, 0);
        Self::from_stable(log, initial, history, resolver, reducer, codec)
    }

    /// Restore an integrity-checked snapshot, validate every bound contract,
    /// decode/re-encode canonical state, then transactionally fold only the tail.
    pub fn restore(
        log: ReactiveLog,
        snapshot: ProjectionSnapshot,
        resolver: impl EventResolver<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        validate_contract_id("codec", codec.id())?;
        let actual_schema = resolver.schema_set_digest();
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
        if snapshot.codec_id() != codec.id() {
            return Err(ProjectionError::CodecMismatch {
                expected: snapshot.codec_id().to_owned(),
                actual: codec.id().to_owned(),
            });
        }
        log.with(|raw| raw.tail(snapshot.history()).map(|_| ()))?;
        let actual_history = *snapshot.history();
        let state = codec.decode(snapshot.state_bytes())?;
        if codec.encode(&state)? != snapshot.state_bytes() {
            return Err(ProjectionError::NonCanonicalState);
        }
        let projection = Self::from_stable(log, state, actual_history, resolver, reducer, codec)?;
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
        resolver: impl EventResolver<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        let snapshot = ProjectionSnapshot::decode(bytes)?;
        Self::restore(log, snapshot, resolver, reducer, codec)
    }

    fn from_stable(
        log: ReactiveLog,
        state: S,
        history: LogCursor,
        resolver: impl EventResolver<E>,
        reducer: Reducer<S, E>,
        codec: impl StateCodec<S>,
    ) -> Result<Self, ProjectionError> {
        validate_contract_id("codec", codec.id())?;
        let schema_set_digest = resolver.schema_set_digest();
        let reducer_identity = reducer.identity().clone();
        let stable = Rc::new(RefCell::new(Stable {
            state,
            history,
            folded: 0,
        }));
        let memo_stable = stable.clone();
        let resolver: Rc<dyn EventResolver<E>> = Rc::new(resolver);
        let memo_resolver = resolver.clone();
        let memo_reducer = reducer.clone();
        let memo = Memo::new(move || {
            match synchronize(log, &memo_stable, memo_resolver.as_ref(), &memo_reducer) {
                Ok(state) => ProjectionRead { state, error: None },
                Err(error) => ProjectionRead {
                    state: memo_stable.borrow().state.clone(),
                    error: Some(error),
                },
            }
        });
        Ok(Self {
            memo,
            stable,
            schema_set_digest,
            reducer_identity,
            codec: Rc::new(codec),
            _event: std::marker::PhantomData,
        })
    }

    /// Settle the reactive node and return state or its fail-closed error.
    pub fn try_get(&self) -> Result<S, ProjectionError> {
        let read = self.memo.get();
        match read.error {
            Some(error) => Err(error),
            None => Ok(read.state),
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
        let state = self.try_get()?;
        let stable = self.stable.borrow();
        let bytes = self.codec.encode(&state)?;
        ProjectionSnapshot::create(
            stable.history,
            self.schema_set_digest,
            self.reducer_identity.clone(),
            self.codec.id(),
            bytes,
        )
        .map_err(ProjectionError::from)
    }

    /// Exact stable history checkpoint. Failing tails do not advance it.
    pub fn history(&self) -> Result<LogCursor, ProjectionError> {
        self.try_get()?;
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
    resolver: &dyn EventResolver<E>,
    reducer: &Reducer<S, E>,
) -> Result<S, ProjectionError>
where
    S: Clone,
    E: 'static,
{
    let (mut candidate, expected) = {
        let stable = stable.borrow();
        (stable.state.clone(), stable.history)
    };
    let (events, next) = log.with(|raw| {
        let tail = raw.tail(&expected).map_err(ProjectionError::from)?;
        Ok::<_, ProjectionError>((tail.to_vec(), raw.cursor()))
    })?;
    if events.is_empty() {
        return Ok(candidate);
    }
    for stored in &events {
        let sequence = stored.seq();
        let resolved = catch_unwind(AssertUnwindSafe(|| resolver.resolve(stored)))
            .map_err(|_| ProjectionError::SchemaPanicked { sequence })?
            .map_err(|error| ProjectionError::Schema {
                sequence,
                message: error.message,
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
    let event_count = u64::try_from(events.len()).map_err(|_| ProjectionError::CounterOverflow)?;
    let published = candidate.clone();
    let mut stable = stable.borrow_mut();
    if stable.history != expected {
        return Err(ProjectionError::ConcurrentMutation);
    }
    let folded = stable
        .folded
        .checked_add(event_count)
        .ok_or(ProjectionError::CounterOverflow)?;
    stable.state = candidate;
    stable.history = next;
    stable.folded = folded;
    Ok(published)
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
