// SPDX-License-Identifier: AGPL-3.0-only

#![forbid(unsafe_code)]

use pliego_fold::{
    CanonicalJsonCodec, Projection, ReactiveLog, Reducer, ReducerError, ReducerIdentity,
};
use pliego_log::{EventCatalogBuilder, EventSchema, SealedEventCatalog};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io;

pub type AppResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppState {
    pub count: u64,
    pub next_item_id: u64,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterIncrementedV1 {
    pub amount: u64,
}

impl EventSchema for CounterIncrementedV1 {
    const KIND: &'static str = "app_counter_incremented";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next/counter-incremented/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemAddedV1 {
    pub id: u64,
    pub label: String,
}

impl EventSchema for ItemAddedV1 {
    const KIND: &'static str = "app_item_added";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next/item-added/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum AppEvent {
    CounterIncremented(CounterIncrementedV1),
    ItemAdded(ItemAddedV1),
}

pub enum Action {
    Increment,
    AddItem(String),
}

pub fn catalog() -> AppResult<SealedEventCatalog<AppEvent>> {
    let mut catalog = EventCatalogBuilder::new();
    catalog.register_current::<CounterIncrementedV1, _>(
        "fixtures.next/counter-incremented/current/1",
        AppEvent::CounterIncremented,
    )?;
    catalog.register_current::<ItemAddedV1, _>(
        "fixtures.next/item-added/current/1",
        AppEvent::ItemAdded,
    )?;
    Ok(catalog.seal()?)
}

pub fn reducer() -> AppResult<Reducer<AppState, AppEvent>> {
    let identity = ReducerIdentity::from_serializable_config(
        "fixtures.next/minimal-state",
        1,
        &AppState::default(),
    )?;
    Ok(Reducer::new(
        identity,
        |state: &mut AppState, event: &AppEvent| {
            match event {
                AppEvent::CounterIncremented(event) => {
                    if event.amount != 1 {
                        return Err(ReducerError::new("counter increments must have amount one"));
                    }
                    state.count = state
                        .count
                        .checked_add(event.amount)
                        .ok_or_else(|| ReducerError::new("counter overflow"))?;
                }
                AppEvent::ItemAdded(event) => {
                    if event.id != state.next_item_id {
                        return Err(ReducerError::new(
                            "item identifier does not match the next sequence",
                        ));
                    }
                    let label = event.label.trim();
                    if label.is_empty() || label != event.label {
                        return Err(ReducerError::new(
                            "item labels must be non-empty and normalized",
                        ));
                    }
                    state.next_item_id = state
                        .next_item_id
                        .checked_add(1)
                        .ok_or_else(|| ReducerError::new("item sequence overflow"))?;
                    state.items.push(Item {
                        id: event.id,
                        label: event.label.clone(),
                    });
                }
            }
            Ok(())
        },
    ))
}

pub fn projection(log: ReactiveLog) -> AppResult<Projection<AppState, AppEvent>> {
    Ok(Projection::new(
        log,
        AppState::default(),
        catalog()?,
        reducer()?,
        CanonicalJsonCodec::default(),
    )?)
}

pub fn dispatch(
    log: ReactiveLog,
    projection: &Projection<AppState, AppEvent>,
    action: Action,
) -> AppResult<()> {
    match action {
        Action::Increment => log.append_typed(&CounterIncrementedV1 { amount: 1 })?,
        Action::AddItem(label) => {
            let label = label.trim();
            if label.is_empty() {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "item label is required").into(),
                );
            }
            let id = projection.try_get()?.next_item_id;
            log.append_typed(&ItemAddedV1 {
                id,
                label: label.to_owned(),
            })?;
        }
    }
    Ok(())
}

pub fn replay_contract(
    log: ReactiveLog,
    live: &Projection<AppState, AppEvent>,
    snapshot_bytes: &[u8],
    snapshot_position: u64,
) -> AppResult<AppState> {
    log.with(|history| history.verify())?;
    let live_state = live.try_get()?;

    let genesis = projection(log)?;
    let genesis_state = genesis.try_get()?;
    if genesis_state != live_state || genesis.events_folded() != log.len() {
        return Err(contract_error("genesis replay diverged from live state"));
    }

    let restored = Projection::<AppState, AppEvent>::restore_bytes(
        log,
        snapshot_bytes,
        catalog()?,
        reducer()?,
        CanonicalJsonCodec::default(),
    )?;
    let restored_state = restored.try_get()?;
    if restored_state != live_state {
        return Err(contract_error(
            "snapshot-tail replay diverged from live state",
        ));
    }
    let expected_tail = log
        .len()
        .checked_sub(snapshot_position)
        .ok_or_else(|| contract_error("snapshot lies after current history"))?;
    if restored.events_folded() != expected_tail {
        return Err(contract_error(format!(
            "snapshot replay folded {} events; expected {expected_tail}",
            restored.events_folded()
        )));
    }
    Ok(live_state)
}

fn contract_error(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_genesis_and_snapshot_tail_replay_agree() -> AppResult<()> {
        let log = ReactiveLog::new();
        let live = projection(log)?;

        dispatch(log, &live, Action::Increment)?;
        dispatch(log, &live, Action::AddItem("seed".to_owned()))?;
        let snapshot = live.snapshot()?.encode();
        let snapshot_position = live.history()?.position;

        dispatch(log, &live, Action::Increment)?;
        dispatch(log, &live, Action::AddItem("tail".to_owned()))?;

        let state = replay_contract(log, &live, &snapshot, snapshot_position)?;
        assert_eq!(state.count, 2);
        assert_eq!(state.items.len(), 2);
        assert_eq!(snapshot_position, 2);
        assert_eq!(log.len(), 4);
        Ok(())
    }

    #[test]
    fn invalid_local_action_does_not_append() -> AppResult<()> {
        let log = ReactiveLog::new();
        let live = projection(log)?;
        assert!(dispatch(log, &live, Action::AddItem("   ".to_owned())).is_err());
        assert!(log.is_empty());
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{Action, AppEvent, AppResult, AppState, dispatch, projection, replay_contract};
    use pliego_dom::{
        DomEvent, IntoView, MountedRoot, View, dyn_text, el, keyed, mount, mount_to, show,
    };
    use pliego_fold::{Projection, ReactiveLog};
    use pliego_reactive::{Owner, Signal, on_cleanup};
    use std::cell::{Cell, RefCell};
    use std::fmt::Display;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static SESSION: RefCell<Option<Rc<Session>>> = const { RefCell::new(None) };
        static HARNESS_ROOT: RefCell<Option<MountedRoot>> = const { RefCell::new(None) };
        static APP_ROOT: RefCell<Option<MountedRoot>> = const { RefCell::new(None) };
    }

    struct Session {
        log: ReactiveLog,
        projection: Projection<AppState, AppEvent>,
        snapshot_bytes: Vec<u8>,
        snapshot_position: u64,
        verified_at: Signal<u64>,
        mounted: Signal<bool>,
    }

    impl Session {
        fn seeded() -> AppResult<Self> {
            let log = ReactiveLog::new();
            let projection = projection(log)?;
            dispatch(log, &projection, Action::Increment)?;
            dispatch(log, &projection, Action::AddItem("seed item".to_owned()))?;
            let snapshot_bytes = projection.snapshot()?.encode();
            let snapshot_position = projection.history()?.position;
            dispatch(log, &projection, Action::Increment)?;
            dispatch(log, &projection, Action::AddItem("tail item".to_owned()))?;
            replay_contract(log, &projection, &snapshot_bytes, snapshot_position)?;
            Ok(Self {
                log,
                projection,
                snapshot_bytes,
                snapshot_position,
                verified_at: Signal::new(log.len()),
                mounted: Signal::new(false),
            })
        }

        fn dispatch_and_verify(&self, action: Action) -> AppResult<()> {
            dispatch(self.log, &self.projection, action)?;
            replay_contract(
                self.log,
                &self.projection,
                &self.snapshot_bytes,
                self.snapshot_position,
            )?;
            self.verified_at.set(self.log.len());
            Ok(())
        }
    }

    fn js_error(error: impl Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }

    fn document() -> Result<web_sys::Document, JsValue> {
        web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| JsValue::from_str("browser document is unavailable"))
    }

    fn input_by_id(id: &str) -> Option<web_sys::HtmlInputElement> {
        document()
            .ok()?
            .get_element_by_id(id)?
            .dyn_into::<web_sys::HtmlInputElement>()
            .ok()
    }

    fn update_local_draft(event: DomEvent, draft: Signal<String>) {
        let Some(target) = event.target() else {
            return;
        };
        let Ok(input) = target.dyn_into::<web_sys::HtmlInputElement>() else {
            return;
        };
        draft.set(input.value());
    }

    fn application_view(session: Rc<Session>, draft: Signal<String>) -> View {
        let increment_session = Rc::clone(&session);
        let count_session = Rc::clone(&session);
        let add_session = Rc::clone(&session);
        let list_session = Rc::clone(&session);
        let conditional_session = Rc::clone(&session);
        let status_session = Rc::clone(&session);

        let rows = keyed(
            move || list_session.projection.get().items,
            |item| item.id,
            |item| {
                el("li")
                    .attr("data-item-id", item.id.to_string())
                    .child(item.label)
                    .into_view()
            },
        );
        let parity = show(
            move || conditional_session.projection.get().count % 2 == 0,
            || {
                el("p")
                    .id("counter-condition")
                    .child("counter is even")
                    .into_view()
            },
            || {
                el("p")
                    .id("counter-condition")
                    .child("counter is odd")
                    .into_view()
            },
        );

        el("section")
            .id("causal-app")
            .child(
                el("button")
                    .id("increment")
                    .attr("type", "button")
                    .child("increment")
                    .on("click", move |_| {
                        increment_session
                            .dispatch_and_verify(Action::Increment)
                            .expect("typed increment and replay parity");
                    }),
            )
            .child(
                el("output")
                    .id("counter-value")
                    .child(dyn_text(move || {
                        count_session.projection.get().count.to_string()
                    })),
            )
            .child(parity)
            .child(
                el("label")
                    .attr("for", "item-draft")
                    .child("local draft"),
            )
            .child(
                el("input")
                    .id("item-draft")
                    .attr("type", "text")
                    .on("input", move |event| update_local_draft(event, draft)),
            )
            .child(
                el("output")
                    .id("draft-preview")
                    .child(dyn_text(move || draft.get())),
            )
            .child(
                el("button")
                    .id("add-item")
                    .attr("type", "button")
                    .child("add item")
                    .on("click", move |_| {
                        let label = draft.get_untracked();
                        if label.trim().is_empty() {
                            return;
                        }
                        add_session
                            .dispatch_and_verify(Action::AddItem(label))
                            .expect("typed item append and replay parity");
                        draft.set(String::new());
                        if let Some(input) = input_by_id("item-draft") {
                            input.set_value("");
                        }
                    }),
            )
            .child(el("ul").id("items").child(rows))
            .child(
                el("output")
                    .id("replay-status")
                    .child(dyn_text(move || {
                        let verified = status_session.verified_at.get();
                        let tail = verified.saturating_sub(status_session.snapshot_position);
                        format!(
                            "{verified} typed events; live = genesis replay = snapshot-tail replay; snapshot tail {tail}"
                        )
                    })),
            )
            .into_view()
    }

    fn harness_view(session: Rc<Session>) -> View {
        let status = Rc::clone(&session);
        el("section")
            .id("fixture-harness")
            .child(
                el("button")
                    .id("mount-app")
                    .attr("type", "button")
                    .child("mount")
                    .on("click", |_| {
                        fixture_mount().expect("mount fixture application");
                    }),
            )
            .child(
                el("button")
                    .id("unmount-app")
                    .attr("type", "button")
                    .child("unmount")
                    .on("click", |_| fixture_unmount()),
            )
            .child(el("output").id("mount-status").child(dyn_text(move || {
                if status.mounted.get() {
                    "mounted".to_owned()
                } else {
                    "unmounted".to_owned()
                }
            })))
            .child(el("div").id("app-host"))
            .into_view()
    }

    fn mount_application(session: Rc<Session>) -> Result<MountedRoot, JsValue> {
        let host = document()?
            .get_element_by_id("app-host")
            .ok_or_else(|| JsValue::from_str("app-host is unavailable"))?;
        let owner = Owner::new();
        let draft = owner.signal(String::new()).map_err(js_error)?;
        let view = application_view(Rc::clone(&session), draft);
        let root = mount(&view, host.as_ref()).map_err(js_error)?;

        let disposed_host = host.clone();
        owner
            .on_cleanup(move || {
                let _ = disposed_host.set_attribute("data-owned-effect", "disposed");
            })
            .map_err(js_error)?;

        let runs = Rc::new(Cell::new(0_u64));
        let cleanups = Rc::new(Cell::new(0_u64));
        let effect_host = host.clone();
        let effect_runs = Rc::clone(&runs);
        let effect_cleanups = Rc::clone(&cleanups);
        let effect_session = Rc::clone(&session);
        owner
            .effect(move || {
                let state = effect_session.projection.get();
                let draft_len = draft.get().len();
                let run = effect_runs.get() + 1;
                effect_runs.set(run);
                effect_host
                    .set_attribute(
                        "data-owned-effect",
                        &format!(
                            "active:{run}:count={}:items={}:draft={draft_len}",
                            state.count,
                            state.items.len()
                        ),
                    )
                    .expect("known data attribute is valid");
                let cleanup_host = effect_host.clone();
                let cleanup_counter = Rc::clone(&effect_cleanups);
                on_cleanup(move || {
                    let next = cleanup_counter.get() + 1;
                    cleanup_counter.set(next);
                    let _ =
                        cleanup_host.set_attribute("data-owned-effect-cleanups", &next.to_string());
                });
            })
            .map_err(js_error)?;

        root.scope()
            .on_cleanup(move || owner.dispose())
            .map_err(js_error)?;
        Ok(root)
    }

    #[wasm_bindgen]
    pub fn fixture_mount() -> Result<(), JsValue> {
        if APP_ROOT.with(|slot| slot.borrow().is_some()) {
            return Ok(());
        }
        let session = SESSION
            .with(|slot| slot.borrow().as_ref().cloned())
            .ok_or_else(|| JsValue::from_str("fixture session is unavailable"))?;
        let root = mount_application(Rc::clone(&session))?;
        APP_ROOT.with(|slot| {
            *slot.borrow_mut() = Some(root);
        });
        session.mounted.set(true);
        Ok(())
    }

    #[wasm_bindgen]
    pub fn fixture_unmount() {
        if let Some(root) = APP_ROOT.with(|slot| slot.borrow_mut().take()) {
            root.dispose();
        }
        SESSION.with(|slot| {
            if let Some(session) = slot.borrow().as_ref() {
                session.mounted.set(false);
            }
        });
    }

    #[wasm_bindgen]
    pub fn fixture_remount() -> Result<(), JsValue> {
        fixture_unmount();
        fixture_mount()
    }

    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        if SESSION.with(|slot| slot.borrow().is_some()) {
            return Err(JsValue::from_str("fixture started more than once"));
        }
        let session = Rc::new(Session::seeded().map_err(js_error)?);
        SESSION.with(|slot| {
            *slot.borrow_mut() = Some(Rc::clone(&session));
        });
        let harness = harness_view(session);
        let root = mount_to("fixture-root", &harness).map_err(js_error)?;
        HARNESS_ROOT.with(|slot| {
            *slot.borrow_mut() = Some(root);
        });
        fixture_mount()
    }
}
