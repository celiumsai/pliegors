// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! M1 spike — the PliegoRS thesis on one screen (docs/00 §6).
//!
//! A task list where **the UI is a fold of an event log**:
//! - Clicking never mutates state; it `append`s an event (`task_added`,
//!   `task_toggled`, `task_removed`) to a hash-chained [`pliego_log::Log`].
//! - The list on screen is [`TaskList`], the incremental fold of that log.
//! - **Undo/time-travel** is a cursor: replaying a prefix rebuilds "the world as
//!   of event N" — no undo stack, the log *is* the undo stack.
//! - **Provenance**: each task's id IS the seq of the event that created it;
//!   the UI shows the event's hash. Values trace to their origin.
//!
//! The reducer + gate tests are native (run `cargo test -p spike`); the DOM code
//! compiles only for wasm32. Rendering here is deliberately hand-rolled — the
//! surgical renderer is M4; M1 proves the *model*.

use pliego_log::Event;

// ───────────────────────── the state and its reducer ─────────────────────────

/// One task. `id` is the seq of the `task_added` event — identity *is*
/// provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// The folded state of the task list.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TaskList {
    pub items: Vec<Task>,
}

/// The pure reducer: one event, one state transition. Deterministic by
/// construction — no clock, no randomness, no I/O (docs/00 §3.3).
pub fn reduce(state: &mut TaskList, e: &Event) {
    match e.kind.as_str() {
        "task_added" => state.items.push(Task {
            id: e.seq,
            text: e.payload.clone(),
            done: false,
        }),
        "task_toggled" => {
            if let Ok(id) = e.payload.parse::<u64>() {
                if let Some(t) = state.items.iter_mut().find(|t| t.id == id) {
                    t.done = !t.done;
                }
            }
        }
        "task_removed" => {
            if let Ok(id) = e.payload.parse::<u64>() {
                state.items.retain(|t| t.id != id);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_log::{Fold, Log};

    fn demo_log() -> Log {
        let mut log = Log::new();
        log.append("task_added", "buy milk"); // seq 0
        log.append("task_added", "call Ana"); // seq 1
        log.append("task_toggled", "0");
        log.append("task_added", "pay power bill"); // seq 3
        log.append("task_removed", "1");
        log
    }

    #[test]
    fn reducer_builds_the_expected_state() {
        let mut fold = Fold::new(TaskList::default(), reduce);
        let log = demo_log();
        fold.sync(&log);
        let s = fold.state();
        assert_eq!(s.items.len(), 2);
        assert_eq!(
            s.items[0],
            Task {
                id: 0,
                text: "buy milk".into(),
                done: true
            }
        );
        assert_eq!(
            s.items[1],
            Task {
                id: 3,
                text: "pay power bill".into(),
                done: false
            }
        );
    }

    /// THE M1 GATE on the real app state: live fold == replay, at every prefix.
    #[test]
    fn gate_replay_equals_live_on_tasklist() {
        let mut log = Log::new();
        let mut live = Fold::new(TaskList::default(), reduce);
        let script: &[(&str, String)] = &[
            ("task_added", "a".into()),
            ("task_added", "b".into()),
            ("task_toggled", "0".into()),
            ("task_added", "c".into()),
            ("task_removed", "1".into()),
            ("task_toggled", "0".into()),
            ("task_toggled", "3".into()),
            ("task_removed", "0".into()),
        ];
        for (i, (kind, payload)) in script.iter().enumerate() {
            log.append(*kind, payload.clone());
            live.sync(&log);
            let replayed: TaskList = Fold::replay(&log, log.len(), reduce);
            assert_eq!(live.state(), &replayed, "diverged after step {i}");
        }
        assert!(log.verify().is_ok());
    }

    /// Undo is a prefix: the world as of event N.
    #[test]
    fn undo_via_cursor() {
        let log = demo_log();
        // before the removal (first 4 events): milk(done) + call Ana + power bill
        let at4: TaskList = Fold::replay(&log, 4, reduce);
        assert_eq!(at4.items.len(), 3);
        // as of event 2: both tasks, milk toggled
        let at3: TaskList = Fold::replay(&log, 3, reduce);
        assert_eq!(at3.items.len(), 2);
        assert!(at3.items[0].done);
    }
}

// ───────────────────────── the browser shell (wasm32 only) ─────────────────────────
//
// M4 rewrite: the ENTIRE page is now built by the framework itself —
// ReactiveLog (pliego-fold) + Fold (the incremental node) + el()/dyn_text/
// dyn_view (pliego-dom). No hand-rolled innerHTML, no manual render calls:
// appends wake the folds, the folds wake exactly the views that read them.

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{TaskList, reduce};
    use pliego_dom::{IntoView, View, dyn_text, dyn_view, el, mount_to};
    use pliego_fold::{Fold, ReactiveLog};
    use pliego_hyphae::Ack;
    use pliego_log::hex;
    use pliego_reactive::Signal;
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    /// Where the Hyphae engine lives (the SSH tunnel to the fleet in dev).
    const HYPHAE_BASE: &str = "http://127.0.0.1:18091";

    /// Push the log's tail to Hyphae in the background; the UI does not await
    /// this. Each ack lands in the `synced` signal, waking exactly the status
    /// chips that read it.
    fn sync_now(
        log: ReactiveLog,
        synced: Signal<HashMap<u64, Ack>>,
        next_push: Rc<Cell<u64>>,
        pushing: Rc<Cell<bool>>,
    ) {
        if pushing.get() {
            return;
        }
        pushing.set(true);
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let seq = next_push.get();
                let Some((kind, payload)) =
                    log.with(|l| l.get(seq).map(|e| (e.kind.clone(), e.payload.clone())))
                else {
                    break; // fully synced
                };
                match pliego_hyphae::fetch::append_remote(HYPHAE_BASE, &kind, &payload).await {
                    Ok(ack) => {
                        synced.update(|m| {
                            m.insert(seq, ack);
                        });
                        next_push.set(seq + 1);
                    }
                    Err(e) => {
                        web_sys::console::warn_1(
                            &format!("hyphae sync parked at #{seq}: {e}").into(),
                        );
                        break; // retry on the next append
                    }
                }
            }
            pushing.set(false);
        });
    }

    fn input_by_id(id: &str) -> web_sys::HtmlInputElement {
        web_sys::window()
            .expect("window")
            .document()
            .expect("document")
            .get_element_by_id(id)
            .expect("input by id")
            .dyn_into()
            .expect("an <input>")
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        // the whole app model: ONE log, ONE live fold, one view cursor — plus the
        // sync map (local seq -> durable ack). All Copy/cheap handles.
        let log = ReactiveLog::new();
        let live = Rc::new(Fold::new(log, TaskList::default(), reduce));
        let view_at: Signal<Option<u64>> = Signal::new(None);
        let synced: Signal<HashMap<u64, Ack>> = Signal::new(HashMap::new());
        let next_push = Rc::new(Cell::new(0u64));
        let pushing = Rc::new(Cell::new(false));
        let kick_sync = {
            let (next_push, pushing) = (next_push.clone(), pushing.clone());
            move || sync_now(log, synced, next_push.clone(), pushing.clone())
        };

        // current state: live fold, or a prefix replay when time-traveling
        let current = {
            let live = live.clone();
            move || -> (TaskList, bool) {
                match view_at.get() {
                    Some(n) if n < log.len() => {
                        (log.with(|l| pliego_log::Fold::replay(l, n, reduce)), true)
                    }
                    _ => (live.get(), false),
                }
            }
        };

        // ── the view: data all the way down ──
        let header = el("div")
            .class("row")
            .child(
                el("input")
                    .id("new-task")
                    .attr("type", "text")
                    .attr("placeholder", "new task → append('task_added', …)"),
            )
            .child(el("button").child("append").on("click", {
                let kick = kick_sync.clone();
                move |_| {
                    let input = input_by_id("new-task");
                    let text = input.value();
                    if !text.trim().is_empty() {
                        input.set_value("");
                        log.append("task_added", text.trim().to_string());
                        view_at.set(None);
                        kick();
                    }
                }
            }));

        let list = {
            let current = current.clone();
            let kick = kick_sync.clone();
            dyn_view(move || {
                let (state, _) = current();
                let mut items: Vec<View> = Vec::new();
                for t in &state.items {
                    let id = t.id;
                    let h = log.with(|l| hex(&l.get(id).expect("origin event").hash));
                    let kick_t = kick.clone();
                    let kick_r = kick.clone();
                    items.push(
                        el("li")
                            .class(if t.done { "done" } else { "" })
                            .child(el("button").child(if t.done { "☑" } else { "☐" }).on(
                                "click",
                                move |_| {
                                    log.append("task_toggled", id.to_string());
                                    view_at.set(None);
                                    kick_t();
                                },
                            ))
                            .child(el("span").class("text").child(t.text.clone()))
                            .child(el("button").child("×").on("click", move |_| {
                                log.append("task_removed", id.to_string());
                                view_at.set(None);
                                kick_r();
                            }))
                            .child(
                                el("code")
                                    .class("prov")
                                    .attr("title", format!("event #{id} · local {h}"))
                                    .child(format!("#{id} · {}…", &h[..12]))
                                    // dual provenance: the durable ack patches in
                                    // surgically when Hyphae confirms THIS event
                                    .child(dyn_text(move || {
                                        synced.with(|m| match m.get(&id) {
                                            Some(a) => {
                                                format!(" ⛓ hyphae #{} {}…", a.seq, &a.hash[..8])
                                            }
                                            None => " ⛓ …".to_string(),
                                        })
                                    })),
                            )
                            .into_view(),
                    );
                }
                el("ul").id("tasks").child(items).into_view()
            })
        };

        let slider = el("div")
            .class("travel")
            .child(
                el("label")
                    .attr("for", "cursor")
                    .child("time-travel cursor — the world as of event N (right edge = live)"),
            )
            .child(
                el("input")
                    .id("cursor")
                    .attr("type", "range")
                    .attr("min", "0")
                    .attr("step", "1")
                    .attr_dyn("max", move || log.len().to_string())
                    .attr_dyn("value", move || match view_at.get() {
                        Some(n) => n.to_string(),
                        None => log.len().to_string(),
                    })
                    .on("input", move |ev| {
                        let Some(target) = ev.target() else { return };
                        let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
                            return;
                        };
                        let n: u64 = input.value().parse().unwrap_or(0);
                        // len untracked here (listeners run outside computations)
                        view_at.set(if n >= log.len() { None } else { Some(n) });
                    }),
            );

        let status = el("div").id("status").child(dyn_text(move || {
            let len = log.len();
            let (head, ok) = log.with(|l| (hex(&l.head()), l.verify().is_ok()));
            let confirmed = synced.with(HashMap::len) as u64;
            let mode = match view_at.get() {
                Some(n) if n < len => format!("⏪ viewing as of event {n}"),
                _ => "live".to_string(),
            };
            format!(
                "{len} events · head {}… · chain {} · ⛓ hyphae {confirmed}/{len} durable · {mode}",
                &head[..12],
                if ok { "✓ intact" } else { "✗ BROKEN" },
            )
        }));

        let app = el("div")
            .child(header)
            .child(list)
            .child(slider)
            .child(status)
            .into_view();
        mount_to("app", &app);

        // seed so the screen speaks on load, then start the durable sync
        log.append(
            "task_added",
            "estudiar Leptos (hecho: se observa, no se copia)",
        );
        log.append("task_added", "plegar el log en interfaz");
        log.append("task_toggled", "0");
        kick_sync();
    }
}
