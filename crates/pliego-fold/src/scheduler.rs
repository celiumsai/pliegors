// SPDX-License-Identifier: AGPL-3.0-only

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use pliego_log::{EventSchema, Log, LogCursor, LogError, SealedEventCatalog};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Projection, ProjectionError, ReactiveLog, Reducer, StateCodec, StateRoot};

/// First deterministic transaction receipt contract.
pub const TRANSACTION_RECEIPT_CONTRACT_V1: &str = "dev.pliegors.fold-transaction-receipt/v1";
/// Hard ceiling for one causal transaction.
pub const MAX_CONFIGURED_EVENTS_PER_TRANSACTION: usize = 1_024;
/// Hard ceiling for transactions executed by one drain.
pub const MAX_CONFIGURED_TRANSACTIONS_PER_DRAIN: usize = 4_096;
/// Hard ceiling for events executed by one drain.
pub const MAX_CONFIGURED_EVENTS_PER_DRAIN: usize = 65_536;

/// Bounded work policy for one causal drain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CausalLimits {
    pub max_events_per_transaction: usize,
    pub max_transactions_per_drain: usize,
    pub max_events_per_drain: usize,
}

impl Default for CausalLimits {
    fn default() -> Self {
        Self {
            max_events_per_transaction: 64,
            max_transactions_per_drain: 256,
            max_events_per_drain: 4_096,
        }
    }
}

impl CausalLimits {
    fn validate(self) -> Result<Self, CausalErrorKind> {
        let valid = self.max_events_per_transaction > 0
            && self.max_transactions_per_drain > 0
            && self.max_events_per_drain > 0
            && self.max_events_per_transaction <= self.max_events_per_drain
            && self.max_events_per_transaction <= MAX_CONFIGURED_EVENTS_PER_TRANSACTION
            && self.max_transactions_per_drain <= MAX_CONFIGURED_TRANSACTIONS_PER_DRAIN
            && self.max_events_per_drain <= MAX_CONFIGURED_EVENTS_PER_DRAIN;
        valid.then_some(self).ok_or(CausalErrorKind::InvalidLimits)
    }
}

/// One-based transaction sequence local to a scheduler incarnation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TransactionId(u64);

impl TransactionId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Observable phase of the causal transaction kernel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimePhase {
    Idle,
    Collecting,
    Reducing,
    Planning,
    Committing,
    RunningEffects,
    RecordingReceipt,
    Recovering,
}

/// Queue assignment returned before an event executes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchTicket {
    pub transaction: TransactionId,
    pub ordinal: u32,
}

/// Exact projection position bound by a transaction receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionPoint {
    pub history: LogCursor,
    pub state_root: StateRoot,
}

/// Whether projection state was published.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionOutcome {
    Committed,
    Rejected,
}

/// Bounded failure category without payload or state disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionFailure {
    EventRejected,
    EventPanicked,
    ProjectionRejected,
    EffectPanicked,
    StateDropPanicked,
}

/// Deterministic receipt for one attempted event transaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub contract: String,
    pub transaction_id: TransactionId,
    pub outcome: TransactionOutcome,
    pub attempted_events: u32,
    pub committed_events: u32,
    pub before: ProjectionPoint,
    pub after: ProjectionPoint,
    pub failure: Option<TransactionFailure>,
}

/// Receipts emitted by one iterative drain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DrainReport {
    pub receipts: Vec<TransactionReceipt>,
}

/// Whether dispatch started a drain or joined an active generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchOutcome {
    Queued(DispatchTicket),
    Drained {
        ticket: DispatchTicket,
        report: DrainReport,
    },
}

/// Fail-closed scheduler error category.
#[derive(Debug)]
pub enum CausalErrorKind {
    SchedulerDropped,
    InvalidLimits,
    ReentrantDrain,
    QueueLimit { maximum: usize },
    TransactionLimit { maximum: usize },
    EventLimit { maximum: usize },
    TransactionIdExhausted,
    EventRejected(LogError),
    EventPanicked { transaction: TransactionId },
    ProjectionRejected(ProjectionError),
    EffectPanicked { transaction: TransactionId },
    StateDropPanicked { transaction: TransactionId },
}

/// Scheduler failure plus every receipt emitted before return.
#[derive(Debug)]
pub struct CausalError {
    kind: CausalErrorKind,
    receipts: Vec<TransactionReceipt>,
}

impl CausalError {
    #[must_use]
    pub fn kind(&self) -> &CausalErrorKind {
        &self.kind
    }

    #[must_use]
    pub fn receipts(&self) -> &[TransactionReceipt] {
        &self.receipts
    }

    fn new(kind: CausalErrorKind) -> Self {
        Self {
            kind,
            receipts: Vec::new(),
        }
    }

    fn with_receipts(kind: CausalErrorKind, receipts: Vec<TransactionReceipt>) -> Self {
        Self { kind, receipts }
    }
}

impl fmt::Display for CausalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            CausalErrorKind::SchedulerDropped => {
                formatter.write_str("causal scheduler was dropped")
            }
            CausalErrorKind::InvalidLimits => {
                formatter.write_str("causal scheduler limits are invalid")
            }
            CausalErrorKind::ReentrantDrain => {
                formatter.write_str("causal drain cannot re-enter itself")
            }
            CausalErrorKind::QueueLimit { maximum } => {
                write!(formatter, "causal queue exceeds {maximum} events")
            }
            CausalErrorKind::TransactionLimit { maximum } => {
                write!(formatter, "causal drain exceeds {maximum} transactions")
            }
            CausalErrorKind::EventLimit { maximum } => {
                write!(formatter, "causal drain exceeds {maximum} events")
            }
            CausalErrorKind::TransactionIdExhausted => {
                formatter.write_str("causal transaction IDs are exhausted")
            }
            CausalErrorKind::EventRejected(error) => {
                write!(formatter, "typed event rejected: {error}")
            }
            CausalErrorKind::EventPanicked { transaction } => write!(
                formatter,
                "typed event panicked in transaction {}",
                transaction.get()
            ),
            CausalErrorKind::ProjectionRejected(error) => {
                write!(formatter, "projection rejected transaction: {error}")
            }
            CausalErrorKind::EffectPanicked { transaction } => write!(
                formatter,
                "effect panicked after transaction {} committed",
                transaction.get()
            ),
            CausalErrorKind::StateDropPanicked { transaction } => write!(
                formatter,
                "previous state drop panicked after transaction {} committed",
                transaction.get()
            ),
        }
    }
}

impl Error for CausalError {}

trait PendingEvent {
    fn append(self: Box<Self>, log: &mut Log) -> Result<(), LogError>;
}

struct TypedPendingEvent<T>(T);

impl<T> PendingEvent for TypedPendingEvent<T>
where
    T: EventSchema + Serialize + DeserializeOwned + PartialEq + 'static,
{
    fn append(self: Box<Self>, log: &mut Log) -> Result<(), LogError> {
        log.append_typed(&self.0).map(|_| ())
    }
}

struct QueuedEvent {
    ticket: DispatchTicket,
    event: Box<dyn PendingEvent>,
}

type TransactionSuccess = (TransactionReceipt, Option<CausalErrorKind>);
type TransactionFailureResult = Box<(TransactionReceipt, CausalErrorKind)>;

struct CausalCore {
    phase: RuntimePhase,
    active: Option<TransactionId>,
    next_transaction: u64,
    ready: VecDeque<QueuedEvent>,
    deferred: VecDeque<QueuedEvent>,
    draining: bool,
}

struct CausalInner<S: 'static, E: 'static> {
    core: RefCell<CausalCore>,
    log: ReactiveLog,
    projection: Projection<S, E>,
    limits: CausalLimits,
}

/// Sole-writer coordinator for event, projection, effect, and receipt causality.
pub struct CausalScheduler<S: 'static, E: 'static> {
    inner: Rc<CausalInner<S, E>>,
}

/// Weak event-dispatch handle intended for reactive effects.
pub struct CausalHandle<S: 'static, E: 'static> {
    inner: Weak<CausalInner<S, E>>,
}

impl<S, E> CausalScheduler<S, E>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
{
    pub fn new<C>(
        initial: S,
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: C,
    ) -> Result<Self, CausalError>
    where
        C: StateCodec<S>,
    {
        Self::from_log(
            Log::new(),
            initial,
            catalog,
            reducer,
            codec,
            CausalLimits::default(),
        )
    }

    pub fn with_limits<C>(
        initial: S,
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: C,
        limits: CausalLimits,
    ) -> Result<Self, CausalError>
    where
        C: StateCodec<S>,
    {
        Self::from_log(Log::new(), initial, catalog, reducer, codec, limits)
    }

    pub fn from_log<C>(
        history: Log,
        initial: S,
        catalog: SealedEventCatalog<E>,
        reducer: Reducer<S, E>,
        codec: C,
        limits: CausalLimits,
    ) -> Result<Self, CausalError>
    where
        C: StateCodec<S>,
    {
        let limits = limits.validate().map_err(CausalError::new)?;
        history
            .verify()
            .map_err(|error| CausalError::new(CausalErrorKind::EventRejected(error)))?;
        let log = ReactiveLog::from_log(history);
        let projection = Projection::new(log, initial, catalog, reducer, codec)
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))?;
        projection
            .try_get()
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))?;
        Ok(Self {
            inner: Rc::new(CausalInner {
                core: RefCell::new(CausalCore {
                    phase: RuntimePhase::Idle,
                    active: None,
                    next_transaction: 1,
                    ready: VecDeque::new(),
                    deferred: VecDeque::new(),
                    draining: false,
                }),
                log,
                projection,
                limits,
            }),
        })
    }

    #[must_use]
    pub fn handle(&self) -> CausalHandle<S, E> {
        CausalHandle {
            inner: Rc::downgrade(&self.inner),
        }
    }

    #[must_use]
    pub fn phase(&self) -> RuntimePhase {
        self.inner.core.borrow().phase
    }

    pub fn state(&self) -> Result<S, CausalError> {
        self.inner
            .projection
            .try_get()
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))
    }

    pub fn state_root(&self) -> Result<StateRoot, CausalError> {
        self.inner
            .projection
            .state_root()
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))
    }

    pub fn history(&self) -> Result<LogCursor, CausalError> {
        self.inner
            .projection
            .history()
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))
    }

    pub fn dispatch<T>(&self, event: T) -> Result<DispatchOutcome, CausalError>
    where
        T: EventSchema + Serialize + DeserializeOwned + PartialEq + 'static,
    {
        let ticket = enqueue(&self.inner, event)?;
        if self.inner.core.borrow().draining {
            return Ok(DispatchOutcome::Queued(ticket));
        }
        self.drain()
            .map(|report| DispatchOutcome::Drained { ticket, report })
    }

    pub fn drain(&self) -> Result<DrainReport, CausalError> {
        if self.inner.core.borrow().draining {
            return Err(CausalError::new(CausalErrorKind::ReentrantDrain));
        }
        self.inner.core.borrow_mut().draining = true;
        let mut guard = DrainGuard::new(&self.inner.core);
        let mut receipts = Vec::new();
        let mut transactions = 0usize;
        let mut events = 0usize;
        let mut post_commit_error = None;

        loop {
            let (transaction, batch) = take_transaction(&self.inner)?;
            let Some(transaction) = transaction else {
                break;
            };
            let batch = batch.expect("transaction and batch are paired");
            transactions += 1;
            events = events.saturating_add(batch.len());
            if transactions > self.inner.limits.max_transactions_per_drain {
                clear_queues(&self.inner.core);
                guard.finish();
                return Err(CausalError::with_receipts(
                    CausalErrorKind::TransactionLimit {
                        maximum: self.inner.limits.max_transactions_per_drain,
                    },
                    receipts,
                ));
            }
            if events > self.inner.limits.max_events_per_drain {
                clear_queues(&self.inner.core);
                guard.finish();
                return Err(CausalError::with_receipts(
                    CausalErrorKind::EventLimit {
                        maximum: self.inner.limits.max_events_per_drain,
                    },
                    receipts,
                ));
            }

            match execute_transaction(&self.inner, transaction, batch) {
                Ok((receipt, failure)) => {
                    if post_commit_error.is_none() {
                        post_commit_error = failure;
                    }
                    receipts.push(receipt);
                    advance_generation(&self.inner.core);
                }
                Err(failure) => {
                    let (receipt, kind) = *failure;
                    receipts.push(receipt);
                    clear_queues(&self.inner.core);
                    guard.finish();
                    return Err(CausalError::with_receipts(kind, receipts));
                }
            }
        }

        guard.finish();
        if let Some(kind) = post_commit_error {
            Err(CausalError::with_receipts(kind, receipts))
        } else {
            Ok(DrainReport { receipts })
        }
    }
}

impl<S, E> CausalHandle<S, E>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
{
    pub fn state(&self) -> Result<S, CausalError> {
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| CausalError::new(CausalErrorKind::SchedulerDropped))?;
        inner
            .projection
            .try_get()
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))
    }

    pub fn state_root(&self) -> Result<StateRoot, CausalError> {
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| CausalError::new(CausalErrorKind::SchedulerDropped))?;
        inner
            .projection
            .state_root()
            .map_err(|error| CausalError::new(CausalErrorKind::ProjectionRejected(error)))
    }

    pub fn enqueue<T>(&self, event: T) -> Result<DispatchTicket, CausalError>
    where
        T: EventSchema + Serialize + DeserializeOwned + PartialEq + 'static,
    {
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| CausalError::new(CausalErrorKind::SchedulerDropped))?;
        enqueue(&inner, event)
    }
}

fn enqueue<S, E, T>(inner: &Rc<CausalInner<S, E>>, event: T) -> Result<DispatchTicket, CausalError>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
    T: EventSchema + Serialize + DeserializeOwned + PartialEq + 'static,
{
    let mut core = inner.core.borrow_mut();
    let active = core.active;
    let transaction = match active {
        Some(active) => TransactionId(
            active
                .0
                .checked_add(1)
                .ok_or_else(|| CausalError::new(CausalErrorKind::TransactionIdExhausted))?,
        ),
        None => TransactionId(core.next_transaction),
    };
    let queue = if active.is_some() {
        &mut core.deferred
    } else {
        &mut core.ready
    };
    if queue.len() >= inner.limits.max_events_per_transaction {
        return Err(CausalError::new(CausalErrorKind::QueueLimit {
            maximum: inner.limits.max_events_per_transaction,
        }));
    }
    let ordinal = u32::try_from(queue.len() + 1).expect("causal queue ceiling fits in u32");
    let ticket = DispatchTicket {
        transaction,
        ordinal,
    };
    queue.push_back(QueuedEvent {
        ticket,
        event: Box::new(TypedPendingEvent(event)),
    });
    Ok(ticket)
}

fn take_transaction<S, E>(
    inner: &Rc<CausalInner<S, E>>,
) -> Result<(Option<TransactionId>, Option<Vec<QueuedEvent>>), CausalError>
where
    S: 'static,
    E: 'static,
{
    let mut core = inner.core.borrow_mut();
    if core.ready.is_empty() {
        return Ok((None, None));
    }
    if core.next_transaction == u64::MAX {
        return Err(CausalError::new(CausalErrorKind::TransactionIdExhausted));
    }
    let id = TransactionId(core.next_transaction);
    core.next_transaction = core
        .next_transaction
        .checked_add(1)
        .ok_or_else(|| CausalError::new(CausalErrorKind::TransactionIdExhausted))?;
    core.active = Some(id);
    core.phase = RuntimePhase::Collecting;
    let batch = core.ready.drain(..).collect::<Vec<_>>();
    debug_assert!(batch.iter().all(|queued| queued.ticket.transaction == id));
    Ok((Some(id), Some(batch)))
}

fn execute_transaction<S, E>(
    inner: &Rc<CausalInner<S, E>>,
    transaction: TransactionId,
    batch: Vec<QueuedEvent>,
) -> Result<TransactionSuccess, TransactionFailureResult>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
{
    let attempted = u32::try_from(batch.len()).expect("causal queue ceiling fits in u32");
    let before = current_point(inner).expect("settled scheduler point");
    let mut candidate = inner.log.clone_untracked();
    for queued in batch {
        match catch_unwind(AssertUnwindSafe(|| queued.event.append(&mut candidate))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                return Err(Box::new((
                    rejected_receipt(
                        transaction,
                        attempted,
                        before,
                        TransactionFailure::EventRejected,
                    ),
                    CausalErrorKind::EventRejected(error),
                )));
            }
            Err(payload) => {
                std::mem::forget(payload);
                return Err(Box::new((
                    rejected_receipt(
                        transaction,
                        attempted,
                        before,
                        TransactionFailure::EventPanicked,
                    ),
                    CausalErrorKind::EventPanicked { transaction },
                )));
            }
        }
    }

    set_phase(&inner.core, RuntimePhase::Reducing);
    let prepared = match inner.projection.prepare(&candidate) {
        Ok(prepared) => prepared,
        Err(error) => {
            return Err(Box::new((
                rejected_receipt(
                    transaction,
                    attempted,
                    before,
                    TransactionFailure::ProjectionRejected,
                ),
                CausalErrorKind::ProjectionRejected(error),
            )));
        }
    };
    set_phase(&inner.core, RuntimePhase::Planning);
    let after = ProjectionPoint {
        history: prepared.history(),
        state_root: prepared.state_root(),
    };
    set_phase(&inner.core, RuntimePhase::Committing);
    let previous = inner
        .projection
        .commit_prepared(prepared)
        .map_err(|error| {
            Box::new((
                rejected_receipt(
                    transaction,
                    attempted,
                    before.clone(),
                    TransactionFailure::ProjectionRejected,
                ),
                CausalErrorKind::ProjectionRejected(error),
            ))
        })?;

    set_phase(&inner.core, RuntimePhase::RunningEffects);
    let effect_panic = catch_unwind(AssertUnwindSafe(|| inner.log.publish(candidate))).err();
    let state_drop_panic = catch_unwind(AssertUnwindSafe(|| drop(previous))).err();
    let effect_panicked = effect_panic.is_some();
    let state_drop_panicked = state_drop_panic.is_some();
    if let Some(payload) = effect_panic {
        std::mem::forget(payload);
    }
    if let Some(payload) = state_drop_panic {
        std::mem::forget(payload);
    }

    let (failure, error) = if effect_panicked {
        (
            Some(TransactionFailure::EffectPanicked),
            Some(CausalErrorKind::EffectPanicked { transaction }),
        )
    } else if state_drop_panicked {
        (
            Some(TransactionFailure::StateDropPanicked),
            Some(CausalErrorKind::StateDropPanicked { transaction }),
        )
    } else {
        (None, None)
    };
    set_phase(&inner.core, RuntimePhase::RecordingReceipt);
    Ok((
        TransactionReceipt {
            contract: TRANSACTION_RECEIPT_CONTRACT_V1.to_owned(),
            transaction_id: transaction,
            outcome: TransactionOutcome::Committed,
            attempted_events: attempted,
            committed_events: attempted,
            before,
            after,
            failure,
        },
        error,
    ))
}

fn current_point<S, E>(inner: &CausalInner<S, E>) -> Result<ProjectionPoint, ProjectionError>
where
    S: Clone + PartialEq + 'static,
    E: 'static,
{
    Ok(ProjectionPoint {
        history: inner.projection.history()?,
        state_root: inner.projection.state_root()?,
    })
}

fn rejected_receipt(
    transaction: TransactionId,
    attempted: u32,
    point: ProjectionPoint,
    failure: TransactionFailure,
) -> TransactionReceipt {
    TransactionReceipt {
        contract: TRANSACTION_RECEIPT_CONTRACT_V1.to_owned(),
        transaction_id: transaction,
        outcome: TransactionOutcome::Rejected,
        attempted_events: attempted,
        committed_events: 0,
        before: point.clone(),
        after: point,
        failure: Some(failure),
    }
}

fn advance_generation(core: &RefCell<CausalCore>) {
    let mut core = core.borrow_mut();
    core.ready = std::mem::take(&mut core.deferred);
    core.active = None;
    core.phase = RuntimePhase::Idle;
}

fn set_phase(core: &RefCell<CausalCore>, phase: RuntimePhase) {
    core.borrow_mut().phase = phase;
}

fn clear_queues(core: &RefCell<CausalCore>) {
    let mut core = core.borrow_mut();
    core.ready.clear();
    core.deferred.clear();
    core.active = None;
}

struct DrainGuard<'a> {
    core: &'a RefCell<CausalCore>,
    active: Cell<bool>,
}

impl<'a> DrainGuard<'a> {
    fn new(core: &'a RefCell<CausalCore>) -> Self {
        Self {
            core,
            active: Cell::new(true),
        }
    }

    fn finish(&mut self) {
        let mut core = self.core.borrow_mut();
        core.phase = RuntimePhase::Idle;
        core.active = None;
        core.draining = false;
        self.active.set(false);
    }
}

impl Drop for DrainGuard<'_> {
    fn drop(&mut self) {
        if !self.active.get() {
            return;
        }
        let mut core = self.core.borrow_mut();
        core.phase = RuntimePhase::Recovering;
        core.ready.clear();
        core.deferred.clear();
        core.active = None;
        core.draining = false;
        core.phase = RuntimePhase::Idle;
    }
}
