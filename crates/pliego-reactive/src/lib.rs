// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![forbid(unsafe_code)]

//! pliego-reactive — the reactive graph under PliegoRS (M2, docs/00 §2).
//!
//! Principles learned from Leptos / Reactively (our implementation, sized to our
//! model — see the founding spec's Kaizen section):
//!
//! - **Runtime dependency tracking.** A thread-local *observer*: reading a
//!   reactive value while a computation runs registers a bidirectional edge.
//!   Dependencies are dynamic — a branch not taken this run is not a dependency.
//! - **Two-phase coloring (push-pull).** A write marks direct subscribers
//!   **Dirty** and deeper descendants **Check** — cheap, nothing recomputes.
//!   Reads pull lazily: a Check node asks its sources `update_if_necessary`; if
//!   none actually changed it becomes Clean without recomputing.
//! - **Equality gating.** A memo that recomputes to an equal value does not wake
//!   its subscribers. In PliegoRS's topology (ONE log at the root fanning out to
//!   a forest of folds) this is what keeps an append from recomputing the world.
//! - **Ownership.** Nodes created while a computation runs are owned by it and
//!   disposed (with their cleanups) when the owner re-runs or is disposed.
//!
//! Single-threaded by design (the browser main thread / one WASM instance); the
//! runtime lives in a thread-local, handles are `Copy` ids.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

// ───────────────────────────── runtime plumbing ─────────────────────────────

/// Stable handle into the runtime arena.
///
/// Slots are reused after disposal, while the generation prevents an old
/// handle from resolving to the new occupant of the same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    slot: u32,
    generation: u32,
}

/// A fail-closed error from an explicit [`Owner`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerError {
    /// The owner handle no longer resolves to its original arena slot.
    Disposed,
    /// The resource being adopted has already been disposed.
    ResourceDisposed,
    /// A running resource cannot change lifecycle ownership.
    ResourceBusy,
    /// A resource can belong to only one owner at a time.
    AlreadyOwned,
}

impl std::fmt::Display for OwnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disposed => f.write_str("reactive owner is disposed"),
            Self::ResourceDisposed => f.write_str("reactive resource is disposed"),
            Self::ResourceBusy => f.write_str("reactive resource is currently running"),
            Self::AlreadyOwned => f.write_str("reactive resource already has an owner"),
        }
    }
}

impl std::error::Error for OwnerError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Color {
    /// Value is current.
    Clean,
    /// A transitive source changed; whether *my* inputs changed is unknown.
    Check,
    /// A direct source changed; recompute before next read.
    Dirty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Owner,
    Signal,
    Memo,
    Effect,
}

type Value = Rc<dyn Any>;
type Compute = Rc<dyn Fn() -> Value>;
type EqFn = Rc<dyn Fn(&dyn Any, &dyn Any) -> bool>;
type Cleanup = Box<dyn FnOnce()>;
type PanicPayload = Box<dyn Any + Send>;

const MIN_EFFECT_RUNS_PER_FLUSH: usize = 1_024;
const EFFECT_RUNS_PER_LIVE_NODE: usize = 256;
const MAX_EFFECT_RUNS_PER_FLUSH: usize = 1_000_000;

struct Node {
    kind: Kind,
    color: Color,
    owner: Option<NodeId>,
    /// An explicit owner requested disposal while one of its descendants was
    /// running or updating. The runtime drains it at the next safe boundary.
    dispose_requested: bool,
    queued: bool,
    updating: bool,
    running: bool,
    /// A failed memo must force its already-colored downstream chain to be
    /// revisited on the next invalidation.
    retry_notify: bool,
    /// Current value (`None` for effects and never-run memos).
    value: Option<Value>,
    /// How to (re)compute (memos and effects; `None` for signals).
    compute: Option<Compute>,
    /// Equality gate (memos). `None` → always treated as changed.
    eq: Option<EqFn>,
    /// Nodes this node read during its last run.
    sources: Vec<NodeId>,
    /// Nodes that read this node.
    subscribers: Vec<NodeId>,
    /// Nodes created while this node's computation ran (disposed on re-run).
    owned: Vec<NodeId>,
    /// Cleanup callbacks. On a successful re-run, the previous scope is retired
    /// after the replacement frame commits; on disposal they run immediately.
    cleanups: Vec<Cleanup>,
}

struct Slot {
    generation: u32,
    node: Option<Node>,
}

struct ComputationFrame {
    observer: NodeId,
    sources: Vec<NodeId>,
    owned: Vec<NodeId>,
    cleanups: Vec<Cleanup>,
}

#[derive(Default)]
struct Runtime {
    slots: Vec<Slot>,
    free: Vec<u32>,
    /// The computation currently running (tracking target), if any.
    observer: Option<NodeId>,
    /// The computation that owns newly-created nodes and cleanups. Unlike the
    /// observer, this remains active inside `untrack`.
    owner: Option<NodeId>,
    /// Transactional dependency/ownership frames for nested computations.
    frames: Vec<ComputationFrame>,
    /// Effects queued by the current write, run at the end of it.
    pending: VecDeque<NodeId>,
    /// Re-entrancy guard for effect flushing.
    flushing: bool,
    /// User callbacks may nest writes. Effects flush when the outer batch ends.
    batch_depth: usize,
    /// Owner roots waiting for their active descendant to reach a safe point.
    deferred_disposals: VecDeque<NodeId>,
}

thread_local! {
    static RT: RefCell<Runtime> = RefCell::new(Runtime::default());
}

impl Runtime {
    fn node(&self, id: NodeId) -> Option<&Node> {
        let slot = self.slots.get(id.slot as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_ref()
    }

    fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_mut()
    }

    fn allocate_node(&mut self, node: Node) -> NodeId {
        if let Some(slot_index) = self.free.pop() {
            let slot = &mut self.slots[slot_index as usize];
            debug_assert!(slot.node.is_none());
            slot.node = Some(node);
            NodeId {
                slot: slot_index,
                generation: slot.generation,
            }
        } else {
            let slot = u32::try_from(self.slots.len()).expect("reactive arena exhausted u32 slots");
            self.slots.push(Slot {
                generation: 0,
                node: Some(node),
            });
            NodeId {
                slot,
                generation: 0,
            }
        }
    }

    fn add_node(&mut self, mut node: Node) -> NodeId {
        let owner = self.owner.filter(|owner| self.node(*owner).is_some());
        node.owner = owner;
        let id = self.allocate_node(node);

        if let Some(owner) = owner {
            let frame = self
                .frames
                .last_mut()
                .expect("active observer must have a computation frame");
            assert_eq!(frame.observer, owner, "observer/frame mismatch");
            frame.owned.push(id);
        }
        id
    }

    fn add_root_node(&mut self, mut node: Node) -> NodeId {
        node.owner = None;
        self.allocate_node(node)
    }

    fn add_owned_node(&mut self, mut node: Node, owner: NodeId) -> NodeId {
        let owner_node = self
            .node(owner)
            .expect("explicit owner disappeared before resource allocation");
        assert_eq!(owner_node.kind, Kind::Owner, "explicit owner kind changed");
        assert!(
            !owner_node.dispose_requested,
            "cannot add a resource to an owner pending disposal"
        );
        node.owner = Some(owner);
        let id = self.allocate_node(node);
        self.node_mut(owner)
            .expect("explicit owner disappeared during resource allocation")
            .owned
            .push(id);
        id
    }

    fn defer_owner_disposal(&mut self, id: NodeId) {
        let should_queue = match self.node_mut(id) {
            Some(node) => {
                assert_eq!(node.kind, Kind::Owner, "only owners may defer disposal");
                if node.dispose_requested {
                    false
                } else {
                    node.dispose_requested = true;
                    true
                }
            }
            None => false,
        };
        if should_queue {
            self.deferred_disposals.push_back(id);
        }
    }

    fn take_node(&mut self, id: NodeId) -> Option<Node> {
        let slot = self.slots.get_mut(id.slot as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let node = slot.node.take()?;
        if let Some(next_generation) = slot.generation.checked_add(1) {
            slot.generation = next_generation;
            self.free.push(id.slot);
        }
        Some(node)
    }

    fn enqueue_effect(&mut self, id: NodeId) {
        let should_enqueue = match self.node_mut(id) {
            Some(node) if node.kind == Kind::Effect && !node.queued => {
                node.queued = true;
                true
            }
            _ => false,
        };
        if should_enqueue {
            self.pending.push_back(id);
        }
    }

    fn pop_pending(&mut self) -> Option<NodeId> {
        while let Some(id) = self.pending.pop_front() {
            if let Some(node) = self.node_mut(id) {
                node.queued = false;
                return Some(id);
            }
        }
        None
    }

    fn clear_pending(&mut self) {
        let mut discarded = Vec::new();
        while let Some(id) = self.pending.pop_front() {
            if let Some(node) = self.node_mut(id) {
                node.queued = false;
                discarded.push(id);
            }
        }
        for effect in discarded {
            self.rearm_discarded_effect(effect);
        }
    }

    fn abort_pending(&mut self, current: NodeId) {
        self.rearm_discarded_effect(current);
        self.clear_pending();
    }

    fn rearm_discarded_effect(&mut self, effect: NodeId) {
        let mut sources = self
            .node(effect)
            .map(|node| node.sources.clone())
            .unwrap_or_default();
        let mut visited = HashSet::new();
        while let Some(source) = sources.pop() {
            if !visited.insert(source) {
                continue;
            }
            let Some(node) = self.node_mut(source) else {
                continue;
            };
            if node.kind == Kind::Memo && node.color == Color::Dirty {
                node.retry_notify = true;
            }
            sources.extend(node.sources.iter().copied());
        }
    }
}

#[derive(Default)]
struct DisposalBatch {
    cleanups: Vec<Cleanup>,
    retired: Vec<Node>,
}

fn collect_disposal(rt: &mut Runtime, id: NodeId, batch: &mut DisposalBatch) {
    enum Task {
        Visit(NodeId),
        Finish { node: Node, cleanups: Vec<Cleanup> },
    }

    let mut tasks = vec![Task::Visit(id)];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(id) => {
                let Some(mut node) = rt.take_node(id) else {
                    continue;
                };

                rt.pending.retain(|pending| *pending != id);
                for frame in &mut rt.frames {
                    frame.owned.retain(|child| *child != id);
                }
                if let Some(owner) = node.owner {
                    if let Some(owner_node) = rt.node_mut(owner) {
                        owner_node.owned.retain(|child| *child != id);
                    }
                }

                for source in std::mem::take(&mut node.sources) {
                    if let Some(source_node) = rt.node_mut(source) {
                        source_node
                            .subscribers
                            .retain(|subscriber| *subscriber != id);
                    }
                }
                for subscriber in std::mem::take(&mut node.subscribers) {
                    if let Some(subscriber_node) = rt.node_mut(subscriber) {
                        subscriber_node.sources.retain(|source| *source != id);
                    }
                }

                let children = std::mem::take(&mut node.owned);
                let cleanups = std::mem::take(&mut node.cleanups);
                node.owner = None;
                node.queued = false;
                node.updating = false;
                node.running = false;
                tasks.push(Task::Finish { node, cleanups });
                // Pushing in creation order makes the newest child the next
                // LIFO task, while Finish keeps descendants before the parent.
                for child in children {
                    tasks.push(Task::Visit(child));
                }
            }
            Task::Finish { node, cleanups } => {
                batch.cleanups.extend(cleanups.into_iter().rev());
                batch.retired.push(node);
            }
        }
    }
}

fn disposable_tree_is_busy(rt: &Runtime, id: NodeId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        let Some(node) = rt.node(id) else {
            continue;
        };
        if node.updating || node.running {
            return true;
        }
        pending.extend(node.owned.iter().copied());
    }
    false
}

fn assert_disposable_tree(rt: &Runtime, id: NodeId) {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        let Some(node) = rt.node(id) else {
            continue;
        };
        assert!(!node.updating, "cannot dispose a signal during its update");
        assert!(!node.running, "cannot dispose a running computation");
        pending.extend(node.owned.iter().copied());
    }
}

struct ObserverGuard {
    previous: Option<NodeId>,
}

impl ObserverGuard {
    fn replace(observer: Option<NodeId>) -> Self {
        let previous = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            std::mem::replace(&mut rt.observer, observer)
        });
        Self { previous }
    }
}

impl Drop for ObserverGuard {
    fn drop(&mut self) {
        RT.with(|rt| rt.borrow_mut().observer = self.previous);
    }
}

fn run_disposal(batch: DisposalBatch) -> Option<PanicPayload> {
    let _untracked = ObserverGuard::replace(None);
    let mut first_panic = None;

    for cleanup in batch.cleanups {
        record_panic(catch_unwind(AssertUnwindSafe(cleanup)), &mut first_panic);
    }
    for node in batch.retired {
        let Node {
            value,
            compute,
            eq,
            sources,
            subscribers,
            owned,
            cleanups,
            ..
        } = node;
        debug_assert!(sources.is_empty());
        debug_assert!(subscribers.is_empty());
        debug_assert!(owned.is_empty());
        drop(sources);
        drop(subscribers);
        drop(owned);
        drop_catching(value, &mut first_panic);
        drop_catching(compute, &mut first_panic);
        drop_catching(eq, &mut first_panic);
        for cleanup in cleanups {
            drop_catching(cleanup, &mut first_panic);
        }
    }

    first_panic
}

fn take_ready_deferred_disposal(rt: &mut Runtime) -> Option<DisposalBatch> {
    let queued = rt.deferred_disposals.len();
    for _ in 0..queued {
        let id = rt
            .deferred_disposals
            .pop_front()
            .expect("deferred disposal queue length changed");
        let Some(node) = rt.node(id) else {
            continue;
        };
        assert_eq!(node.kind, Kind::Owner, "deferred node is not an owner");
        if !node.dispose_requested {
            continue;
        }
        if disposable_tree_is_busy(rt, id) {
            rt.deferred_disposals.push_back(id);
            continue;
        }

        rt.node_mut(id)
            .expect("deferred owner disappeared before collection")
            .dispose_requested = false;
        let mut batch = DisposalBatch::default();
        collect_disposal(rt, id, &mut batch);
        return Some(batch);
    }
    None
}

fn merge_panic(first: &mut Option<PanicPayload>, payload: Option<PanicPayload>) {
    if let Some(payload) = payload {
        if first.is_none() {
            *first = Some(payload);
        } else {
            suppress_panic(payload);
        }
    }
}

fn drain_deferred_disposals() -> Option<PanicPayload> {
    let mut first_panic = None;
    loop {
        let batch = RT.with(|rt| take_ready_deferred_disposal(&mut rt.borrow_mut()));
        let Some(batch) = batch else { break };
        merge_panic(&mut first_panic, run_disposal(batch));
    }
    first_panic
}

struct BatchGuard {
    active: bool,
}

impl BatchGuard {
    fn enter() -> Self {
        RT.with(|rt| rt.borrow_mut().batch_depth += 1);
        Self { active: true }
    }

    fn finish(mut self) -> bool {
        let should_flush = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            rt.batch_depth = rt
                .batch_depth
                .checked_sub(1)
                .expect("reactive batch depth underflow");
            rt.batch_depth == 0
        });
        self.active = false;
        should_flush
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        if self.active {
            RT.with(|rt| {
                let mut rt = rt.borrow_mut();
                rt.batch_depth = rt
                    .batch_depth
                    .checked_sub(1)
                    .expect("reactive batch depth underflow");
            });
        }
    }
}

struct ComputationGuard {
    id: NodeId,
    previous_observer: Option<NodeId>,
    previous_owner: Option<NodeId>,
    active: bool,
}

impl ComputationGuard {
    fn enter(id: NodeId) -> Self {
        let (previous_observer, previous_owner) = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            let node = rt.node_mut(id).expect("stale computation handle");
            assert!(!node.running, "reactive computation re-entered itself");
            node.running = true;
            let previous_observer = rt.observer;
            let previous_owner = rt.owner;
            rt.observer = Some(id);
            rt.owner = Some(id);
            rt.frames.push(ComputationFrame {
                observer: id,
                sources: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            });
            (previous_observer, previous_owner)
        });
        Self {
            id,
            previous_observer,
            previous_owner,
            active: true,
        }
    }

    fn finish(mut self) -> ComputationFrame {
        let frame = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            let frame = rt.frames.pop().expect("missing computation frame");
            assert_eq!(frame.observer, self.id, "computation frame order changed");
            rt.observer = self.previous_observer;
            rt.owner = self.previous_owner;
            rt.node_mut(self.id)
                .expect("computation disposed while running")
                .running = false;
            frame
        });
        self.active = false;
        frame
    }

    fn take_rollback(&mut self) -> DisposalBatch {
        RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            let frame = rt.frames.pop().expect("missing computation frame");
            assert_eq!(frame.observer, self.id, "computation frame order changed");
            rt.observer = self.previous_observer;
            rt.owner = self.previous_owner;
            if let Some(node) = rt.node_mut(self.id) {
                node.running = false;
            }
            let mut batch = DisposalBatch::default();
            for child in frame.owned.into_iter().rev() {
                collect_disposal(&mut rt, child, &mut batch);
            }
            batch.cleanups.extend(frame.cleanups.into_iter().rev());
            batch
        })
    }

    fn rollback(mut self) -> Option<PanicPayload> {
        let batch = self.take_rollback();
        self.active = false;
        let mut cleanup_panic = run_disposal(batch);
        merge_panic(&mut cleanup_panic, drain_deferred_disposals());
        cleanup_panic
    }
}

impl Drop for ComputationGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let batch = self.take_rollback();
        self.active = false;
        let already_panicking = std::thread::panicking();
        let mut cleanup_panic = run_disposal(batch);
        merge_panic(&mut cleanup_panic, drain_deferred_disposals());
        if let Some(payload) = cleanup_panic {
            if !already_panicking {
                resume_unwind(payload);
            } else {
                suppress_panic(payload);
            }
        }
    }
}

struct FlushGuard;

impl FlushGuard {
    fn enter() -> Option<Self> {
        RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            if rt.flushing {
                None
            } else {
                rt.flushing = true;
                Some(Self)
            }
        })
    }
}

impl Drop for FlushGuard {
    fn drop(&mut self) {
        RT.with(|rt| rt.borrow_mut().flushing = false);
    }
}

fn assert_signal_write_allowed(rt: &Runtime, id: NodeId) {
    for frame in &rt.frames {
        let mut pending = frame.sources.clone();
        pending.extend_from_slice(
            rt.node(frame.observer)
                .map(|node| node.sources.as_slice())
                .unwrap_or_default(),
        );
        let mut visited = HashSet::new();
        while let Some(source) = pending.pop() {
            if source == id {
                panic!("a computation cannot write to one of its reactive sources");
            }
            if !visited.insert(source) {
                continue;
            }
            if let Some(node) = rt.node(source) {
                pending.extend(node.sources.iter().copied());
            }
        }
    }
}

fn suppress_panic(payload: PanicPayload) {
    // A panic payload may itself panic from Drop. When preserving an earlier
    // panic, leaking this exceptional value is the only double-panic-safe path.
    std::mem::forget(payload);
}

fn record_panic(result: Result<(), PanicPayload>, first_panic: &mut Option<PanicPayload>) {
    if let Err(payload) = result {
        if first_panic.is_none() {
            *first_panic = Some(payload);
        } else {
            suppress_panic(payload);
        }
    }
}

fn drop_catching<T>(value: T, first_panic: &mut Option<PanicPayload>) {
    record_panic(catch_unwind(AssertUnwindSafe(|| drop(value))), first_panic);
}

fn flush_while_preserving(primary: PanicPayload, should_flush: bool) -> ! {
    if should_flush {
        if let Err(secondary) = catch_unwind(AssertUnwindSafe(flush_effects)) {
            suppress_panic(secondary);
        }
    }
    resume_unwind(primary)
}

struct UpdateGuard {
    id: NodeId,
    active: bool,
}

impl UpdateGuard {
    fn enter<T: 'static>(id: NodeId) -> (Self, Value) {
        let value = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            assert_signal_write_allowed(&rt, id);
            let node = rt.node_mut(id).expect("stale or disposed signal handle");
            assert!(!node.updating, "reentrant write to the same signal");
            let value = node.value.clone().expect("signal value");
            assert!(value.is::<T>(), "signal type mismatch");
            node.updating = true;
            value
        });
        (Self { id, active: true }, value)
    }

    fn finish(mut self) {
        RT.with(|rt| {
            rt.borrow_mut()
                .node_mut(self.id)
                .expect("signal disposed during update")
                .updating = false;
        });
        self.active = false;
    }
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        if self.active {
            RT.with(|rt| {
                if let Some(node) = rt.borrow_mut().node_mut(self.id) {
                    node.updating = false;
                }
            });
            let already_panicking = std::thread::panicking();
            if let Some(payload) = drain_deferred_disposals() {
                if already_panicking {
                    suppress_panic(payload);
                } else {
                    resume_unwind(payload);
                }
            }
        }
    }
}

/// Record a source in the active provisional computation frame.
fn track(source: NodeId) {
    RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let Some(obs) = rt.observer else { return };
        if obs == source || rt.node(source).is_none() {
            return;
        }
        let frame = rt
            .frames
            .last_mut()
            .expect("active observer must have a computation frame");
        assert_eq!(frame.observer, obs, "observer/frame mismatch");
        if !frame.sources.contains(&source) {
            frame.sources.push(source);
        }
    });
}

/// Phase 1 (push, cheap): direct subscribers of a changed node become Dirty,
/// deeper descendants become (at least) Check. Effects encountered are queued.
fn mark_subscribers(rt: &mut Runtime, of: NodeId, color: Color) {
    mark_subscribers_with_force(rt, of, color, false);
}

fn mark_subscribers_with_force(rt: &mut Runtime, of: NodeId, color: Color, force: bool) {
    let Some(subs) = rt.node(of).map(|node| node.subscribers.clone()) else {
        return;
    };
    let mut pending = Vec::with_capacity(subs.len());
    let mut visited = HashSet::new();
    for subscriber in subs.into_iter().rev() {
        pending.push((subscriber, color, force));
    }
    while let Some((s, color, force)) = pending.pop() {
        if !visited.insert((s, color, force)) {
            continue;
        }
        let Some((kind, should_descend, force_descend)) = rt.node_mut(s).map(|node| {
            let should_descend = node.color < color;
            if should_descend {
                node.color = color;
            }
            let force_descend = force || node.retry_notify;
            node.retry_notify = false;
            (node.kind, should_descend, force_descend)
        }) else {
            continue;
        };
        if kind == Kind::Effect {
            rt.enqueue_effect(s);
        } else if should_descend || force_descend {
            let children = rt
                .node(s)
                .map(|node| node.subscribers.clone())
                .unwrap_or_default();
            for child in children.into_iter().rev() {
                pending.push((child, Color::Check, force_descend));
            }
        }
    }
}

/// Phase 2 (pull, lazy): make `id` current, recomputing only what truly changed.
fn update_if_necessary(id: NodeId) {
    struct PullFrame {
        id: NodeId,
        sources: Vec<NodeId>,
        next_source: usize,
    }

    fn frame(id: NodeId) -> Option<PullFrame> {
        RT.with(|rt| {
            rt.borrow().node(id).map(|node| PullFrame {
                id,
                sources: node.sources.clone(),
                next_source: 0,
            })
        })
    }

    let Some(root) = frame(id) else { return };
    let mut stack = vec![root];
    let mut active = HashSet::from([id]);
    while let Some(current) = stack.last_mut() {
        let Some((color, kind)) = RT.with(|rt| {
            rt.borrow()
                .node(current.id)
                .map(|node| (node.color, node.kind))
        }) else {
            active.remove(&current.id);
            stack.pop();
            continue;
        };

        if color == Color::Clean || kind == Kind::Signal {
            active.remove(&current.id);
            stack.pop();
            continue;
        }
        if color == Color::Dirty {
            let id = current.id;
            active.remove(&id);
            stack.pop();
            run_computation(id);
            continue;
        }

        if let Some(source) = current.sources.get(current.next_source).copied() {
            current.next_source += 1;
            if let Some(source_frame) = frame(source) {
                assert!(
                    active.insert(source),
                    "cycle detected in reactive dependencies"
                );
                stack.push(source_frame);
            }
            continue;
        }

        let id = current.id;
        active.remove(&id);
        stack.pop();
        RT.with(|rt| {
            if let Some(node) = rt.borrow_mut().node_mut(id) {
                node.color = Color::Clean;
            }
        });
    }
}

fn commit_computation(
    id: NodeId,
    frame: ComputationFrame,
    new_value: Value,
    changed: bool,
) -> DisposalBatch {
    RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let (kind, was_retry, old_sources, old_owned, old_cleanups) = {
            let node = rt.node_mut(id).expect("stale computation handle");
            (
                node.kind,
                node.retry_notify,
                std::mem::take(&mut node.sources),
                std::mem::take(&mut node.owned),
                std::mem::take(&mut node.cleanups),
            )
        };

        for source in old_sources {
            if let Some(source_node) = rt.node_mut(source) {
                source_node
                    .subscribers
                    .retain(|subscriber| *subscriber != id);
            }
        }

        let mut new_sources = Vec::with_capacity(frame.sources.len());
        for source in frame.sources {
            if let Some(source_node) = rt.node_mut(source) {
                if !source_node.subscribers.contains(&id) {
                    source_node.subscribers.push(id);
                }
                new_sources.push(source);
            }
        }

        {
            let node = rt.node_mut(id).expect("stale computation handle");
            node.sources = new_sources;
            node.owned = frame.owned;
            node.cleanups = frame.cleanups;
            node.retry_notify = false;
            if kind == Kind::Memo {
                node.value = Some(new_value);
            }
            node.color = Color::Clean;
        }

        if kind == Kind::Memo && (changed || was_retry) {
            let color = if changed { Color::Dirty } else { Color::Check };
            mark_subscribers_with_force(&mut rt, id, color, was_retry);
        }

        let mut batch = DisposalBatch::default();
        for child in old_owned.into_iter().rev() {
            collect_disposal(&mut rt, child, &mut batch);
        }
        batch.cleanups.extend(old_cleanups.into_iter().rev());
        batch
    })
}

/// Re-run a memo/effect in a provisional frame and publish only on success.
fn run_computation(id: NodeId) {
    let batch_guard = BatchGuard::enter();
    let (kind, compute, prev_value, eq) = RT.with(|rt| {
        let rt = rt.borrow();
        let node = rt.node(id).expect("stale computation handle");
        (
            node.kind,
            node.compute.clone().expect("computation node"),
            node.value.clone(),
            node.eq.clone(),
        )
    });

    let computation_guard = ComputationGuard::enter(id);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let new_value = compute();
        let changed = match (&prev_value, &eq) {
            (Some(prev), Some(eq)) => untrack(|| !eq(prev.as_ref(), new_value.as_ref())),
            _ => true, // first run, or no comparator
        };
        (new_value, changed)
    }));
    // Keep user-owned captures anchored in the live node while deferred owner
    // disposal is drained, so their final Drop is handled by `run_disposal`.
    drop(compute);
    drop(prev_value);
    drop(eq);
    let (new_value, changed) = match outcome {
        Ok(result) => result,
        Err(primary) => {
            if let Some(cleanup_panic) = computation_guard.rollback() {
                suppress_panic(cleanup_panic);
            }
            if kind == Kind::Memo {
                RT.with(|rt| {
                    if let Some(node) = rt.borrow_mut().node_mut(id) {
                        node.retry_notify = true;
                    }
                });
            }
            let should_flush = batch_guard.finish();
            flush_while_preserving(primary, should_flush);
        }
    };
    let frame = computation_guard.finish();
    let retired = commit_computation(id, frame, new_value, changed);
    let mut cleanup_panic = run_disposal(retired);
    merge_panic(&mut cleanup_panic, drain_deferred_disposals());
    let should_flush = batch_guard.finish();
    if let Some(payload) = cleanup_panic {
        flush_while_preserving(payload, should_flush);
    }
    if should_flush {
        flush_effects();
    }
}

/// Run queued effects until quiescent. Called at the end of every write.
fn flush_effects() {
    let Some(flush_guard) = FlushGuard::enter() else {
        return;
    };
    let mut first_panic = None;
    let max_executions = RT.with(|rt| {
        let live_nodes = rt
            .borrow()
            .slots
            .iter()
            .filter(|slot| slot.node.is_some())
            .count();
        MIN_EFFECT_RUNS_PER_FLUSH
            .max(live_nodes.saturating_mul(EFFECT_RUNS_PER_LIVE_NODE))
            .min(MAX_EFFECT_RUNS_PER_FLUSH)
    });
    let mut executions = 0usize;
    while let Some(next) = RT.with(|rt| rt.borrow_mut().pop_pending()) {
        executions += 1;
        if executions > max_executions {
            let payload = diagnostic_panic_payload(format!(
                "reactive flush exceeded its {max_executions}-effect safety budget"
            ));
            if first_panic.is_none() {
                first_panic = Some(payload);
            } else {
                suppress_panic(payload);
            }
            RT.with(|rt| rt.borrow_mut().abort_pending(next));
            break;
        }
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| update_if_necessary(next))) {
            if first_panic.is_none() {
                first_panic = Some(payload);
            } else {
                suppress_panic(payload);
            }
        }
    }
    drop(flush_guard);
    if let Some(payload) = first_panic {
        resume_unwind(payload);
    }
}

fn diagnostic_panic_payload(message: String) -> PanicPayload {
    match catch_unwind(AssertUnwindSafe(|| std::panic::panic_any(message))) {
        Err(payload) => payload,
        Ok(()) => unreachable!("panic_any returned normally"),
    }
}

// ───────────────────────────── public API ─────────────────────────────

/// An explicit lifecycle scope for reactive resources.
///
/// Owners never inherit the currently-running computation. A child owner or an
/// owned effect is attached only through this handle, which makes mount
/// lifetimes deterministic even when setup runs inside another computation.
/// Dropping an owner recursively disposes its children and executes cleanups in
/// LIFO order. Calling [`Owner::dispose`] first is safe; the later `Drop` is a
/// no-op.
///
/// Do not strongly capture an owner (for example through `Rc<Owner>`) inside a
/// resource owned by that same owner: that creates an application-level
/// retention cycle. Capture only the signals a callback needs, or use a weak,
/// non-owning application handle.
///
/// Owners are intentionally local to one browser thread:
///
/// ```compile_fail
/// use pliego_reactive::Owner;
/// fn require_send<T: Send>(_: T) {}
/// require_send(Owner::new());
/// ```
pub struct Owner {
    id: NodeId,
    _local: PhantomData<Rc<()>>,
}

impl Owner {
    /// Create an independent root scope.
    pub fn new() -> Self {
        let id = RT.with(|rt| {
            rt.borrow_mut().add_root_node(Node {
                kind: Kind::Owner,
                color: Color::Clean,
                owner: None,
                dispose_requested: false,
                queued: false,
                updating: false,
                running: false,
                retry_notify: false,
                value: None,
                compute: None,
                eq: None,
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            })
        });
        Self {
            id,
            _local: PhantomData,
        }
    }

    /// Create a nested lifecycle scope owned by `self`.
    pub fn child(&self) -> Result<Self, OwnerError> {
        self.ensure_alive()?;
        let id = RT.with(|rt| {
            rt.borrow_mut().add_owned_node(
                Node {
                    kind: Kind::Owner,
                    color: Color::Clean,
                    owner: None,
                    dispose_requested: false,
                    queued: false,
                    updating: false,
                    running: false,
                    retry_notify: false,
                    value: None,
                    compute: None,
                    eq: None,
                    sources: Vec::new(),
                    subscribers: Vec::new(),
                    owned: Vec::new(),
                    cleanups: Vec::new(),
                },
                self.id,
            )
        });
        Ok(Self {
            id,
            _local: PhantomData,
        })
    }

    /// Register a cleanup directly on this scope, independent of ambient state.
    pub fn on_cleanup(&self, f: impl FnOnce() + 'static) -> Result<(), OwnerError> {
        self.ensure_alive()?;
        RT.with(|rt| {
            rt.borrow_mut()
                .node_mut(self.id)
                .expect("explicit owner disappeared before cleanup registration")
                .cleanups
                .push(Box::new(f));
        });
        Ok(())
    }

    /// Create a signal whose lifetime is immediately bound to this scope.
    pub fn signal<T: 'static>(&self, value: T) -> Result<Signal<T>, OwnerError> {
        self.ensure_alive()?;
        Ok(Signal::new_in_owner(value, self.id))
    }

    /// Create a memo whose lifetime is immediately bound to this scope.
    pub fn memo<T: PartialEq + 'static>(
        &self,
        f: impl Fn() -> T + 'static,
    ) -> Result<Memo<T>, OwnerError> {
        self.ensure_alive()?;
        Ok(Memo::new_in_owner(f, self.id))
    }

    /// Create an effect whose lifetime is immediately bound to this scope.
    pub fn effect(&self, f: impl Fn() + 'static) -> Result<Effect, OwnerError> {
        self.ensure_alive()?;
        Ok(Effect::new_in_owner(f, self.id))
    }

    /// Adopt an existing, live, unowned effect into this scope.
    ///
    /// Effects already owned by another computation or scope are rejected;
    /// ownership is never silently stolen or duplicated.
    pub fn adopt_effect(&self, effect: &Effect) -> Result<(), OwnerError> {
        RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            let Some(owner) = rt.node(self.id) else {
                return Err(OwnerError::Disposed);
            };
            if owner.kind != Kind::Owner {
                return Err(OwnerError::Disposed);
            }
            if owner.dispose_requested {
                return Err(OwnerError::Disposed);
            }

            let Some(resource) = rt.node(effect.id) else {
                return Err(OwnerError::ResourceDisposed);
            };
            debug_assert_eq!(resource.kind, Kind::Effect);
            if resource.running {
                return Err(OwnerError::ResourceBusy);
            }
            match resource.owner {
                Some(current) if current == self.id => return Ok(()),
                Some(_) => return Err(OwnerError::AlreadyOwned),
                None => {}
            }

            rt.node_mut(effect.id)
                .expect("effect disappeared during adoption")
                .owner = Some(self.id);
            rt.node_mut(self.id)
                .expect("owner disappeared during adoption")
                .owned
                .push(effect.id);
            Ok(())
        })
    }

    /// Recursively dispose this scope. Repeated calls are no-ops.
    ///
    /// If a descendant is currently running or updating, the owner becomes
    /// immediately unusable and teardown is deferred to that operation's safe
    /// boundary. Cleanup panics are resumed from that boundary after every
    /// sibling cleanup has run.
    pub fn dispose(&self) {
        dispose_owner(self.id);
    }

    /// Whether this handle no longer resolves to its original scope.
    pub fn is_disposed(&self) -> bool {
        self.ensure_alive().is_err()
    }

    fn ensure_alive(&self) -> Result<(), OwnerError> {
        RT.with(|rt| match rt.borrow().node(self.id) {
            Some(node) if node.kind == Kind::Owner && !node.dispose_requested => Ok(()),
            _ => Err(OwnerError::Disposed),
        })
    }
}

impl Default for Owner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if RT.try_with(|_| ()).is_err() {
            return;
        }
        let already_panicking = std::thread::panicking();
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| dispose_owner(self.id))) {
            if already_panicking {
                suppress_panic(payload);
            } else {
                resume_unwind(payload);
            }
        }
    }
}

/// A reactive, writable value — a root of the graph.
///
/// In PliegoRS discipline there is ultimately ONE meaningful root (the log);
/// `Signal` exists as the general primitive the fold node builds on (M3).
///
/// Handles are intentionally local to the thread that owns the thread-local
/// runtime:
///
/// ```compile_fail
/// use pliego_reactive::Signal;
/// fn require_send<T: Send>(_: T) {}
/// require_send(Signal::new(1));
/// ```
pub struct Signal<T> {
    id: NodeId,
    _t: PhantomData<fn() -> T>,
    _local: PhantomData<Rc<()>>,
}
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Signal<T> {}

impl<T: 'static> Signal<T> {
    /// A new signal holding `value`.
    pub fn new(value: T) -> Self {
        Self::install(value, None)
    }

    fn new_in_owner(value: T, owner: NodeId) -> Self {
        Self::install(value, Some(owner))
    }

    fn install(value: T, explicit_owner: Option<NodeId>) -> Self {
        let id = RT.with(|rt| {
            let node = Node {
                kind: Kind::Signal,
                color: Color::Clean,
                owner: None,
                dispose_requested: false,
                queued: false,
                updating: false,
                running: false,
                retry_notify: false,
                value: Some(Rc::new(value)),
                compute: None,
                eq: None,
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            };
            let mut rt = rt.borrow_mut();
            match explicit_owner {
                Some(owner) => rt.add_owned_node(node, owner),
                None => rt.add_node(node),
            }
        });
        Signal {
            id,
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// Read (tracked): subscribes the running computation, if any.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        track(self.id);
        self.get_untracked()
    }

    /// Read without subscribing.
    pub fn get_untracked(&self) -> T
    where
        T: Clone,
    {
        let value: Value = RT.with(|rt| {
            let rt = rt.borrow();
            rt.node(self.id)
                .expect("stale or disposed signal handle")
                .value
                .as_ref()
                .expect("signal value")
                .clone()
        });
        value.downcast_ref::<T>().expect("signal type").clone()
    }

    /// Read by reference (tracked), without cloning `T` — the path large values
    /// (like the log) take. The value's `Rc` is cloned cheaply and the runtime
    /// borrow released before `f` runs, so `f` may freely read other reactives.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(self.id);
        let value: Value = RT.with(|rt| {
            rt.borrow()
                .node(self.id)
                .expect("stale or disposed signal handle")
                .value
                .clone()
                .expect("signal value")
        });
        f(value.downcast_ref::<T>().expect("signal type"))
    }

    /// Read if alive (`None` after disposal) — the `try_` discipline from the spec.
    pub fn try_get(&self) -> Option<T>
    where
        T: Clone,
    {
        let value = RT.with(|rt| {
            let rt = rt.borrow();
            let node = rt.node(self.id)?;
            node.value.clone()
        })?;
        value.downcast_ref::<T>().cloned()
    }

    /// Replace the value and notify: subscribers marked, effects flushed.
    pub fn set(&self, value: T) {
        let batch_guard = BatchGuard::enter();
        let replacement: Value = Rc::new(value);
        let previous = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            assert_signal_write_allowed(&rt, self.id);
            let node = rt
                .node_mut(self.id)
                .expect("stale or disposed signal handle");
            assert!(!node.updating, "reentrant write to the same signal");
            let previous = node.value.replace(replacement).expect("signal value");
            mark_subscribers(&mut rt, self.id, Color::Dirty);
            previous
        });
        let drop_result = catch_unwind(AssertUnwindSafe(|| drop(previous)));
        let should_flush = batch_guard.finish();
        if let Err(primary) = drop_result {
            flush_while_preserving(primary, should_flush);
        }
        if should_flush {
            flush_effects();
        }
    }

    /// Mutate a cloned candidate and publish it atomically on success.
    ///
    /// `T: Clone` is required because safe Rust cannot recover the pre-callback
    /// state of an arbitrary `&mut T` after a panic. The stable value remains in
    /// the runtime until `f` returns normally, and callbacks run without a
    /// runtime borrow. Nested writes to other signals are batched; a write to
    /// this same signal is rejected deterministically.
    pub fn update(&self, f: impl FnOnce(&mut T))
    where
        T: Clone,
    {
        let batch_guard = BatchGuard::enter();
        let (update_guard, stable) = UpdateGuard::enter::<T>(self.id);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut candidate = stable.downcast_ref::<T>().expect("signal type").clone();
            f(&mut candidate);
            candidate
        }));
        let candidate = match outcome {
            Ok(candidate) => candidate,
            Err(primary) => {
                update_guard.finish();
                if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(stable))) {
                    suppress_panic(secondary);
                }
                if let Some(secondary) = drain_deferred_disposals() {
                    suppress_panic(secondary);
                }
                let should_flush = batch_guard.finish();
                flush_while_preserving(primary, should_flush);
            }
        };

        let previous = RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            let previous = rt
                .node_mut(self.id)
                .expect("signal disposed during update")
                .value
                .replace(Rc::new(candidate))
                .expect("signal value");
            mark_subscribers(&mut rt, self.id, Color::Dirty);
            previous
        });
        update_guard.finish();
        let drop_result = catch_unwind(AssertUnwindSafe(|| {
            drop(previous);
            drop(stable);
        }));
        let mut post_update_panic = drop_result.err();
        merge_panic(&mut post_update_panic, drain_deferred_disposals());
        let should_flush = batch_guard.finish();
        if let Some(primary) = post_update_panic {
            flush_while_preserving(primary, should_flush);
        }
        if should_flush {
            flush_effects();
        }
    }

    /// Dispose this signal (edges removed; reads via `try_get` return `None`).
    pub fn dispose(&self) {
        dispose(self.id);
    }

    /// The raw node id (the fold node in M3 composes on this).
    pub fn id(&self) -> NodeId {
        self.id
    }
}

/// A derived, cached, equality-gated value — the materialized-fold primitive.
pub struct Memo<T> {
    id: NodeId,
    _t: PhantomData<fn() -> T>,
    _local: PhantomData<Rc<()>>,
}
impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Memo<T> {}

impl<T: PartialEq + 'static> Memo<T> {
    /// A memo computing `f`. Lazy: `f` first runs on first read.
    pub fn new(f: impl Fn() -> T + 'static) -> Self {
        Self::install(f, None)
    }

    fn new_in_owner(f: impl Fn() -> T + 'static, owner: NodeId) -> Self {
        Self::install(f, Some(owner))
    }

    fn install(f: impl Fn() -> T + 'static, explicit_owner: Option<NodeId>) -> Self {
        let compute: Compute = Rc::new(move || Rc::new(f()) as Value);
        let eq: EqFn = Rc::new(
            |a, b| match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        );
        let id = RT.with(|rt| {
            let node = Node {
                kind: Kind::Memo,
                color: Color::Dirty, // never ran
                owner: None,
                dispose_requested: false,
                queued: false,
                updating: false,
                running: false,
                retry_notify: false,
                value: None,
                compute: Some(compute),
                eq: Some(eq),
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            };
            let mut rt = rt.borrow_mut();
            match explicit_owner {
                Some(owner) => rt.add_owned_node(node, owner),
                None => rt.add_node(node),
            }
        });
        Memo {
            id,
            _t: PhantomData,
            _local: PhantomData,
        }
    }

    /// Read (tracked). Settles the node first (lazy pull).
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        update_if_necessary(self.id);
        track(self.id);
        let value: Value = RT.with(|rt| {
            let rt = rt.borrow();
            rt.node(self.id)
                .expect("stale or disposed memo handle")
                .value
                .as_ref()
                .expect("memo ran")
                .clone()
        });
        value.downcast_ref::<T>().expect("memo type").clone()
    }

    /// Read if alive. A disposed owner makes all memo handles stale.
    pub fn try_get(&self) -> Option<T>
    where
        T: Clone,
    {
        if RT.with(|rt| rt.borrow().node(self.id).is_none()) {
            return None;
        }
        update_if_necessary(self.id);
        track(self.id);
        let value: Value = RT.with(|rt| rt.borrow().node(self.id)?.value.clone())?;
        value.downcast_ref::<T>().cloned()
    }

    pub fn dispose(&self) {
        dispose(self.id);
    }
}

/// A terminal reaction — in PliegoRS, almost always a render effect.
pub struct Effect {
    id: NodeId,
    _local: PhantomData<Rc<()>>,
}

impl Effect {
    /// Create and run once now; re-runs whenever tracked dependencies change.
    pub fn new(f: impl Fn() + 'static) -> Self {
        Self::install(f, None)
    }

    fn new_in_owner(f: impl Fn() + 'static, owner: NodeId) -> Self {
        Self::install(f, Some(owner))
    }

    fn install(f: impl Fn() + 'static, explicit_owner: Option<NodeId>) -> Self {
        let compute: Compute = Rc::new(move || {
            f();
            Rc::new(()) as Value
        });
        let id = RT.with(|rt| {
            let node = Node {
                kind: Kind::Effect,
                color: Color::Dirty,
                owner: None,
                dispose_requested: false,
                queued: false,
                updating: false,
                running: false,
                retry_notify: false,
                value: None,
                compute: Some(compute),
                eq: None,
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            };
            let mut rt = rt.borrow_mut();
            match explicit_owner {
                Some(owner) => rt.add_owned_node(node, owner),
                None => rt.add_node(node),
            }
        });
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| run_computation(id))) {
            if let Err(disposal_panic) = catch_unwind(AssertUnwindSafe(|| dispose(id))) {
                suppress_panic(disposal_panic);
            }
            resume_unwind(payload);
        }
        Effect {
            id,
            _local: PhantomData,
        }
    }

    /// Stop this effect: cleanups run, edges removed, never fires again.
    pub fn dispose(&self) {
        dispose(self.id);
    }
}

/// Register a cleanup on the currently-running computation.
///
/// A failed replacement keeps the previous stable scope alive. Consequently,
/// its cleanups run only after a successful replacement commits, or on disposal.
pub fn on_cleanup(f: impl FnOnce() + 'static) {
    RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        if let Some(owner) = rt.owner {
            let frame = rt
                .frames
                .last_mut()
                .expect("active owner must have a computation frame");
            assert_eq!(frame.observer, owner, "owner/frame mismatch");
            frame.cleanups.push(Box::new(f));
        }
    });
}

/// Run `f` without tracking (reads inside register no dependencies).
pub fn untrack<T>(f: impl FnOnce() -> T) -> T {
    let _guard = ObserverGuard::replace(None);
    f()
}

fn dispose(id: NodeId) {
    let batch_guard = BatchGuard::enter();
    let batch = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        assert_disposable_tree(&rt, id);
        let mut batch = DisposalBatch::default();
        collect_disposal(&mut rt, id, &mut batch);
        batch
    });
    let mut cleanup_panic = run_disposal(batch);
    merge_panic(&mut cleanup_panic, drain_deferred_disposals());
    let should_flush = batch_guard.finish();
    if let Some(payload) = cleanup_panic {
        flush_while_preserving(payload, should_flush);
    }
    if should_flush {
        flush_effects();
    }
}

fn dispose_owner(id: NodeId) {
    let batch_guard = BatchGuard::enter();
    let batch = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let node = rt.node(id)?;
        if node.kind != Kind::Owner {
            return None;
        }
        if disposable_tree_is_busy(&rt, id) {
            rt.defer_owner_disposal(id);
            return None;
        }

        let mut batch = DisposalBatch::default();
        collect_disposal(&mut rt, id, &mut batch);
        Some(batch)
    });
    let mut cleanup_panic = batch.and_then(run_disposal);
    merge_panic(&mut cleanup_panic, drain_deferred_disposals());
    let should_flush = batch_guard.finish();
    if let Some(payload) = cleanup_panic {
        flush_while_preserving(payload, should_flush);
    }
    if should_flush {
        flush_effects();
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeStats {
    slots_total: usize,
    slots_live: usize,
    slots_free: usize,
    pending: usize,
    deferred_disposals: usize,
    observer_present: bool,
    owner_present: bool,
    flushing: bool,
    owned_edges: usize,
    source_edges: usize,
    subscriber_edges: usize,
    cleanup_count: usize,
}

#[cfg(test)]
fn reset_runtime_for_test() {
    let previous = RT.with(|rt| rt.replace(Runtime::default()));
    drop(previous);
}

#[cfg(test)]
fn runtime_stats() -> RuntimeStats {
    RT.with(|rt| {
        let rt = rt.borrow();
        let nodes = rt.slots.iter().filter_map(|slot| slot.node.as_ref());
        let (mut slots_live, mut owned_edges, mut source_edges) = (0, 0, 0);
        let (mut subscriber_edges, mut cleanup_count) = (0, 0);
        for node in nodes {
            slots_live += 1;
            owned_edges += node.owned.len();
            source_edges += node.sources.len();
            subscriber_edges += node.subscribers.len();
            cleanup_count += node.cleanups.len();
        }
        RuntimeStats {
            slots_total: rt.slots.len(),
            slots_live,
            slots_free: rt.free.len(),
            pending: rt.pending.len(),
            deferred_disposals: rt.deferred_disposals.len(),
            observer_present: rt.observer.is_some(),
            owner_present: rt.owner.is_some(),
            flushing: rt.flushing,
            owned_edges,
            source_edges,
            subscriber_edges,
            cleanup_count,
        }
    })
}

#[cfg(test)]
fn assert_runtime_invariants() {
    use std::collections::HashSet;

    fn assert_unique(ids: &[NodeId], label: &str) {
        let unique: HashSet<_> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate IDs in {label}");
    }

    RT.with(|rt| {
        let rt = rt.borrow();
        assert!(rt.observer.is_none(), "observer leaked outside computation");
        assert!(rt.owner.is_none(), "owner leaked outside computation");
        assert!(rt.frames.is_empty(), "computation frame leaked");
        assert!(!rt.flushing, "flushing leaked outside scheduler");
        assert_eq!(rt.batch_depth, 0, "batch depth leaked");
        assert!(
            rt.deferred_disposals.is_empty(),
            "ready owner disposal was not drained"
        );

        let free: HashSet<_> = rt.free.iter().copied().collect();
        assert_eq!(free.len(), rt.free.len(), "duplicate slots in free list");
        for slot in &free {
            assert!(
                rt.slots[*slot as usize].node.is_none(),
                "free slot contains a live node"
            );
        }

        let pending: HashSet<_> = rt.pending.iter().copied().collect();
        assert_eq!(pending.len(), rt.pending.len(), "duplicate pending effect");
        for id in &rt.pending {
            let node = rt.node(*id).expect("pending contains stale node ID");
            assert_eq!(node.kind, Kind::Effect, "pending node is not an effect");
            assert!(node.queued, "pending effect is missing queued flag");
        }
        let deferred: HashSet<_> = rt.deferred_disposals.iter().copied().collect();
        assert_eq!(
            deferred.len(),
            rt.deferred_disposals.len(),
            "duplicate deferred owner disposal"
        );
        for id in &rt.deferred_disposals {
            let node = rt.node(*id).expect("deferred disposal contains stale ID");
            assert_eq!(node.kind, Kind::Owner, "deferred node is not an owner");
            assert!(node.dispose_requested, "deferred owner lacks request flag");
        }

        for (slot_index, slot) in rt.slots.iter().enumerate() {
            let Some(node) = &slot.node else { continue };
            let id = NodeId {
                slot: u32::try_from(slot_index).expect("test arena exceeds u32"),
                generation: slot.generation,
            };

            assert!(!node.running, "computation left running");
            assert!(!node.updating, "signal left updating");
            if node.retry_notify {
                assert_eq!(node.kind, Kind::Memo, "non-memo requested retry notify");
                assert_eq!(node.color, Color::Dirty, "retry memo is not dirty");
            }
            assert_eq!(node.queued, pending.contains(&id), "queued flag mismatch");
            assert_eq!(
                node.dispose_requested,
                deferred.contains(&id),
                "deferred disposal flag mismatch"
            );
            assert_unique(&node.sources, "sources");
            assert_unique(&node.subscribers, "subscribers");
            assert_unique(&node.owned, "owned children");
            assert!(!node.sources.contains(&id), "node sources itself");
            assert!(!node.subscribers.contains(&id), "node subscribes itself");
            assert!(!node.owned.contains(&id), "node owns itself");
            if node.kind == Kind::Owner {
                assert_eq!(node.color, Color::Clean, "owner color changed");
                assert!(node.value.is_none(), "owner stores a reactive value");
                assert!(node.compute.is_none(), "owner stores a computation");
                assert!(node.eq.is_none(), "owner stores an equality callback");
                assert!(node.sources.is_empty(), "owner has reactive sources");
                assert!(node.subscribers.is_empty(), "owner has subscribers");
            } else {
                assert!(
                    !node.dispose_requested,
                    "non-owner requested deferred disposal"
                );
            }

            if let Some(owner) = node.owner {
                let owner_node = rt.node(owner).expect("child has stale owner");
                assert_eq!(
                    owner_node
                        .owned
                        .iter()
                        .filter(|child| **child == id)
                        .count(),
                    1,
                    "owner does not contain child exactly once"
                );
            }
            for child in &node.owned {
                let child_node = rt.node(*child).expect("owner has stale child");
                assert_eq!(child_node.owner, Some(id), "child owner mismatch");
            }
            for source in &node.sources {
                let source_node = rt.node(*source).expect("node has stale source");
                assert!(
                    source_node.subscribers.contains(&id),
                    "source edge lacks inverse subscriber edge"
                );
            }
            for subscriber in &node.subscribers {
                let subscriber_node = rt.node(*subscriber).expect("node has stale subscriber");
                assert!(
                    subscriber_node.sources.contains(&id),
                    "subscriber edge lacks inverse source edge"
                );
            }
        }
    });
}

// ───────────────────────────── tests (the M2 gate) ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// THE M2 GATE 1: the diamond. One source feeding two memos feeding one
    /// effect → a write runs the effect exactly ONCE (glitch-free).
    #[test]
    fn gate_diamond_runs_effect_once() {
        reset_runtime_for_test();
        let src = Signal::new(1);
        let a = Memo::new(move || src.get() * 2);
        let b = Memo::new(move || src.get() + 10);
        let runs = Rc::new(Cell::new(0));
        let seen = Rc::new(Cell::new(0));
        {
            let runs = runs.clone();
            let seen = seen.clone();
            Effect::new(move || {
                runs.set(runs.get() + 1);
                seen.set(a.get() + b.get());
            });
        }
        assert_eq!(runs.get(), 1); // initial run
        assert_eq!(seen.get(), 2 + 11);
        src.set(5);
        assert_eq!(runs.get(), 2, "diamond must fire the effect exactly once");
        assert_eq!(seen.get(), 10 + 15);
        assert_runtime_invariants();
    }

    /// THE M2 GATE 2: dynamic dependencies — the branch not taken is not
    /// tracked; writes to it cause no recompute.
    #[test]
    fn gate_untaken_branch_not_tracked() {
        reset_runtime_for_test();
        let flag = Signal::new(true);
        let a = Signal::new(1);
        let b = Signal::new(100);
        let computes = Rc::new(Cell::new(0));
        let m = {
            let computes = computes.clone();
            Memo::new(move || {
                computes.set(computes.get() + 1);
                if flag.get() { a.get() } else { b.get() }
            })
        };
        assert_eq!(m.get(), 1);
        assert_eq!(computes.get(), 1);

        b.set(200); // untaken branch: must NOT recompute
        assert_eq!(m.get(), 1);
        assert_eq!(computes.get(), 1, "untracked branch caused a recompute");

        flag.set(false); // switch branches → recompute, now tracks b
        assert_eq!(m.get(), 200);
        assert_eq!(computes.get(), 2);

        a.set(7); // now A is the untaken one
        assert_eq!(m.get(), 200);
        assert_eq!(computes.get(), 2, "stale dependency survived the re-track");
        assert_runtime_invariants();
    }

    /// THE M2 GATE 3: equality gating — a memo recomputing to an equal value
    /// does not wake its subscribers (x*0 stays 0; the effect must not re-run).
    #[test]
    fn gate_equality_gate_stops_propagation() {
        reset_runtime_for_test();
        let x = Signal::new(3);
        let zero = Memo::new(move || {
            let _ = x.get();
            0
        });
        let runs = Rc::new(Cell::new(0));
        {
            let runs = runs.clone();
            Effect::new(move || {
                runs.set(runs.get() + 1);
                let _ = zero.get();
            });
        }
        assert_eq!(runs.get(), 1);
        x.set(42);
        x.set(-1);
        assert_eq!(runs.get(), 1, "equal memo value must not wake the effect");
        assert_runtime_invariants();
    }

    /// THE M2 GATE 4: disposal is clean — cleanups run, effects stop firing,
    /// disposed signals read as None.
    #[test]
    fn gate_disposal_is_clean() {
        reset_runtime_for_test();
        let s = Signal::new(0);
        let runs = Rc::new(Cell::new(0));
        let cleaned = Rc::new(Cell::new(false));
        let eff = {
            let runs = runs.clone();
            let cleaned = cleaned.clone();
            Effect::new(move || {
                let _ = s.get();
                runs.set(runs.get() + 1);
                let cleaned = cleaned.clone();
                on_cleanup(move || cleaned.set(true));
            })
        };
        assert_eq!(runs.get(), 1);
        s.set(1);
        assert_eq!(runs.get(), 2);

        eff.dispose();
        assert!(cleaned.get(), "cleanup must run on disposal");
        s.set(2);
        assert_eq!(runs.get(), 2, "disposed effect must not fire");

        s.dispose();
        assert_eq!(s.try_get(), None, "disposed signal reads as None");
        assert_runtime_invariants();
    }

    /// Lazy pull: a memo nobody reads doesn't recompute on writes.
    #[test]
    fn memo_is_lazy() {
        reset_runtime_for_test();
        let s = Signal::new(1);
        let computes = Rc::new(Cell::new(0));
        let m = {
            let computes = computes.clone();
            Memo::new(move || {
                computes.set(computes.get() + 1);
                s.get() * 2
            })
        };
        assert_eq!(computes.get(), 0, "memo must not run before first read");
        s.set(2);
        s.set(3);
        assert_eq!(
            computes.get(),
            0,
            "unread memo must not recompute on writes"
        );
        assert_eq!(m.get(), 6);
        assert_eq!(computes.get(), 1, "reads collapse into one recompute");
        assert_runtime_invariants();
    }

    /// untrack() suppresses subscription.
    #[test]
    fn untrack_reads_do_not_subscribe() {
        reset_runtime_for_test();
        let s = Signal::new(1);
        let runs = Rc::new(Cell::new(0));
        {
            let runs = runs.clone();
            Effect::new(move || {
                runs.set(runs.get() + 1);
                let _ = untrack(|| s.get());
            });
        }
        assert_eq!(runs.get(), 1);
        s.set(2);
        assert_eq!(runs.get(), 1, "untracked read must not subscribe");
        assert_runtime_invariants();
    }

    /// Chained memos propagate through, still one effect run per write.
    #[test]
    fn chains_propagate_once() {
        reset_runtime_for_test();
        let s = Signal::new(1);
        let a = Memo::new(move || s.get() + 1);
        let b = Memo::new(move || a.get() + 1);
        let c = Memo::new(move || b.get() + 1);
        let runs = Rc::new(Cell::new(0));
        let last = Rc::new(Cell::new(0));
        {
            let runs = runs.clone();
            let last = last.clone();
            Effect::new(move || {
                runs.set(runs.get() + 1);
                last.set(c.get());
            });
        }
        assert_eq!((runs.get(), last.get()), (1, 4));
        s.set(10);
        assert_eq!((runs.get(), last.get()), (2, 13));
        assert_runtime_invariants();
    }

    #[test]
    fn observer_is_restored_and_failed_memo_can_retry() {
        reset_runtime_for_test();
        let source = Signal::new(1);
        let panic_once = Rc::new(Cell::new(true));
        let memo = {
            let panic_once = panic_once.clone();
            Memo::new(move || {
                let value = source.get();
                if panic_once.replace(false) {
                    panic!("intentional memo panic");
                }
                value * 2
            })
        };

        let first = catch_unwind(AssertUnwindSafe(|| memo.get()));
        assert!(first.is_err());
        assert_eq!(memo.get(), 2, "failed memo must remain retryable");

        let healthy_source = Signal::new(4);
        let seen = Rc::new(Cell::new(0));
        let healthy = {
            let seen = seen.clone();
            Effect::new(move || seen.set(healthy_source.get()))
        };
        healthy_source.set(7);
        assert_eq!(seen.get(), 7);
        healthy.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn untrack_restores_observer_during_unwind() {
        reset_runtime_for_test();
        let source = Signal::new(1);
        let computes = Rc::new(Cell::new(0));
        let memo = {
            let computes = computes.clone();
            Memo::new(move || {
                computes.set(computes.get() + 1);
                let panic = catch_unwind(AssertUnwindSafe(|| {
                    untrack(|| panic!("intentional untrack panic"));
                }));
                assert!(panic.is_err());
                source.get()
            })
        };

        assert_eq!(memo.get(), 1);
        source.set(2);
        assert_eq!(memo.get(), 2);
        assert_eq!(computes.get(), 2, "observer was not restored after untrack");
        assert_runtime_invariants();
    }

    #[test]
    fn flushing_is_restored_after_effect_panic() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let runs = Rc::new(Cell::new(0));
        let seen = Rc::new(Cell::new(0));
        let effect = {
            let runs = runs.clone();
            let seen = seen.clone();
            Effect::new(move || {
                let value = source.get();
                runs.set(runs.get() + 1);
                if value == 1 {
                    panic!("intentional effect panic");
                }
                seen.set(value);
            })
        };

        let failed_write = catch_unwind(AssertUnwindSafe(|| source.set(1)));
        assert!(failed_write.is_err());
        assert!(!runtime_stats().flushing, "scheduler remained poisoned");

        source.set(2);
        assert_eq!(seen.get(), 2);
        assert_eq!(runs.get(), 3, "effect did not run after scheduler recovery");
        effect.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn scheduler_drains_healthy_nested_effects_before_resuming_panic() {
        reset_runtime_for_test();
        let trigger = Signal::new(0);
        let nested = Signal::new(0);
        let observed = Rc::new(Cell::new(0));
        let watcher = {
            let observed = observed.clone();
            Effect::new(move || observed.set(nested.get()))
        };
        let failing = Effect::new(move || {
            let value = trigger.get();
            if value == 1 {
                nested.set(9);
                panic!("intentional effect panic after nested write");
            }
        });

        let failed = catch_unwind(AssertUnwindSafe(|| trigger.set(1)));
        assert!(failed.is_err());
        assert_eq!(nested.get_untracked(), 9);
        assert_eq!(observed.get(), 9, "healthy nested effect remained pending");
        assert_eq!(runtime_stats().pending, 0);
        assert!(!runtime_stats().flushing);

        trigger.set(2);
        failing.dispose();
        watcher.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn update_runs_without_runtime_borrow_and_rolls_back_on_panic() {
        reset_runtime_for_test();
        let a = Signal::new(10);
        let b = Signal::new(5);
        a.update(|value| *value += b.get());
        assert_eq!(a.get_untracked(), 15);

        let failed = catch_unwind(AssertUnwindSafe(|| {
            a.update(|value| {
                *value = 99;
                panic!("intentional update panic");
            });
        }));
        assert!(failed.is_err());
        assert_eq!(a.get_untracked(), 15, "panic published a partial update");

        a.update(|value| *value += 1);
        assert_eq!(a.get_untracked(), 16, "signal remained poisoned");
        assert_runtime_invariants();
    }

    #[test]
    fn nested_updates_are_batched_and_same_signal_reentrancy_is_rejected() {
        reset_runtime_for_test();
        let a = Signal::new(1);
        let b = Signal::new(10);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let effect = {
            let seen = seen.clone();
            Effect::new(move || seen.borrow_mut().push((a.get(), b.get())))
        };

        a.update(|value| {
            *value = 2;
            b.update(|nested| *nested = 11);
        });
        assert_eq!(&*seen.borrow(), &[(1, 10), (2, 11)]);

        let alias = a;
        let reentrant = catch_unwind(AssertUnwindSafe(|| {
            a.update(|value| {
                *value = 7;
                alias.set(8);
            });
        }));
        assert!(reentrant.is_err());
        assert_eq!(
            a.get_untracked(),
            2,
            "reentrant update was partially published"
        );
        effect.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn nested_write_is_observed_before_outer_update_panic_resumes() {
        reset_runtime_for_test();
        let outer = Signal::new(0);
        let nested = Signal::new(0);
        let observed = Rc::new(Cell::new(0));
        let watcher = {
            let observed = observed.clone();
            Effect::new(move || observed.set(nested.get()))
        };

        let failed = catch_unwind(AssertUnwindSafe(|| {
            outer.update(|candidate| {
                *candidate = 1;
                nested.set(7);
                panic!("intentional outer update panic");
            });
        }));
        assert!(failed.is_err());
        assert_eq!(outer.get_untracked(), 0, "outer candidate was published");
        assert_eq!(nested.get_untracked(), 7, "nested committed write was lost");
        assert_eq!(observed.get(), 7, "nested effect remained stuck in pending");
        assert_eq!(runtime_stats().pending, 0);
        watcher.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn computation_cannot_write_a_stable_or_provisional_source() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let attempt_write = Rc::new(Cell::new(false));
        let memo = {
            let attempt_write = attempt_write.clone();
            Memo::new(move || {
                let value = source.get();
                if attempt_write.get() {
                    source.set(value + 1);
                }
                value
            })
        };
        assert_eq!(memo.get(), 0);

        attempt_write.set(true);
        source.set(1);
        let failed = catch_unwind(AssertUnwindSafe(|| memo.get()));
        assert!(failed.is_err());
        assert_eq!(source.get_untracked(), 1, "self-write was published");
        RT.with(|rt| {
            let rt = rt.borrow();
            let node = rt.node(memo.id).expect("memo disappeared");
            assert_eq!(node.color, Color::Dirty);
            assert_eq!(
                node.value
                    .as_ref()
                    .and_then(|value| value.downcast_ref::<i32>())
                    .copied(),
                Some(0)
            );
        });

        attempt_write.set(false);
        assert_eq!(memo.get(), 1, "memo did not recover after rejected write");
        assert_runtime_invariants();
    }

    #[test]
    fn computation_cannot_write_a_transitive_source() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let derived = Memo::new(move || source.get());
        let rendered = Rc::new(Cell::new(-1));

        let failed = {
            let rendered = rendered.clone();
            catch_unwind(AssertUnwindSafe(|| {
                Effect::new(move || {
                    let value = derived.get();
                    rendered.set(value);
                    if value == 0 {
                        source.set(1);
                    }
                });
            }))
        };

        assert!(failed.is_err());
        assert_eq!(rendered.get(), 0);
        assert_eq!(source.get_untracked(), 0, "transitive write was published");
        assert_eq!(derived.get(), 0, "derived value was corrupted");
        assert_eq!(runtime_stats().slots_live, 2, "failed effect leaked");

        derived.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn untrack_suppresses_dependencies_without_suppressing_ownership() {
        reset_runtime_for_test();
        let parent_source = Signal::new(0);
        let child_source = Signal::new(0);
        let child_runs = Rc::new(Cell::new(0));
        let parent = {
            let child_runs = child_runs.clone();
            Effect::new(move || {
                let _ = parent_source.get();
                let child_runs = child_runs.clone();
                untrack(|| {
                    Effect::new(move || {
                        let _ = child_source.get();
                        child_runs.set(child_runs.get() + 1);
                    });
                });
            })
        };
        assert_eq!(child_runs.get(), 1);

        parent.dispose();
        child_source.set(1);
        assert_eq!(
            child_runs.get(),
            1,
            "untracked child escaped owner disposal"
        );
        assert_runtime_invariants();
    }

    #[test]
    fn clone_and_drop_user_code_run_without_runtime_borrows() {
        reset_runtime_for_test();

        struct CloneWrites {
            target: Signal<i32>,
        }
        impl Clone for CloneWrites {
            fn clone(&self) -> Self {
                self.target.set(self.target.get_untracked() + 1);
                Self {
                    target: self.target,
                }
            }
        }
        impl PartialEq for CloneWrites {
            fn eq(&self, _other: &Self) -> bool {
                true
            }
        }

        struct DropWrites {
            target: Signal<i32>,
            value: i32,
        }
        impl Drop for DropWrites {
            fn drop(&mut self) {
                self.target.set(self.value);
            }
        }

        let clone_target = Signal::new(0);
        let holder = Signal::new(CloneWrites {
            target: clone_target,
        });
        let _ = holder.get_untracked();
        let _ = holder.try_get();
        let memo = Memo::new(move || CloneWrites {
            target: clone_target,
        });
        let _ = memo.get();
        assert_eq!(clone_target.get_untracked(), 3);

        let drop_target = Signal::new(0);
        let observed = Rc::new(Cell::new(0));
        let watcher = {
            let observed = observed.clone();
            Effect::new(move || observed.set(drop_target.get()))
        };
        let drop_holder = Signal::new(DropWrites {
            target: drop_target,
            value: 1,
        });
        drop_holder.set(DropWrites {
            target: drop_target,
            value: 2,
        });
        assert_eq!(observed.get(), 1, "old value Drop did not notify safely");
        drop_holder.dispose();
        assert_eq!(
            observed.get(),
            2,
            "disposed value Drop did not notify safely"
        );

        holder.dispose();
        memo.dispose();
        watcher.dispose();
        clone_target.dispose();
        drop_target.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn effect_can_update_another_signal_deterministically() {
        reset_runtime_for_test();
        let trigger = Signal::new(0);
        let target = Signal::new(0);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let target_effect = {
            let seen = seen.clone();
            Effect::new(move || seen.borrow_mut().push(target.get()))
        };
        let bridge_effect = Effect::new(move || {
            let value = trigger.get();
            if value > 0 {
                target.set(value * 10);
            }
        });

        trigger.set(2);
        assert_eq!(&*seen.borrow(), &[0, 20]);
        bridge_effect.dispose();
        target_effect.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn scheduler_is_fifo_and_deduplicates_within_a_batch() {
        reset_runtime_for_test();
        let a = Signal::new(0);
        let b = Signal::new(0);
        let order = Rc::new(RefCell::new(Vec::new()));
        let first = {
            let order = order.clone();
            Effect::new(move || {
                let _ = (a.get(), b.get());
                order.borrow_mut().push(1);
            })
        };
        let second = {
            let order = order.clone();
            Effect::new(move || {
                let _ = (a.get(), b.get());
                order.borrow_mut().push(2);
            })
        };
        order.borrow_mut().clear();

        a.update(|value| {
            *value = 1;
            b.set(1);
            b.set(2);
        });
        assert_eq!(&*order.borrow(), &[1, 2]);
        assert_eq!(runtime_stats().pending, 0);
        first.dispose();
        second.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn scheduler_rejects_an_effect_feedback_cycle_without_hanging() {
        reset_runtime_for_test();
        let x = Signal::new(0);
        let y = Signal::new(0);
        let enabled = Rc::new(Cell::new(false));
        let first = {
            let enabled = enabled.clone();
            Effect::new(move || {
                let value = x.get();
                if enabled.get() {
                    y.set(value + 1);
                }
            })
        };
        let second = {
            let enabled = enabled.clone();
            Effect::new(move || {
                let value = y.get();
                if enabled.get() {
                    x.set(value + 1);
                }
            })
        };

        enabled.set(true);
        let failed = catch_unwind(AssertUnwindSafe(|| x.set(1)));
        assert!(failed.is_err(), "feedback cycle was not rejected");
        assert_eq!(runtime_stats().pending, 0);
        assert!(!runtime_stats().flushing);

        enabled.set(false);
        x.set(0);
        y.set(0);
        first.dispose();
        second.dispose();
        x.dispose();
        y.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn scheduler_completes_a_long_finite_effect_chain() {
        reset_runtime_for_test();
        const STAGES: usize = 101;

        let enabled = Rc::new(Cell::new(false));
        let triggers: Vec<_> = (0..STAGES).map(|_| Signal::new(0usize)).collect();
        let output = Signal::new(0usize);
        let seen = Rc::new(Cell::new(0usize));
        let consumer_runs = Rc::new(Cell::new(0usize));
        let consumer = {
            let seen = seen.clone();
            let consumer_runs = consumer_runs.clone();
            Effect::new(move || {
                consumer_runs.set(consumer_runs.get() + 1);
                seen.set(output.get());
            })
        };
        let mut stages = Vec::with_capacity(STAGES);
        for index in 0..STAGES {
            let enabled = enabled.clone();
            let input = triggers[index];
            let next = triggers.get(index + 1).copied();
            stages.push(Effect::new(move || {
                let value = input.get();
                if enabled.get() {
                    output.set(index + 1);
                    if let Some(next) = next {
                        next.set(value + 1);
                    }
                }
            }));
        }

        enabled.set(true);
        triggers[0].set(1);
        assert_eq!(seen.get(), STAGES);
        assert_eq!(consumer_runs.get(), STAGES + 1);
        assert_eq!(runtime_stats().pending, 0);

        for stage in stages {
            stage.dispose();
        }
        consumer.dispose();
        for trigger in triggers {
            trigger.dispose();
        }
        output.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn scheduler_budget_stops_unbounded_effect_id_churn() {
        reset_runtime_for_test();
        type Factory = Rc<dyn Fn()>;

        let trigger = Signal::new(0usize);
        let factory_slot: Rc<RefCell<Option<Factory>>> = Rc::new(RefCell::new(None));
        let factory_slot_for_closure = factory_slot.clone();
        let factory: Factory = Rc::new(move || {
            let factory_slot = factory_slot_for_closure.clone();
            Effect::new(move || {
                let _ = trigger.get();
                let factory_slot = factory_slot.clone();
                on_cleanup(move || {
                    let next = factory_slot
                        .borrow()
                        .as_ref()
                        .expect("factory disappeared")
                        .clone();
                    next();
                    trigger.set(trigger.get_untracked() + 1);
                });
            });
        });
        *factory_slot.borrow_mut() = Some(factory.clone());
        factory();

        let failed = catch_unwind(AssertUnwindSafe(|| trigger.set(1)));
        assert!(failed.is_err(), "unbounded scheduler churn was not stopped");
        assert_eq!(runtime_stats().pending, 0);
        assert!(!runtime_stats().flushing);
        assert_runtime_invariants();

        reset_runtime_for_test();
        factory_slot.borrow_mut().take();
        drop(factory);
        assert_runtime_invariants();
    }

    #[test]
    fn discarded_scheduler_work_rearms_dirty_memo_chains() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let memo = Memo::new(move || source.get());
        let seen = Rc::new(Cell::new(0));
        let watcher = {
            let seen = seen.clone();
            Effect::new(move || seen.set(memo.get()))
        };

        let batch = BatchGuard::enter();
        source.set(1);
        assert_eq!(runtime_stats().pending, 1);
        RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            let current = rt.pop_pending().expect("watcher was not queued");
            rt.abort_pending(current);
        });
        assert!(batch.finish());
        assert_eq!(seen.get(), 0);

        source.set(2);
        assert_eq!(seen.get(), 2, "discarded effect was not rearmed");
        watcher.dispose();
        memo.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn rerun_disposes_previous_children_exactly_once() {
        reset_runtime_for_test();
        let rerun = Signal::new(0);
        let leaf = Signal::new(0);
        let child_runs = Rc::new(Cell::new(0));
        let child_cleanups = Rc::new(Cell::new(0));
        let parent = {
            let child_runs = child_runs.clone();
            let child_cleanups = child_cleanups.clone();
            Effect::new(move || {
                let _ = rerun.get();
                let child_runs = child_runs.clone();
                let child_cleanups = child_cleanups.clone();
                Effect::new(move || {
                    let _ = leaf.get();
                    child_runs.set(child_runs.get() + 1);
                    let child_cleanups = child_cleanups.clone();
                    on_cleanup(move || child_cleanups.set(child_cleanups.get() + 1));
                });
            })
        };

        assert_eq!(child_runs.get(), 1);
        rerun.set(1);
        assert_eq!(child_runs.get(), 2);
        assert_eq!(child_cleanups.get(), 1);
        leaf.set(1);
        assert_eq!(child_runs.get(), 3, "disposed child reacted after rerun");
        assert_eq!(
            child_cleanups.get(),
            2,
            "live child cleanup did not run on rerun"
        );

        parent.dispose();
        assert_eq!(child_cleanups.get(), 3);
        assert_runtime_invariants();
    }

    #[test]
    fn recursive_disposal_is_lifo_and_idempotent() {
        reset_runtime_for_test();
        let order = Rc::new(RefCell::new(Vec::new()));
        let root = {
            let order = order.clone();
            Effect::new(move || {
                let first = order.clone();
                on_cleanup(move || first.borrow_mut().push("root-a"));
                let second = order.clone();
                on_cleanup(move || second.borrow_mut().push("root-b"));

                let order = order.clone();
                Effect::new(move || {
                    let first = order.clone();
                    on_cleanup(move || first.borrow_mut().push("child-a"));
                    let second = order.clone();
                    on_cleanup(move || second.borrow_mut().push("child-b"));

                    let order = order.clone();
                    Effect::new(move || {
                        let first = order.clone();
                        on_cleanup(move || first.borrow_mut().push("grandchild-a"));
                        let second = order.clone();
                        on_cleanup(move || second.borrow_mut().push("grandchild-b"));
                    });
                });
            })
        };

        root.dispose();
        assert_eq!(
            &*order.borrow(),
            &[
                "grandchild-b",
                "grandchild-a",
                "child-b",
                "child-a",
                "root-b",
                "root-a",
            ]
        );
        root.dispose();
        assert_eq!(order.borrow().len(), 6, "second dispose repeated cleanup");
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_cleanups_are_lifo_and_disposal_is_idempotent() {
        reset_runtime_for_test();
        let order = Rc::new(RefCell::new(Vec::new()));
        let owner = Owner::new();
        for label in ["first", "second", "third"] {
            let order = order.clone();
            owner
                .on_cleanup(move || order.borrow_mut().push(label))
                .unwrap();
        }

        owner.dispose();
        owner.dispose();
        assert_eq!(&*order.borrow(), &["third", "second", "first"]);
        drop(owner);
        assert_eq!(order.borrow().len(), 3, "Drop repeated owner cleanup");
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_disposes_children_newest_first_and_exactly_once() {
        reset_runtime_for_test();
        let order = Rc::new(RefCell::new(Vec::new()));
        let root = Owner::new();
        let root_order = order.clone();
        root.on_cleanup(move || root_order.borrow_mut().push("root"))
            .unwrap();

        let first = root.child().unwrap();
        let first_order = order.clone();
        first
            .on_cleanup(move || first_order.borrow_mut().push("first"))
            .unwrap();
        let grandchild = first.child().unwrap();
        let grandchild_order = order.clone();
        grandchild
            .on_cleanup(move || grandchild_order.borrow_mut().push("grandchild"))
            .unwrap();

        let second = root.child().unwrap();
        let second_order = order.clone();
        second
            .on_cleanup(move || second_order.borrow_mut().push("second"))
            .unwrap();

        root.dispose();
        assert_eq!(&*order.borrow(), &["second", "grandchild", "first", "root"]);
        assert!(first.is_disposed());
        assert!(grandchild.is_disposed());
        assert!(second.is_disposed());

        drop(second);
        drop(grandchild);
        drop(first);
        drop(root);
        assert_eq!(order.borrow().len(), 4, "stale child handle cleaned twice");
        assert_runtime_invariants();
    }

    #[test]
    fn dropping_child_owner_detaches_it_from_live_parent() {
        reset_runtime_for_test();
        let cleanups = Rc::new(Cell::new(0));
        let root = Owner::new();
        let child = root.child().unwrap();
        let child_cleanups = cleanups.clone();
        child
            .on_cleanup(move || child_cleanups.set(child_cleanups.get() + 1))
            .unwrap();
        assert_eq!(runtime_stats().owned_edges, 1);

        drop(child);
        assert_eq!(cleanups.get(), 1);
        assert_eq!(runtime_stats().owned_edges, 0);
        root.dispose();
        assert_eq!(cleanups.get(), 1, "parent repeated detached child cleanup");
        drop(root);
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_cleanup_panic_runs_remaining_work_and_restores_runtime() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let observed = Rc::new(Cell::new(0));
        let watcher = {
            let observed = observed.clone();
            Effect::new(move || observed.set(source.get()))
        };
        let order = Rc::new(RefCell::new(Vec::new()));
        let owner = Owner::new();
        let remaining_order = order.clone();
        owner
            .on_cleanup(move || {
                remaining_order.borrow_mut().push("remaining");
                source.set(1);
            })
            .unwrap();
        owner
            .on_cleanup(|| panic!("intentional explicit owner cleanup panic"))
            .unwrap();
        let before_order = order.clone();
        owner
            .on_cleanup(move || before_order.borrow_mut().push("before"))
            .unwrap();

        let outcome = catch_unwind(AssertUnwindSafe(|| owner.dispose()));
        assert!(outcome.is_err());
        assert_eq!(&*order.borrow(), &["before", "remaining"]);
        assert_eq!(observed.get(), 1, "healthy queued work was not flushed");
        owner.dispose();

        let proof = Owner::new();
        let runs = Rc::new(Cell::new(0));
        let proof_effect = {
            let runs = runs.clone();
            proof.effect(move || runs.set(runs.get() + 1)).unwrap()
        };
        assert_eq!(
            runs.get(),
            1,
            "runtime remained poisoned after cleanup panic"
        );
        drop(proof);
        proof_effect.dispose();
        watcher.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_drop_is_raii_without_double_cleanup() {
        reset_runtime_for_test();
        let cleanups = Rc::new(Cell::new(0));
        {
            let owner = Owner::new();
            let cleanups = cleanups.clone();
            owner
                .on_cleanup(move || cleanups.set(cleanups.get() + 1))
                .unwrap();
        }
        assert_eq!(cleanups.get(), 1);

        let owner = Owner::new();
        let cleanups_for_explicit = cleanups.clone();
        owner
            .on_cleanup(move || cleanups_for_explicit.set(cleanups_for_explicit.get() + 1))
            .unwrap();
        owner.dispose();
        owner.dispose();
        drop(owner);
        assert_eq!(cleanups.get(), 2);
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_drop_preserves_primary_panic_during_unwind() {
        reset_runtime_for_test();
        let remaining = Rc::new(Cell::new(false));
        let remaining_after_panic = remaining.clone();
        let outcome = catch_unwind(AssertUnwindSafe(move || {
            let owner = Owner::new();
            owner
                .on_cleanup(move || remaining_after_panic.set(true))
                .unwrap();
            owner
                .on_cleanup(|| panic!("secondary owner cleanup panic"))
                .unwrap();
            panic!("primary owner scope panic");
        }));

        let payload = outcome.expect_err("primary panic was swallowed");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"primary owner scope panic"),
            "owner Drop replaced the primary panic"
        );
        assert!(remaining.get(), "cleanup after secondary panic did not run");
        assert_runtime_invariants();
    }

    #[test]
    fn last_owner_handle_dropped_by_owned_effect_is_drained_after_rerun() {
        reset_runtime_for_test();
        let trigger = Signal::new(0);
        let holder = Signal::new(None::<Owner>);
        let runs = Rc::new(Cell::new(0));
        let cleanups = Rc::new(Cell::new(0));
        let owner = Owner::new();
        let cleanup_count = cleanups.clone();
        owner
            .on_cleanup(move || cleanup_count.set(cleanup_count.get() + 1))
            .unwrap();
        let effect = {
            let runs = runs.clone();
            owner
                .effect(move || {
                    runs.set(runs.get() + 1);
                    if trigger.get() == 1 {
                        holder.set(None);
                    }
                })
                .unwrap()
        };
        holder.set(Some(owner));

        trigger.set(1);
        assert_eq!(runs.get(), 2);
        assert_eq!(cleanups.get(), 1);
        assert!(holder.with(Option::is_none));
        let (owners, effects) = RT.with(|rt| {
            let rt = rt.borrow();
            let owners = rt
                .slots
                .iter()
                .filter_map(|slot| slot.node.as_ref())
                .filter(|node| node.kind == Kind::Owner)
                .count();
            let effects = rt
                .slots
                .iter()
                .filter_map(|slot| slot.node.as_ref())
                .filter(|node| node.kind == Kind::Effect)
                .count();
            (owners, effects)
        });
        assert_eq!((owners, effects), (0, 0));
        assert_eq!(runtime_stats().deferred_disposals, 0);

        trigger.set(2);
        assert_eq!(runs.get(), 2, "disposed effect reacted again");
        effect.dispose();
        holder.dispose();
        trigger.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn owner_disposal_requested_during_signal_update_drains_at_safe_point() {
        reset_runtime_for_test();
        let owner_slot = Rc::new(RefCell::new(None));
        let cleanups = Rc::new(Cell::new(0));
        let owner = Owner::new();
        let cleanup_count = cleanups.clone();
        owner
            .on_cleanup(move || cleanup_count.set(cleanup_count.get() + 1))
            .unwrap();
        let signal = owner.signal(0_u32).unwrap();
        *owner_slot.borrow_mut() = Some(owner);

        let owner_slot_for_update = owner_slot.clone();
        signal.update(move |value| {
            *value = 1;
            drop(owner_slot_for_update.borrow_mut().take());
        });

        assert_eq!(cleanups.get(), 1);
        assert!(owner_slot.borrow().is_none());
        assert_eq!(signal.try_get(), None);
        assert_eq!(runtime_stats().deferred_disposals, 0);
        signal.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn deferred_owner_cleanup_panic_cannot_replace_update_panic() {
        reset_runtime_for_test();
        let owner_slot = Rc::new(RefCell::new(None));
        let remaining_cleanup = Rc::new(Cell::new(false));
        let owner = Owner::new();
        let remaining = remaining_cleanup.clone();
        owner.on_cleanup(move || remaining.set(true)).unwrap();
        owner
            .on_cleanup(|| panic!("secondary deferred cleanup panic"))
            .unwrap();
        let signal = owner.signal(0_u32).unwrap();
        *owner_slot.borrow_mut() = Some(owner);

        let owner_slot_for_update = owner_slot.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            signal.update(move |_| {
                drop(owner_slot_for_update.borrow_mut().take());
                panic!("primary signal update panic");
            });
        }));
        let payload = outcome.expect_err("update panic was swallowed");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"primary signal update panic")
        );
        assert!(remaining_cleanup.get());
        assert_eq!(signal.try_get(), None);
        assert_eq!(runtime_stats().slots_live, 0);
        assert_eq!(runtime_stats().deferred_disposals, 0);
        signal.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn owner_pending_disposal_rejects_new_resources_and_cleanups() {
        reset_runtime_for_test();
        let trigger = Signal::new(0);
        let candidate = Rc::new(Effect::new(|| {}));
        let owner = Rc::new(Owner::new());
        let rejected = Rc::new(Cell::new(0));
        let effect = {
            let owner_for_effect = owner.clone();
            let candidate = candidate.clone();
            let rejected = rejected.clone();
            owner
                .effect(move || {
                    if trigger.get() == 1 {
                        owner_for_effect.dispose();
                        rejected.set(
                            usize::from(matches!(
                                owner_for_effect.child(),
                                Err(OwnerError::Disposed)
                            )) + usize::from(
                                owner_for_effect.on_cleanup(|| {}) == Err(OwnerError::Disposed),
                            ) + usize::from(matches!(
                                owner_for_effect.signal(1_u8),
                                Err(OwnerError::Disposed)
                            )) + usize::from(matches!(
                                owner_for_effect.memo(|| 1_u8),
                                Err(OwnerError::Disposed)
                            )) + usize::from(matches!(
                                owner_for_effect.effect(|| {}),
                                Err(OwnerError::Disposed)
                            )) + usize::from(
                                owner_for_effect.adopt_effect(candidate.as_ref())
                                    == Err(OwnerError::Disposed),
                            ),
                        );
                    }
                })
                .unwrap()
        };

        trigger.set(1);
        assert_eq!(rejected.get(), 6);
        assert!(owner.is_disposed());
        assert_eq!(runtime_stats().deferred_disposals, 0);
        effect.dispose();
        candidate.dispose();
        trigger.dispose();
        drop(owner);
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_effect_stops_reacting_after_scope_disposal() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let runs = Rc::new(Cell::new(0));
        let seen = Rc::new(Cell::new(0));
        let owner = Owner::new();
        let effect = {
            let runs = runs.clone();
            let seen = seen.clone();
            owner
                .effect(move || {
                    runs.set(runs.get() + 1);
                    seen.set(source.get());
                })
                .unwrap()
        };

        source.set(1);
        assert_eq!((runs.get(), seen.get()), (2, 1));
        owner.dispose();
        source.set(2);
        assert_eq!((runs.get(), seen.get()), (2, 1));
        effect.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn explicitly_owned_signal_handle_is_stale_after_owner_disposal() {
        reset_runtime_for_test();
        let owner = Owner::new();
        let signal = owner.signal(41_u32).unwrap();
        assert_eq!(signal.try_get(), Some(41));

        owner.dispose();
        assert_eq!(signal.try_get(), None);
        signal.dispose();

        let replacement = Owner::new();
        let current = replacement.signal(42_u32).unwrap();
        assert_eq!(
            signal.try_get(),
            None,
            "stale signal resolved a reused slot"
        );
        assert_eq!(current.try_get(), Some(42));
        replacement.dispose();
        current.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn explicitly_owned_memo_stops_and_is_stale_after_owner_disposal() {
        reset_runtime_for_test();
        let source = Signal::new(1_u32);
        let runs = Rc::new(Cell::new(0));
        let owner = Owner::new();
        let memo = {
            let runs = runs.clone();
            owner
                .memo(move || {
                    runs.set(runs.get() + 1);
                    source.get() * 2
                })
                .unwrap()
        };

        assert_eq!(memo.try_get(), Some(2));
        source.set(2);
        assert_eq!(memo.try_get(), Some(4));
        assert_eq!(runs.get(), 2);
        owner.dispose();
        source.set(3);
        assert_eq!(memo.try_get(), None);
        assert_eq!(runs.get(), 2, "disposed memo recomputed");
        memo.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_adoption_is_fail_closed() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let runs = Rc::new(Cell::new(0));
        let effect = {
            let runs = runs.clone();
            Effect::new(move || {
                let _ = source.get();
                runs.set(runs.get() + 1);
            })
        };
        let owner = Owner::new();
        let other = Owner::new();
        assert_eq!(owner.adopt_effect(&effect), Ok(()));
        assert_eq!(owner.adopt_effect(&effect), Ok(()));
        assert_eq!(other.adopt_effect(&effect), Err(OwnerError::AlreadyOwned));

        source.set(1);
        assert_eq!(runs.get(), 2);
        owner.dispose();
        source.set(2);
        assert_eq!(runs.get(), 2, "adopted effect escaped owner disposal");

        let stale_effect = Effect::new(|| {});
        stale_effect.dispose();
        assert_eq!(
            other.adopt_effect(&stale_effect),
            Err(OwnerError::ResourceDisposed)
        );
        other.dispose();
        stale_effect.dispose();
        effect.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn stale_explicit_owner_cannot_capture_reused_slots() {
        reset_runtime_for_test();
        let stale = Owner::new();
        let stale_id = stale.id;
        stale.dispose();
        let current = Owner::new();
        assert_eq!(stale_id.slot, current.id.slot, "test did not reuse a slot");
        assert_ne!(stale_id.generation, current.id.generation);

        assert!(matches!(stale.child(), Err(OwnerError::Disposed)));
        assert_eq!(stale.on_cleanup(|| {}), Err(OwnerError::Disposed));
        assert!(matches!(stale.signal(1_u8), Err(OwnerError::Disposed)));
        assert!(matches!(stale.memo(|| 1_u8), Err(OwnerError::Disposed)));
        assert!(matches!(stale.effect(|| {}), Err(OwnerError::Disposed)));
        assert!(!current.is_disposed());
        current.dispose();
        drop(current);
        drop(stale);
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_does_not_inherit_ambient_computation() {
        reset_runtime_for_test();
        let slot = Rc::new(RefCell::new(None));
        let parent = {
            let slot = slot.clone();
            Effect::new(move || {
                if slot.borrow().is_none() {
                    *slot.borrow_mut() = Some(Owner::new());
                }
            })
        };
        let owner = slot.borrow_mut().take().expect("owner was not created");
        parent.dispose();
        assert!(
            !owner.is_disposed(),
            "explicit owner inherited ambient scope"
        );

        let ran = Rc::new(Cell::new(false));
        let owned = {
            let ran = ran.clone();
            owner.effect(move || ran.set(true)).unwrap()
        };
        assert!(ran.get());
        owner.dispose();
        owned.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn explicit_owner_arena_plateaus_after_ten_thousand_cycles() {
        reset_runtime_for_test();
        for value in 0..10_000_u32 {
            let owner = Owner::new();
            owner.on_cleanup(|| {}).unwrap();
            let signal = owner.signal(value).unwrap();
            let memo = owner.memo(move || signal.get() + 1).unwrap();
            let effect = owner
                .effect(move || {
                    let _ = memo.get();
                })
                .unwrap();
            owner.dispose();
            effect.dispose();
            memo.dispose();
            signal.dispose();
            drop(owner);
        }

        let stats = runtime_stats();
        assert_eq!(stats.slots_live, 0);
        assert_eq!(stats.slots_total, 4, "owner arena grew with cycle count");
        assert_eq!(stats.slots_free, 4);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.deferred_disposals, 0);
        assert_eq!(stats.owned_edges, 0);
        assert_eq!(stats.source_edges, 0);
        assert_eq!(stats.subscriber_edges, 0);
        assert_eq!(stats.cleanup_count, 0);
        assert!(!stats.observer_present);
        assert!(!stats.owner_present);
        assert!(!stats.flushing);
        assert_runtime_invariants();
    }

    #[test]
    fn successful_replacement_commits_before_retiring_previous_scope() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let order = Rc::new(RefCell::new(Vec::new()));
        let effect = {
            let order = order.clone();
            Effect::new(move || {
                let value = source.get();
                order.borrow_mut().push(format!("run-{value}"));
                let order = order.clone();
                on_cleanup(move || order.borrow_mut().push(format!("cleanup-{value}")));
            })
        };

        source.set(1);
        assert_eq!(
            &*order.borrow(),
            &["run-0", "run-1", "cleanup-0"],
            "transactional replacement order changed"
        );
        effect.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn cleanup_panic_runs_remaining_callbacks_and_preserves_runtime() {
        reset_runtime_for_test();
        let completed = Rc::new(Cell::new(0));
        let touched = Signal::new(0);
        let observed = Rc::new(Cell::new(0));
        let watcher = {
            let observed = observed.clone();
            Effect::new(move || observed.set(touched.get()))
        };
        let effect = {
            let completed = completed.clone();
            Effect::new(move || {
                let completed = completed.clone();
                on_cleanup(move || {
                    touched.set(1);
                    completed.set(completed.get() + 1);
                });
                on_cleanup(|| panic!("intentional cleanup panic"));
            })
        };

        let disposal = catch_unwind(AssertUnwindSafe(|| effect.dispose()));
        assert!(disposal.is_err());
        assert_eq!(completed.get(), 1, "cleanup after panic did not run");
        assert_eq!(
            touched.get_untracked(),
            1,
            "cleanup ran under runtime borrow"
        );
        assert_eq!(
            observed.get(),
            1,
            "cleanup panic left a healthy effect pending"
        );
        effect.dispose();
        watcher.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn disposal_isolates_panics_from_independent_node_fields() {
        reset_runtime_for_test();

        struct ValueBomb;
        impl PartialEq for ValueBomb {
            fn eq(&self, _other: &Self) -> bool {
                true
            }
        }
        impl Drop for ValueBomb {
            fn drop(&mut self) {
                panic!("intentional value drop panic");
            }
        }

        struct CaptureBomb;
        impl Drop for CaptureBomb {
            fn drop(&mut self) {
                panic!("intentional compute capture drop panic");
            }
        }

        let capture = CaptureBomb;
        let memo = Memo::new(move || {
            let _ = &capture;
            ValueBomb
        });
        update_if_necessary(memo.id);

        assert!(catch_unwind(AssertUnwindSafe(|| memo.dispose())).is_err());
        assert_runtime_invariants();
    }

    #[test]
    fn failed_rerun_discards_provisional_children_but_keeps_stable_tree() {
        reset_runtime_for_test();
        let rerun = Signal::new(0);
        let leaf = Signal::new(0);
        let fail = Rc::new(Cell::new(false));
        let child_runs = Rc::new(Cell::new(0));
        let child_cleanups = Rc::new(Cell::new(0));
        let parent = {
            let fail = fail.clone();
            let child_runs = child_runs.clone();
            let child_cleanups = child_cleanups.clone();
            Effect::new(move || {
                let _ = rerun.get();
                let child_runs = child_runs.clone();
                let child_cleanups = child_cleanups.clone();
                Effect::new(move || {
                    let _ = leaf.get();
                    child_runs.set(child_runs.get() + 1);
                    let child_cleanups = child_cleanups.clone();
                    on_cleanup(move || child_cleanups.set(child_cleanups.get() + 1));
                });
                assert!(!fail.get(), "intentional parent panic");
            })
        };
        assert_eq!(child_runs.get(), 1);

        fail.set(true);
        let failed = catch_unwind(AssertUnwindSafe(|| rerun.set(1)));
        assert!(failed.is_err());
        assert_eq!(child_runs.get(), 2, "provisional child did not run");
        assert_eq!(child_cleanups.get(), 1, "provisional child was not cleaned");

        leaf.set(1);
        assert_eq!(
            child_runs.get(),
            3,
            "stable child was lost or provisional child survived rollback"
        );
        assert_eq!(child_cleanups.get(), 2);

        fail.set(false);
        rerun.set(2);
        assert_eq!(child_runs.get(), 4);
        assert_eq!(
            child_cleanups.get(),
            3,
            "stable child was not retired on commit"
        );
        parent.dispose();
        assert_eq!(child_cleanups.get(), 4);
        assert_runtime_invariants();
    }

    #[test]
    fn stale_handle_cannot_resolve_reused_slot() {
        reset_runtime_for_test();
        let stale = Signal::new(1);
        let stale_id = stale.id();
        stale.dispose();
        let current = Signal::new(2);
        let current_id = current.id();

        assert_eq!(stale_id.slot, current_id.slot, "test did not reuse a slot");
        assert_ne!(stale_id.generation, current_id.generation);
        assert_eq!(stale.try_get(), None);
        assert_eq!(current.try_get(), Some(2));
        assert_runtime_invariants();
    }

    #[test]
    fn failed_memo_keeps_last_stable_value_and_dependencies() {
        reset_runtime_for_test();
        let source = Signal::new(1);
        let fail = Rc::new(Cell::new(false));
        let memo = {
            let fail = fail.clone();
            Memo::new(move || {
                let value = source.get();
                assert!(!fail.get(), "intentional recomputation panic");
                value
            })
        };
        assert_eq!(memo.get(), 1);

        fail.set(true);
        source.set(2);
        let recompute = catch_unwind(AssertUnwindSafe(|| memo.get()));
        assert!(recompute.is_err());
        RT.with(|rt| {
            let rt = rt.borrow();
            let node = rt.node(memo.id).expect("memo disappeared after panic");
            assert_eq!(
                node.value
                    .as_ref()
                    .and_then(|value| value.downcast_ref::<i32>())
                    .copied(),
                Some(1)
            );
            assert_eq!(node.sources, vec![source.id()]);
            assert_eq!(node.color, Color::Dirty);
        });

        fail.set(false);
        assert_eq!(memo.get(), 2);
        assert_runtime_invariants();
    }

    #[test]
    fn failed_memo_rearms_effect_through_an_already_colored_chain() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let fail = Rc::new(Cell::new(false));
        let first = {
            let fail = fail.clone();
            Memo::new(move || {
                let value = source.get();
                assert!(!fail.get(), "intentional upstream memo panic");
                value
            })
        };
        let second = Memo::new(move || first.get() * 2);
        let runs = Rc::new(Cell::new(0));
        let seen = Rc::new(Cell::new(-1));
        let effect = {
            let runs = runs.clone();
            let seen = seen.clone();
            Effect::new(move || {
                runs.set(runs.get() + 1);
                seen.set(second.get());
            })
        };
        assert_eq!(runs.get(), 1);
        assert_eq!(seen.get(), 0);

        fail.set(true);
        assert!(catch_unwind(AssertUnwindSafe(|| source.set(1))).is_err());
        assert_eq!(runs.get(), 1, "effect body ran after its source failed");
        assert_eq!(seen.get(), 0, "failed value was published");

        fail.set(false);
        source.set(2);
        assert_eq!(runs.get(), 2, "effect was not rearmed after memo panic");
        assert_eq!(seen.get(), 4);

        effect.dispose();
        second.dispose();
        first.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn manual_memo_recovery_repairs_the_downstream_chain() {
        reset_runtime_for_test();
        let source = Signal::new(0);
        let fail = Rc::new(Cell::new(false));
        let first = {
            let fail = fail.clone();
            Memo::new(move || {
                let value = source.get();
                assert!(!fail.get(), "intentional manual recovery panic");
                value
            })
        };
        let second = Memo::new(move || first.get() * 2);
        let runs = Rc::new(Cell::new(0));
        let seen = Rc::new(Cell::new(-1));
        let effect = {
            let runs = runs.clone();
            let seen = seen.clone();
            Effect::new(move || {
                runs.set(runs.get() + 1);
                seen.set(second.get());
            })
        };

        fail.set(true);
        assert!(catch_unwind(AssertUnwindSafe(|| source.set(1))).is_err());
        fail.set(false);
        assert_eq!(first.get(), 1);
        assert_eq!(runs.get(), 2, "changed recovery did not notify effect");
        assert_eq!(seen.get(), 2);

        fail.set(true);
        assert!(catch_unwind(AssertUnwindSafe(|| source.set(1))).is_err());
        fail.set(false);
        assert_eq!(first.get(), 1);
        assert_eq!(runs.get(), 2, "equal recovery reran a clean effect");

        source.set(2);
        assert_eq!(runs.get(), 3, "equal recovery left downstream colored");
        assert_eq!(seen.get(), 4);

        effect.dispose();
        second.dispose();
        first.dispose();
        source.dispose();
        assert_runtime_invariants();
    }

    #[test]
    fn arena_reaches_plateau_after_ten_thousand_cycles() {
        reset_runtime_for_test();
        let baseline = runtime_stats();
        for value in 0..10_000 {
            let signal = Signal::new(value);
            let effect = Effect::new(move || {
                let _ = signal.get();
            });
            effect.dispose();
            signal.dispose();
        }

        let final_stats = runtime_stats();
        assert_eq!(baseline.slots_total, 0);
        assert_eq!(final_stats.slots_live, 0);
        assert_eq!(
            final_stats.slots_total, 2,
            "arena grew instead of reusing slots"
        );
        assert_eq!(final_stats.slots_free, 2);
        assert_eq!(final_stats.pending, 0);
        assert_eq!(final_stats.deferred_disposals, 0);
        assert_eq!(final_stats.owned_edges, 0);
        assert_eq!(final_stats.source_edges, 0);
        assert_eq!(final_stats.subscriber_edges, 0);
        assert_eq!(final_stats.cleanup_count, 0);
        assert!(!final_stats.observer_present);
        assert!(!final_stats.owner_present);
        assert!(!final_stats.flushing);
        assert_runtime_invariants();
    }

    #[test]
    fn deep_ownership_disposal_uses_an_explicit_stack() {
        reset_runtime_for_test();
        let handles: Vec<_> = (0..20_000).map(Signal::new).collect();
        RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            for pair in handles.windows(2) {
                let parent = pair[0].id();
                let child = pair[1].id();
                rt.node_mut(parent)
                    .expect("parent missing")
                    .owned
                    .push(child);
                rt.node_mut(child).expect("child missing").owner = Some(parent);
            }
        });

        handles[0].dispose();
        let stats = runtime_stats();
        assert_eq!(stats.slots_live, 0);
        assert_eq!(stats.slots_free, 20_000);
        assert_runtime_invariants();
    }
}
