// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! pliego-fold — the fold as a first-class reactive node (M3, docs/00 §3.1).
//!
//! This is the marriage of M1 and M2: the append-only log enters the reactive
//! graph, and projections update themselves.
//!
//! The construction is deliberately simple: **a `Fold` is a `Memo` whose closure
//! captures an accumulator and a cursor.** The memo machinery provides tracking,
//! lazy pull, and the equality gate; the captured accumulator makes the recompute
//! *incremental* — on wake it folds only `log[cursor..]`, O(new events), never
//! O(log). This is the piece the Leptos study identified as the one thing the
//! existing machinery doesn't have (spec §3.1), and it is the same
//! accumulator+cursor+snapshot design as Hyphae's projections (BACKEND-6).
//!
//! The loop, end to end:
//!
//! ```text
//! interaction → log.append(event)          the ONLY write
//!             → log signal marks the graph  (two-phase coloring, cheap)
//!             → a read pulls the fold        (lazy)
//!             → fold consumes the TAIL       (incremental)
//!             → equal state? subscribers sleep (equality gate)
//!             → else render effects patch the DOM
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use pliego_log::{Event, Log};
use pliego_reactive::{Memo, Signal};

/// The log as a reactive root: PliegoRS's single writable thing, in the graph.
#[derive(Clone, Copy)]
pub struct ReactiveLog {
    inner: Signal<Log>,
}

impl ReactiveLog {
    /// A new, empty reactive log.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Signal::new(Log::new()),
        }
    }

    /// Wrap an existing log (e.g. rebuilt from a snapshot or synced from Hyphae).
    #[must_use]
    pub fn from_log(log: Log) -> Self {
        Self {
            inner: Signal::new(log),
        }
    }

    /// Append an event — the ONLY write in the framework. Marks the graph and
    /// flushes effects (folds wake lazily and consume just the tail).
    pub fn append(&self, kind: impl Into<String>, payload: impl Into<String>) {
        let (kind, payload) = (kind.into(), payload.into());
        self.inner.update(move |log| {
            log.append(kind, payload);
        });
    }

    /// Tracked read of the whole log by reference (what folds use).
    pub fn with<R>(&self, f: impl FnOnce(&Log) -> R) -> R {
        self.inner.with(f)
    }

    /// Number of events (tracked — a view of "how long is history" is reactive).
    #[must_use]
    pub fn len(&self) -> u64 {
        self.inner.with(Log::len)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ReactiveLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal accumulator: the folded state so far + how far into the log it read.
struct Acc<S> {
    state: S,
    cursor: u64,
    /// Total reducer invocations (observability + the O(tail) gate).
    folded: u64,
}

/// The incremental fold node: a projection of the log, live in the graph.
///
/// Reading it (`get`) inside an effect subscribes; appending to the log wakes it
/// lazily; waking folds only the tail; folding to an equal state wakes nobody
/// downstream.
pub struct Fold<S: 'static> {
    memo: Memo<S>,
    acc: Rc<RefCell<Acc<S>>>,
}

impl<S: Clone + PartialEq + 'static> Fold<S> {
    /// A fold over `log` starting from `initial` at genesis.
    pub fn new(log: ReactiveLog, initial: S, reducer: impl Fn(&mut S, &Event) + 'static) -> Self {
        Self::from_snapshot(log, initial, 0, reducer)
    }

    /// Resume a fold from a snapshot: `state` is the fold of `log[..cursor]`,
    /// so a cold start folds only the tail beyond it — Hyphae's BACKEND-6
    /// trick as a client primitive (spec §3.1).
    pub fn from_snapshot(
        log: ReactiveLog,
        state: S,
        cursor: u64,
        reducer: impl Fn(&mut S, &Event) + 'static,
    ) -> Self {
        let acc = Rc::new(RefCell::new(Acc {
            state,
            cursor,
            folded: 0,
        }));
        let memo_acc = acc.clone();
        let memo = Memo::new(move || {
            log.with(|log| {
                let mut acc = memo_acc.borrow_mut();
                let Acc {
                    state,
                    cursor,
                    folded,
                } = &mut *acc;
                for e in log.tail(*cursor) {
                    reducer(state, e);
                    *folded += 1;
                }
                *cursor = log.len();
                state.clone()
            })
        });
        Fold { memo, acc }
    }

    /// The folded state (tracked read; settles the node first — lazy pull).
    #[must_use]
    pub fn get(&self) -> S {
        self.memo.get()
    }

    /// Current `(state, cursor)` — persist it and resume later with
    /// [`Fold::from_snapshot`]. Settles first so the snapshot is current.
    #[must_use]
    pub fn snapshot(&self) -> (S, u64) {
        let state = self.memo.get();
        (state, self.acc.borrow().cursor)
    }

    /// How many events this fold has ever consumed (the O(tail) observability).
    #[must_use]
    pub fn events_folded(&self) -> u64 {
        self.acc.borrow().folded
    }

    /// Stop this fold (disposal discipline from pliego-reactive).
    pub fn dispose(&self) {
        self.memo.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_reactive::Effect;
    use std::cell::Cell;

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct Tasks {
        titles: Vec<String>,
    }

    fn reduce(state: &mut Tasks, e: &Event) {
        match e.kind.as_str() {
            "add" => state.titles.push(e.payload.clone()),
            "remove_last" => {
                state.titles.pop();
            }
            _ => {} // unknown events do not change the projection
        }
    }

    /// THE M3 GATE 1: the reactive loop end to end — append wakes the fold,
    /// the effect re-renders with the new state.
    #[test]
    fn gate_append_drives_the_effect() {
        let log = ReactiveLog::new();
        let fold = Rc::new(Fold::new(log, Tasks::default(), reduce));
        let runs = Rc::new(Cell::new(0));
        let last_len = Rc::new(Cell::new(0));
        {
            let (fold, runs, last_len) = (fold.clone(), runs.clone(), last_len.clone());
            Effect::new(move || {
                runs.set(runs.get() + 1);
                last_len.set(fold.get().titles.len());
            });
        }
        assert_eq!((runs.get(), last_len.get()), (1, 0));
        log.append("add", "fold the log");
        assert_eq!((runs.get(), last_len.get()), (2, 1));
        log.append("add", "into interface");
        assert_eq!((runs.get(), last_len.get()), (3, 2));
    }

    /// THE M3 GATE 2: incrementality survives reactivity — N appends fold each
    /// event exactly once, never from genesis.
    #[test]
    fn gate_incremental_under_reactivity() {
        let log = ReactiveLog::new();
        let fold = Rc::new(Fold::new(log, Tasks::default(), reduce));
        {
            let fold = fold.clone();
            Effect::new(move || {
                let _ = fold.get();
            });
        }
        for i in 0..100 {
            log.append("add", format!("t{i}"));
        }
        assert_eq!(fold.get().titles.len(), 100);
        assert_eq!(
            fold.events_folded(),
            100,
            "each event must be folded exactly once (O(tail), not O(log))"
        );
    }

    /// THE M3 GATE 3: the equality gate reaches through the fold — an event
    /// that doesn't change the projection wakes nobody downstream.
    #[test]
    fn gate_unchanged_projection_wakes_nobody() {
        let log = ReactiveLog::new();
        let fold = Rc::new(Fold::new(log, Tasks::default(), reduce));
        let runs = Rc::new(Cell::new(0));
        {
            let (fold, runs) = (fold.clone(), runs.clone());
            Effect::new(move || {
                runs.set(runs.get() + 1);
                let _ = fold.get();
            });
        }
        assert_eq!(runs.get(), 1);
        log.append("annotate", "irrelevant to this projection"); // unknown kind
        assert_eq!(
            runs.get(),
            1,
            "an event that folds to an equal state must not wake the effect"
        );
        log.append("add", "real change");
        assert_eq!(runs.get(), 2);
    }

    /// THE M3 GATE 4 (the spec's M3 gate): cold start from a snapshot folds
    /// only the tail.
    #[test]
    fn gate_snapshot_cold_start_folds_only_the_tail() {
        // session 1: build a log, fold it, snapshot
        let mut raw = Log::new();
        for i in 0..500 {
            raw.append("add", format!("t{i}"));
        }
        let log = ReactiveLog::from_log(raw.clone());
        let fold = Fold::new(log, Tasks::default(), reduce);
        let (state, cursor) = fold.snapshot();
        assert_eq!(cursor, 500);

        // session 2 (cold start): same log + 3 new events, resume from snapshot
        for i in 500..503 {
            raw.append("add", format!("t{i}"));
        }
        let log2 = ReactiveLog::from_log(raw);
        let resumed = Fold::from_snapshot(log2, state, cursor, reduce);
        let s = resumed.get();
        assert_eq!(s.titles.len(), 503);
        assert_eq!(
            resumed.events_folded(),
            3,
            "cold start must fold ONLY the tail beyond the snapshot"
        );
    }

    /// The forest: two independent folds over ONE log, each with its own cursor,
    /// both correct — Hyphae's one-log/many-projections shape, client-side.
    #[test]
    fn two_folds_one_log() {
        let log = ReactiveLog::new();
        let tasks = Fold::new(log, Tasks::default(), reduce);
        // a second projection: just a count of ALL events (audit-style)
        let count = Fold::new(log, 0u64, |n: &mut u64, _e| *n += 1);

        log.append("add", "a");
        log.append("annotate", "x");
        log.append("add", "b");

        assert_eq!(tasks.get().titles, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(count.get(), 3);
        assert_eq!(tasks.events_folded(), 3);
        assert_eq!(count.events_folded(), 3);
    }
}
