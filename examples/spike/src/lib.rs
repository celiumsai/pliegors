// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! A typed task-list spike for PliegoRS.
//!
//! Every interaction appends an [`EventSchema`] payload. A sealed catalog
//! resolves stored versions into [`TaskEvent`], and one transactional
//! [`Projection`] materializes [`TaskList`]. Time travel rebuilds an exact
//! prefix through the same typed append and projection APIs used by the live
//! application. No free-form event or state/cursor tuple bypass exists.

use pliego_fold::{
    CanonicalJsonCodec, Projection, ProjectionError, ReactiveLog, Reducer, ReducerError,
    ReducerIdentity,
};
use pliego_log::{EventCatalogBuilder, EventSchema, SealedEventCatalog};
use serde::{Deserialize, Serialize};

#[cfg(any(test, target_arch = "wasm32"))]
use pliego_log::{Log, LogError};

/// One task. Its identifier is chosen from the next local sequence before the
/// `TaskAdded` event is appended, preserving event-level provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// The materialized task-list state.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    pub items: Vec<Task>,
    events_seen: u64,
}

/// Version 1 payload for creating a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAddedV1 {
    pub id: u64,
    pub text: String,
}

impl EventSchema for TaskAddedV1 {
    const KIND: &'static str = "app_task_added";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "pliego.example/task-added/1";
}

/// Version 1 payload for toggling a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskToggledV1 {
    pub id: u64,
}

impl EventSchema for TaskToggledV1 {
    const KIND: &'static str = "app_task_toggled";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "pliego.example/task-toggled/1";
}

/// Version 1 payload for removing a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRemovedV1 {
    pub id: u64,
}

impl EventSchema for TaskRemovedV1 {
    const KIND: &'static str = "app_task_removed";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "pliego.example/task-removed/1";
}

/// Reducer-facing event set produced only by the sealed schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TaskEvent {
    Added(TaskAddedV1),
    Toggled(TaskToggledV1),
    Removed(TaskRemovedV1),
}

impl TaskEvent {
    #[cfg(any(test, target_arch = "wasm32"))]
    fn append_to_log(&self, log: &mut Log) -> Result<(), LogError> {
        match self {
            Self::Added(event) => log.append_typed(event).map(|_| ()),
            Self::Toggled(event) => log.append_typed(event).map(|_| ()),
            Self::Removed(event) => log.append_typed(event).map(|_| ()),
        }
    }

    #[cfg(test)]
    fn append_to_reactive(&self, log: ReactiveLog) -> Result<(), LogError> {
        match self {
            Self::Added(event) => log.append_typed(event),
            Self::Toggled(event) => log.append_typed(event),
            Self::Removed(event) => log.append_typed(event),
        }
    }
}

/// The immutable application schema and upcaster graph.
#[must_use]
pub fn task_catalog() -> SealedEventCatalog<TaskEvent> {
    let mut builder = EventCatalogBuilder::new();
    builder
        .register_current::<TaskAddedV1, _>(
            "pliego.example/task-event-added-map/1",
            TaskEvent::Added,
        )
        .expect("static task-added schema is valid")
        .register_current::<TaskToggledV1, _>(
            "pliego.example/task-event-toggled-map/1",
            TaskEvent::Toggled,
        )
        .expect("static task-toggled schema is valid")
        .register_current::<TaskRemovedV1, _>(
            "pliego.example/task-event-removed-map/1",
            TaskEvent::Removed,
        )
        .expect("static task-removed schema is valid");
    builder
        .seal()
        .expect("static task catalog is complete and deterministic")
}

/// Pure, fallible task reducer. Rejections occur before state is published by
/// `Projection`, so a malformed history tail cannot partially mutate the UI.
pub fn reduce(state: &mut TaskList, event: &TaskEvent) -> Result<(), ReducerError> {
    let next_events_seen = state
        .events_seen
        .checked_add(1)
        .ok_or_else(|| ReducerError::new("event counter overflow"))?;
    match event {
        TaskEvent::Added(event) => {
            if event.id != state.events_seen {
                return Err(ReducerError::new(
                    "task identifier does not match its origin sequence",
                ));
            }
            if state.items.iter().any(|task| task.id == event.id) {
                return Err(ReducerError::new("duplicate task identifier"));
            }
            state.items.push(Task {
                id: event.id,
                text: event.text.clone(),
                done: false,
            });
        }
        TaskEvent::Toggled(event) => {
            let task = state
                .items
                .iter_mut()
                .find(|task| task.id == event.id)
                .ok_or_else(|| ReducerError::new("toggle references an unknown task"))?;
            task.done = !task.done;
        }
        TaskEvent::Removed(event) => {
            let index = state
                .items
                .iter()
                .position(|task| task.id == event.id)
                .ok_or_else(|| ReducerError::new("remove references an unknown task"))?;
            state.items.remove(index);
        }
    }
    state.events_seen = next_events_seen;
    Ok(())
}

fn task_reducer() -> Reducer<TaskList, TaskEvent> {
    let identity = ReducerIdentity::new("pliego.example/task-list", 1, [0; 32])
        .expect("static reducer identity is valid");
    Reducer::new(identity, reduce)
}

/// Build the live transactional task projection at genesis.
pub fn task_projection(
    log: ReactiveLog,
) -> Result<Projection<TaskList, TaskEvent>, ProjectionError> {
    Projection::new(
        log,
        TaskList::default(),
        task_catalog(),
        task_reducer(),
        CanonicalJsonCodec::default(),
    )
}

#[cfg(target_arch = "wasm32")]
fn append_task(log: ReactiveLog, text: impl Into<String>) -> Result<u64, LogError> {
    let id = log.len();
    log.append_typed(&TaskAddedV1 {
        id,
        text: text.into(),
    })?;
    Ok(id)
}

/// Materialize an exact prefix without constructing raw events. The source is
/// first verified, each event crosses the sealed catalog, and the rebuilt log
/// uses only `append_typed` before entering a fresh `Projection`.
#[cfg(any(test, target_arch = "wasm32"))]
fn replay_prefix(source: &Log, position: u64) -> Result<TaskList, String> {
    source
        .verify()
        .map_err(|error| format!("source log rejected: {error}"))?;
    source
        .cursor_at(position)
        .map_err(|error| format!("prefix cursor rejected: {error}"))?;
    let end = usize::try_from(position).map_err(|_| "prefix exceeds platform size".to_owned())?;
    let catalog = task_catalog();
    let mut prefix = Log::new();
    for stored in &source.events()[..end] {
        catalog
            .decode(stored)
            .map_err(|error| format!("schema rejected prefix: {error}"))?
            .append_to_log(&mut prefix)
            .map_err(|error| format!("typed prefix append rejected: {error}"))?;
    }
    task_projection(ReactiveLog::from_log(prefix))
        .and_then(|projection| projection.try_get())
        .map_err(|error| format!("prefix projection rejected: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_log() -> Log {
        let mut log = Log::new();
        for event in [
            TaskEvent::Added(TaskAddedV1 {
                id: 0,
                text: "buy milk".into(),
            }),
            TaskEvent::Added(TaskAddedV1 {
                id: 1,
                text: "call Ana".into(),
            }),
            TaskEvent::Toggled(TaskToggledV1 { id: 0 }),
            TaskEvent::Added(TaskAddedV1 {
                id: 3,
                text: "pay power bill".into(),
            }),
            TaskEvent::Removed(TaskRemovedV1 { id: 1 }),
        ] {
            event.append_to_log(&mut log).unwrap();
        }
        log
    }

    #[test]
    fn projection_builds_the_expected_state() {
        let projection = task_projection(ReactiveLog::from_log(demo_log())).unwrap();
        let state = projection.try_get().unwrap();
        assert_eq!(
            state.items,
            vec![
                Task {
                    id: 0,
                    text: "buy milk".into(),
                    done: true,
                },
                Task {
                    id: 3,
                    text: "pay power bill".into(),
                    done: false,
                },
            ]
        );
        assert_eq!(projection.history().unwrap().position, 5);
    }

    #[test]
    fn gate_typed_replay_equals_live_at_every_prefix() {
        let log = ReactiveLog::new();
        let live = task_projection(log).unwrap();
        let script = [
            TaskEvent::Added(TaskAddedV1 {
                id: 0,
                text: "a".into(),
            }),
            TaskEvent::Added(TaskAddedV1 {
                id: 1,
                text: "b".into(),
            }),
            TaskEvent::Toggled(TaskToggledV1 { id: 0 }),
            TaskEvent::Added(TaskAddedV1 {
                id: 3,
                text: "c".into(),
            }),
            TaskEvent::Removed(TaskRemovedV1 { id: 1 }),
            TaskEvent::Toggled(TaskToggledV1 { id: 0 }),
            TaskEvent::Toggled(TaskToggledV1 { id: 3 }),
            TaskEvent::Removed(TaskRemovedV1 { id: 0 }),
        ];
        for (index, event) in script.iter().enumerate() {
            event.append_to_reactive(log).unwrap();
            let live_state = live.sync().unwrap();
            let source = log.with(Clone::clone);
            assert_eq!(
                live_state,
                replay_prefix(&source, source.len()).unwrap(),
                "diverged after step {index}"
            );
        }
        assert!(log.with(Log::verify).is_ok());
    }

    #[test]
    fn time_travel_is_an_exact_typed_prefix() {
        let log = demo_log();
        let at_four = replay_prefix(&log, 4).unwrap();
        assert_eq!(at_four.items.len(), 3);
        let at_three = replay_prefix(&log, 3).unwrap();
        assert_eq!(at_three.items.len(), 2);
        assert!(at_three.items[0].done);
        assert!(replay_prefix(&log, 6).is_err());
    }

    #[test]
    fn projection_snapshot_restores_with_bound_contracts() {
        let log = ReactiveLog::from_log(demo_log());
        let live = task_projection(log).unwrap();
        let expected = live.try_get().unwrap();
        let bytes = live.snapshot().unwrap().encode();
        let restored = Projection::restore_bytes(
            log,
            &bytes,
            task_catalog(),
            task_reducer(),
            CanonicalJsonCodec::default(),
        )
        .unwrap();
        assert_eq!(restored.try_get().unwrap(), expected);
        assert_eq!(restored.history().unwrap(), log.with(Log::cursor));
    }

    #[test]
    fn forged_origin_sequence_fails_without_publishing_state() {
        let log = ReactiveLog::new();
        let projection = task_projection(log).unwrap();
        log.append_typed(&TaskAddedV1 {
            id: 7,
            text: "forged".into(),
        })
        .unwrap();
        assert!(projection.try_get().is_err());
        assert_eq!(projection.stable_state(), TaskList::default());
        assert_eq!(projection.stable_history().position, 0);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{
        TaskList, TaskRemovedV1, TaskToggledV1, append_task, replay_prefix, task_projection,
    };
    use pliego_dom::{IntoView, MountedRoot, View, dyn_text, dyn_view, el, mount_to};
    use pliego_fold::ReactiveLog;
    use pliego_hyphae::Ack;
    use pliego_log::hex;
    use pliego_reactive::Signal;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    const HYPHAE_BASE: &str = "http://127.0.0.1:18091";

    thread_local! {
        static APP_ROOT: RefCell<Option<MountedRoot>> = const { RefCell::new(None) };
    }

    /// Experimental compatibility transport. Its ACK is a UI preview only: it
    /// is neither a verified R2 receipt nor authority-bound durable evidence.
    fn sync_preview(
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
                let sequence = next_push.get();
                let Some((kind, payload)) = log.with(|raw| {
                    raw.get(sequence)
                        .map(|event| (event.kind().to_owned(), event.payload().as_str().to_owned()))
                }) else {
                    break;
                };
                match pliego_hyphae::fetch::append_remote(HYPHAE_BASE, &kind, &payload).await {
                    Ok(ack) => {
                        synced.update(|entries| {
                            entries.insert(sequence, ack);
                        });
                        next_push.set(sequence + 1);
                    }
                    Err(error) => {
                        web_sys::console::warn_1(
                            &format!("hyphae preview parked at #{sequence}: {error}").into(),
                        );
                        break;
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
        let log = ReactiveLog::new();
        let live = Rc::new(task_projection(log).expect("static task projection is valid"));
        let view_at: Signal<Option<u64>> = Signal::new(None);
        let synced: Signal<HashMap<u64, Ack>> = Signal::new(HashMap::new());
        let next_push = Rc::new(Cell::new(0_u64));
        let pushing = Rc::new(Cell::new(false));
        let kick_sync = {
            let (next_push, pushing) = (next_push.clone(), pushing.clone());
            move || sync_preview(log, synced, next_push.clone(), pushing.clone())
        };

        let current = {
            let live = live.clone();
            move || -> (TaskList, bool) {
                match view_at.get() {
                    Some(position) if position < log.len() => (
                        log.with(|raw| {
                            replay_prefix(raw, position)
                                .expect("verified local prefix must project deterministically")
                        }),
                        true,
                    ),
                    _ => (live.get(), false),
                }
            }
        };

        let header = el("div")
            .class("row")
            .child(
                el("input")
                    .id("new-task")
                    .attr("type", "text")
                    .attr("placeholder", "new task"),
            )
            .child(el("button").child("append").on("click", {
                let kick = kick_sync.clone();
                move |_| {
                    let input = input_by_id("new-task");
                    let text = input.value();
                    if !text.trim().is_empty() {
                        append_task(log, text.trim())
                            .expect("typed task append must satisfy its static schema");
                        input.set_value("");
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
                for task in &state.items {
                    let id = task.id;
                    let hash =
                        log.with(|raw| hex(raw.get(id).expect("task origin event exists").hash()));
                    let toggle_kick = kick.clone();
                    let remove_kick = kick.clone();
                    items.push(
                        el("li")
                            .class(if task.done { "done" } else { "" })
                            .child(el("button").child(if task.done { "☑" } else { "☐" }).on(
                                "click",
                                move |_| {
                                    log.append_typed(&TaskToggledV1 { id })
                                        .expect("typed toggle satisfies its static schema");
                                    view_at.set(None);
                                    toggle_kick();
                                },
                            ))
                            .child(el("span").class("text").child(task.text.clone()))
                            .child(el("button").child("×").on("click", move |_| {
                                log.append_typed(&TaskRemovedV1 { id })
                                    .expect("typed removal satisfies its static schema");
                                view_at.set(None);
                                remove_kick();
                            }))
                            .child(
                                el("code")
                                    .class("prov")
                                    .attr("title", format!("event #{id} · local {hash}"))
                                    .child(format!("#{id} · {}…", &hash[..12]))
                                    .child(dyn_text(move || {
                                        synced.with(|entries| match entries.get(&id) {
                                            Some(ack) => format!(
                                                " hyphae preview/unverified #{} {}…",
                                                ack.seq,
                                                &ack.hash[..8]
                                            ),
                                            None => " ⛓ …".to_owned(),
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
                    .child("time-travel cursor — exact typed event prefix"),
            )
            .child(
                el("input")
                    .id("cursor")
                    .attr("type", "range")
                    .attr("min", "0")
                    .attr("step", "1")
                    .attr_dyn("max", move || log.len().to_string())
                    .attr_dyn("value", move || match view_at.get() {
                        Some(position) => position.to_string(),
                        None => log.len().to_string(),
                    })
                    .on("input", move |event| {
                        let Some(target) = event.target() else {
                            return;
                        };
                        let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
                            return;
                        };
                        let position = input.value().parse().unwrap_or(0);
                        view_at.set(if position >= log.len() {
                            None
                        } else {
                            Some(position)
                        });
                    }),
            );

        let status = el("div").id("status").child(dyn_text(move || {
            let len = log.len();
            let (head, valid) = log.with(|raw| (hex(&raw.head()), raw.verify().is_ok()));
            let acknowledged = synced.with(HashMap::len) as u64;
            let mode = match view_at.get() {
                Some(position) if position < len => format!("viewing prefix {position}"),
                _ => "live".to_owned(),
            };
            format!(
                "{len} typed events · head {}… · chain {} · hyphae preview/unverified {acknowledged}/{len} · {mode}",
                &head[..12],
                if valid { "intact" } else { "BROKEN" },
            )
        }));

        let app = el("div")
            .child(header)
            .child(list)
            .child(slider)
            .child(status)
            .into_view();
        let root = mount_to("app", &app).expect("mount spike application");
        APP_ROOT.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "spike application mounted more than once");
            *slot = Some(root);
        });

        append_task(log, "estudiar Leptos (se observa, no se copia)")
            .expect("seed task schema is valid");
        append_task(log, "plegar el log tipado en interfaz").expect("seed task schema is valid");
        log.append_typed(&TaskToggledV1 { id: 0 })
            .expect("seed toggle schema is valid");
        kick_sync();
    }
}
