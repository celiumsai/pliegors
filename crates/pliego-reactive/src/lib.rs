// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

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
use std::marker::PhantomData;
use std::rc::Rc;

// ───────────────────────────── runtime plumbing ─────────────────────────────

/// Index of a node in the runtime arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Color {
    /// Value is current.
    Clean,
    /// A transitive source changed; whether *my* inputs changed is unknown.
    Check,
    /// A direct source changed; recompute before next read.
    Dirty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Signal,
    Memo,
    Effect,
}

type Value = Rc<dyn Any>;
type Compute = Rc<dyn Fn() -> Value>;
type EqFn = Rc<dyn Fn(&dyn Any, &dyn Any) -> bool>;

struct Node {
    kind: Kind,
    color: Color,
    disposed: bool,
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
    /// Cleanup callbacks, run before re-run/disposal.
    cleanups: Vec<Box<dyn FnOnce()>>,
}

#[derive(Default)]
struct Runtime {
    nodes: Vec<Node>,
    /// The computation currently running (tracking target), if any.
    observer: Option<NodeId>,
    /// Effects queued by the current write, run at the end of it.
    pending: Vec<NodeId>,
    /// Re-entrancy guard for effect flushing.
    flushing: bool,
}

thread_local! {
    static RT: RefCell<Runtime> = RefCell::new(Runtime::default());
}

impl Runtime {
    fn add_node(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        // computations created inside a running computation are owned by it
        if let Some(owner) = self.observer {
            self.nodes[owner.0].owned.push(id);
        }
        id
    }
}

/// Establish `source → observer` edges if a computation is running.
fn track(source: NodeId) {
    RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let Some(obs) = rt.observer else { return };
        if obs == source || rt.nodes[source.0].disposed {
            return;
        }
        if !rt.nodes[source.0].subscribers.contains(&obs) {
            rt.nodes[source.0].subscribers.push(obs);
        }
        if !rt.nodes[obs.0].sources.contains(&source) {
            rt.nodes[obs.0].sources.push(source);
        }
    });
}

/// Phase 1 (push, cheap): direct subscribers of a changed node become Dirty,
/// deeper descendants become (at least) Check. Effects encountered are queued.
fn mark_subscribers(rt: &mut Runtime, of: NodeId, color: Color) {
    let subs = rt.nodes[of.0].subscribers.clone();
    for s in subs {
        let node = &mut rt.nodes[s.0];
        if node.disposed || node.color >= color {
            continue; // already at least this stale — its subtree already marked
        }
        node.color = color;
        if node.kind == Kind::Effect {
            if !rt.pending.contains(&s) {
                rt.pending.push(s);
            }
        } else {
            mark_subscribers(rt, s, Color::Check);
        }
    }
}

/// Phase 2 (pull, lazy): make `id` current, recomputing only what truly changed.
fn update_if_necessary(id: NodeId) {
    let (color, kind) = RT.with(|rt| {
        let rt = rt.borrow();
        (rt.nodes[id.0].color, rt.nodes[id.0].kind)
    });
    if color == Color::Clean || kind == Kind::Signal {
        return;
    }

    if color == Color::Check {
        // ask each source to settle; a source that recomputes-and-changes will
        // promote us to Dirty (see run_computation's notification)
        let sources = RT.with(|rt| rt.borrow().nodes[id.0].sources.clone());
        for s in sources {
            update_if_necessary(s);
            let now = RT.with(|rt| rt.borrow().nodes[id.0].color);
            if now == Color::Dirty {
                break;
            }
        }
    }

    let now = RT.with(|rt| rt.borrow().nodes[id.0].color);
    if now == Color::Dirty {
        run_computation(id);
    } else {
        RT.with(|rt| rt.borrow_mut().nodes[id.0].color = Color::Clean);
    }
}

/// Re-run a memo's/effect's computation with tracking, dispose what the previous
/// run owned, apply the equality gate, and notify subscribers on real change.
fn run_computation(id: NodeId) {
    // detach from old sources + dispose owned children; collect cleanups
    let cleanups = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let sources = std::mem::take(&mut rt.nodes[id.0].sources);
        for s in sources {
            rt.nodes[s.0].subscribers.retain(|x| *x != id);
        }
        let owned = std::mem::take(&mut rt.nodes[id.0].owned);
        for o in owned {
            dispose_inner(&mut rt, o);
        }
        std::mem::take(&mut rt.nodes[id.0].cleanups)
    });
    // run user cleanups with NO runtime borrow held (they may touch the graph)
    for c in cleanups {
        c();
    }
    let (compute, prev_value, eq) = RT.with(|rt| {
        let rt = rt.borrow();
        (
            rt.nodes[id.0].compute.clone().expect("computation node"),
            rt.nodes[id.0].value.clone(),
            rt.nodes[id.0].eq.clone(),
        )
    });

    // run tracked: reads inside re-register sources
    let prev_obs = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let prev = rt.observer;
        rt.observer = Some(id);
        prev
    });
    let new_value = compute();
    RT.with(|rt| rt.borrow_mut().observer = prev_obs);

    // equality gate: unchanged value → subscribers stay unwoken
    let changed = match (&prev_value, &eq) {
        (Some(prev), Some(eq)) => !eq(prev.as_ref(), new_value.as_ref()),
        _ => true, // first run, or no comparator
    };

    RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let is_memo = rt.nodes[id.0].kind == Kind::Memo;
        if is_memo {
            rt.nodes[id.0].value = Some(new_value);
        }
        rt.nodes[id.0].color = Color::Clean;
        if changed && is_memo {
            // direct subscribers must recompute: promote to Dirty
            let subs = rt.nodes[id.0].subscribers.clone();
            for s in subs {
                if !rt.nodes[s.0].disposed {
                    rt.nodes[s.0].color = Color::Dirty;
                    if rt.nodes[s.0].kind == Kind::Effect && !rt.pending.contains(&s) {
                        rt.pending.push(s);
                    }
                }
            }
        }
    });
}

/// Run queued effects until quiescent. Called at the end of every write.
fn flush_effects() {
    let already = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        if rt.flushing {
            return true;
        }
        rt.flushing = true;
        false
    });
    if already {
        return; // the outer flush will drain everything
    }
    while let Some(next) = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        if rt.pending.is_empty() {
            None
        } else {
            Some(rt.pending.remove(0))
        }
    }) {
        let disposed = RT.with(|rt| rt.borrow().nodes[next.0].disposed);
        if !disposed {
            update_if_necessary(next);
        }
    }
    RT.with(|rt| rt.borrow_mut().flushing = false);
}

fn dispose_inner(rt: &mut Runtime, id: NodeId) {
    if rt.nodes[id.0].disposed {
        return;
    }
    rt.nodes[id.0].disposed = true;
    let sources = std::mem::take(&mut rt.nodes[id.0].sources);
    for s in sources {
        rt.nodes[s.0].subscribers.retain(|x| *x != id);
    }
    let subs = std::mem::take(&mut rt.nodes[id.0].subscribers);
    for s in subs {
        rt.nodes[s.0].sources.retain(|x| *x != id);
    }
    let owned = std::mem::take(&mut rt.nodes[id.0].owned);
    for o in owned {
        dispose_inner(rt, o);
    }
    // cleanups run outside (dispose() handles the borrow dance)
}

// ───────────────────────────── public API ─────────────────────────────

/// A reactive, writable value — a root of the graph.
///
/// In PliegoRS discipline there is ultimately ONE meaningful root (the log);
/// `Signal` exists as the general primitive the fold node builds on (M3).
pub struct Signal<T> {
    id: NodeId,
    _t: PhantomData<fn() -> T>,
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
        let id = RT.with(|rt| {
            rt.borrow_mut().add_node(Node {
                kind: Kind::Signal,
                color: Color::Clean,
                disposed: false,
                value: Some(Rc::new(value)),
                compute: None,
                eq: None,
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            })
        });
        Signal {
            id,
            _t: PhantomData,
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
        RT.with(|rt| {
            let rt = rt.borrow();
            let v = rt.nodes[self.id.0].value.as_ref().expect("signal value");
            v.downcast_ref::<T>().expect("signal type").clone()
        })
    }

    /// Read by reference (tracked), without cloning `T` — the path large values
    /// (like the log) take. The value's `Rc` is cloned cheaply and the runtime
    /// borrow released before `f` runs, so `f` may freely read other reactives.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        track(self.id);
        let value: Value = RT.with(|rt| {
            rt.borrow().nodes[self.id.0]
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
        RT.with(|rt| {
            let rt = rt.borrow();
            let node = rt.nodes.get(self.id.0)?;
            if node.disposed {
                return None;
            }
            node.value.as_ref()?.downcast_ref::<T>().cloned()
        })
    }

    /// Replace the value and notify: subscribers marked, effects flushed.
    pub fn set(&self, value: T) {
        self.update(|v| *v = value);
    }

    /// Mutate in place and notify. (In PliegoRS, the log's `append` is exactly
    /// this — `update(|log| log.append(event))` — the fold node builds on it.)
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        RT.with(|rt| {
            let mut rt = rt.borrow_mut();
            if rt.nodes[self.id.0].disposed {
                return;
            }
            let value = rt.nodes[self.id.0].value.take().expect("signal value");
            // sole-owner fast path or clone-on-write for shared Rc
            let mut inner: T = match Rc::try_unwrap(value.downcast::<T>().expect("signal type")) {
                Ok(v) => v,
                Err(rc) => panic!(
                    "signal value aliased during update ({} refs)",
                    Rc::strong_count(&rc)
                ),
            };
            f(&mut inner);
            rt.nodes[self.id.0].value = Some(Rc::new(inner));
            mark_subscribers(&mut rt, self.id, Color::Dirty);
        });
        flush_effects();
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
        let compute: Compute = Rc::new(move || Rc::new(f()) as Value);
        let eq: EqFn = Rc::new(
            |a, b| match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        );
        let id = RT.with(|rt| {
            rt.borrow_mut().add_node(Node {
                kind: Kind::Memo,
                color: Color::Dirty, // never ran
                disposed: false,
                value: None,
                compute: Some(compute),
                eq: Some(eq),
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            })
        });
        Memo {
            id,
            _t: PhantomData,
        }
    }

    /// Read (tracked). Settles the node first (lazy pull).
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        update_if_necessary(self.id);
        track(self.id);
        RT.with(|rt| {
            let rt = rt.borrow();
            let v = rt.nodes[self.id.0].value.as_ref().expect("memo ran");
            v.downcast_ref::<T>().expect("memo type").clone()
        })
    }

    pub fn dispose(&self) {
        dispose(self.id);
    }
}

/// A terminal reaction — in PliegoRS, almost always a render effect.
pub struct Effect {
    id: NodeId,
}

impl Effect {
    /// Create and run once now; re-runs whenever tracked dependencies change.
    pub fn new(f: impl Fn() + 'static) -> Self {
        let compute: Compute = Rc::new(move || {
            f();
            Rc::new(()) as Value
        });
        let id = RT.with(|rt| {
            rt.borrow_mut().add_node(Node {
                kind: Kind::Effect,
                color: Color::Clean,
                disposed: false,
                value: None,
                compute: Some(compute),
                eq: None,
                sources: Vec::new(),
                subscribers: Vec::new(),
                owned: Vec::new(),
                cleanups: Vec::new(),
            })
        });
        run_computation(id);
        Effect { id }
    }

    /// Stop this effect: cleanups run, edges removed, never fires again.
    pub fn dispose(&self) {
        dispose(self.id);
    }
}

/// Register a cleanup on the currently-running computation (runs before its
/// next re-run, or on disposal).
pub fn on_cleanup(f: impl FnOnce() + 'static) {
    RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        if let Some(obs) = rt.observer {
            rt.nodes[obs.0].cleanups.push(Box::new(f));
        }
    });
}

/// Run `f` without tracking (reads inside register no dependencies).
pub fn untrack<T>(f: impl FnOnce() -> T) -> T {
    let prev = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let prev = rt.observer;
        rt.observer = None;
        prev
    });
    let out = f();
    RT.with(|rt| rt.borrow_mut().observer = prev);
    out
}

fn dispose(id: NodeId) {
    let cleanups = RT.with(|rt| {
        let mut rt = rt.borrow_mut();
        let cleanups = std::mem::take(&mut rt.nodes[id.0].cleanups);
        dispose_inner(&mut rt, id);
        cleanups
    });
    for c in cleanups {
        c();
    }
}

// ───────────────────────────── tests (the M2 gate) ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// THE M2 GATE 1: the diamond. One source feeding two memos feeding one
    /// effect → a write runs the effect exactly ONCE (glitch-free).
    #[test]
    fn gate_diamond_runs_effect_once() {
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
    }

    /// THE M2 GATE 2: dynamic dependencies — the branch not taken is not
    /// tracked; writes to it cause no recompute.
    #[test]
    fn gate_untaken_branch_not_tracked() {
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
    }

    /// THE M2 GATE 3: equality gating — a memo recomputing to an equal value
    /// does not wake its subscribers (x*0 stays 0; the effect must not re-run).
    #[test]
    fn gate_equality_gate_stops_propagation() {
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
    }

    /// THE M2 GATE 4: disposal is clean — cleanups run, effects stop firing,
    /// disposed signals read as None.
    #[test]
    fn gate_disposal_is_clean() {
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
    }

    /// Lazy pull: a memo nobody reads doesn't recompute on writes.
    #[test]
    fn memo_is_lazy() {
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
    }

    /// untrack() suppresses subscription.
    #[test]
    fn untrack_reads_do_not_subscribe() {
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
    }

    /// Chained memos propagate through, still one effect run per write.
    #[test]
    fn chains_propagate_once() {
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
    }
}
