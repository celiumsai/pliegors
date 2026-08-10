use super::{
    Action, AppEvent, AppResult, DashboardState, ROW_COUNT, Region, RowState, Severity, SortOrder,
    dispatch, projection, replay_contract,
};
use js_sys::WebAssembly;
use pliego_dom::{DomEvent, IntoView, MountedRoot, View, dyn_text, el, keyed, mount, mount_to};
use pliego_fold::{Projection, ReactiveLog};
use pliego_reactive::{Memo, Owner, Signal};
use serde::Serialize;
use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::rc::Rc;
use wasm_bindgen::{JsCast, prelude::*};

const LIFECYCLE_WARMUP: u32 = 1_000;
const LIFECYCLE_BATCHES: u32 = 10;
const LIFECYCLE_BATCH_CYCLES: u32 = 1_000;

thread_local! {
    static SESSION: RefCell<Option<Rc<Session>>> = const { RefCell::new(None) };
    static HARNESS_ROOT: RefCell<Option<MountedRoot>> = const { RefCell::new(None) };
    static DASHBOARD_ROOT: RefCell<Option<MountedRoot>> = const { RefCell::new(None) };
}

struct Session {
    log: ReactiveLog,
    projection: Rc<Projection<DashboardState, AppEvent>>,
    snapshot_bytes: Vec<u8>,
    snapshot_position: u64,
    verified_at: Signal<u64>,
    mounted: Signal<bool>,
}

impl Session {
    fn seeded() -> AppResult<Self> {
        let log = ReactiveLog::new();
        let projection = Rc::new(projection(log)?);
        dispatch(log, &projection, Action::Tick(8))?;
        let snapshot_bytes = projection.snapshot()?.encode();
        let snapshot_position = projection.history()?.position;
        dispatch(log, &projection, Action::Tick(2))?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowDisplay {
    throughput: u16,
    latency_ms: u16,
    errors: u16,
    revision: u32,
}

impl From<&RowState> for RowDisplay {
    fn from(row: &RowState) -> Self {
        Self {
            throughput: row.throughput,
            latency_ms: row.latency_ms,
            errors: row.errors,
            revision: row.revision,
        }
    }
}

#[derive(Clone)]
struct RowBinding {
    id: u32,
    region: Region,
    severity: Severity,
    display: Signal<RowDisplay>,
}

struct OwnedDashboard {
    dashboard: Owner,
    table: Owner,
    detail: Owner,
    chart: Owner,
}

impl OwnedDashboard {
    fn dispose(self) {
        self.dashboard.dispose();
        drop((self.table, self.detail, self.chart));
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleObservation {
    completed_cycles: u32,
    linear_memory_bytes: u64,
    dom_child_nodes: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleReport {
    contract: &'static str,
    warmup_cycles: u32,
    measured_cycles: u32,
    batch_cycles: u32,
    observations: Vec<LifecycleObservation>,
    memory_plateau: bool,
    dom_residue: u32,
    detached_listener_calls: u32,
}

fn js_error(error: impl Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn document() -> Result<web_sys::Document, JsValue> {
    web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("browser document is unavailable"))
}

fn selected_value(event: DomEvent) -> Option<String> {
    event
        .target()?
        .dyn_into::<web_sys::HtmlSelectElement>()
        .ok()
        .map(|select| select.value())
}

fn select_view(
    id: &str,
    label: &str,
    options: &[(&str, &str)],
    handler: impl Fn(DomEvent) + 'static,
) -> View {
    let mut select = el("select").id(id).on("change", handler);
    for (value, text) in options {
        select = select.child(el("option").attr("value", *value).child(*text));
    }
    el("label")
        .child(el("span").class("metric-label").child(label))
        .child(select)
        .into_view()
}

fn row_view(row: RowBinding, selected: Signal<Option<u32>>, session: Rc<Session>) -> View {
    let selected_id = row.id;
    let throughput = row.display;
    let latency = row.display;
    let errors = row.display;
    let revision = row.display;
    el("tr")
        .attr("data-row-id", row.id.to_string())
        .attr_dyn("data-selected", move || {
            (selected.get() == Some(selected_id)).to_string()
        })
        .on("click", move |_| {
            session
                .dispatch_and_verify(Action::SelectRow(selected_id))
                .expect("selected row must remain inside the deterministic dataset");
        })
        .child(el("td").child(row.id.to_string()))
        .child(el("td").child(row.region.label()))
        .child(el("td").child(row.severity.label()))
        .child(
            el("td")
                .class("throughput")
                .child(dyn_text(move || throughput.get().throughput.to_string())),
        )
        .child(
            el("td")
                .class("latency")
                .child(dyn_text(move || latency.get().latency_ms.to_string())),
        )
        .child(
            el("td")
                .class("errors")
                .child(dyn_text(move || errors.get().errors.to_string())),
        )
        .child(
            el("td")
                .class("revision")
                .child(dyn_text(move || revision.get().revision.to_string())),
        )
        .into_view()
}

fn table_view(
    bindings: Rc<Vec<RowBinding>>,
    visible: Memo<Vec<u32>>,
    selected: Signal<Option<u32>>,
    session: Rc<Session>,
) -> View {
    let rows = keyed(
        move || {
            visible
                .get()
                .into_iter()
                .map(|id| bindings[id as usize].clone())
                .collect::<Vec<_>>()
        },
        |row| row.id,
        move |row| row_view(row, selected, Rc::clone(&session)),
    );
    el("div")
        .class("panel table-shell")
        .child(
            el("table")
                .id("dashboard-table")
                .child(
                    el("thead").child(
                        el("tr")
                            .child(el("th").child("id"))
                            .child(el("th").child("region"))
                            .child(el("th").child("severity"))
                            .child(el("th").child("throughput"))
                            .child(el("th").child("latency"))
                            .child(el("th").child("errors"))
                            .child(el("th").child("revision")),
                    ),
                )
                .child(el("tbody").id("dashboard-rows").child(rows)),
        )
        .into_view()
}

fn dashboard_view(
    session: Rc<Session>,
    frame: Signal<Rc<DashboardState>>,
    visible: Memo<Vec<u32>>,
    bindings: Rc<Vec<RowBinding>>,
    selected: Signal<Option<u32>>,
    chart_points: Signal<String>,
) -> View {
    let once = Rc::clone(&session);
    let burst = Rc::clone(&session);
    let region = Rc::clone(&session);
    let severity = Rc::clone(&session);
    let sort = Rc::clone(&session);
    let replay = Rc::clone(&session);
    let visible_count = visible;
    let selected_frame = frame;

    let controls = el("div")
        .class("controls")
        .child(select_view(
            "region-filter",
            "region",
            &[
                ("all", "all"),
                ("north", "north"),
                ("south", "south"),
                ("east", "east"),
                ("west", "west"),
            ],
            move |event| {
                let value = selected_value(event).unwrap_or_default();
                let selected = match value.as_str() {
                    "north" => Some(Region::North),
                    "south" => Some(Region::South),
                    "east" => Some(Region::East),
                    "west" => Some(Region::West),
                    _ => None,
                };
                region
                    .dispatch_and_verify(Action::SetRegion(selected))
                    .expect("valid region filter");
            },
        ))
        .child(select_view(
            "severity-filter",
            "severity",
            &[
                ("all", "all"),
                ("normal", "normal"),
                ("elevated", "elevated"),
                ("warning", "warning"),
                ("critical", "critical"),
            ],
            move |event| {
                let value = selected_value(event).unwrap_or_default();
                let selected = match value.as_str() {
                    "normal" => Some(Severity::Normal),
                    "elevated" => Some(Severity::Elevated),
                    "warning" => Some(Severity::Warning),
                    "critical" => Some(Severity::Critical),
                    _ => None,
                };
                severity
                    .dispatch_and_verify(Action::SetSeverity(selected))
                    .expect("valid severity filter");
            },
        ))
        .child(select_view(
            "sort-order",
            "sort",
            &[
                ("id", "id ascending"),
                ("throughput", "throughput descending"),
            ],
            move |event| {
                let order = if selected_value(event).as_deref() == Some("throughput") {
                    SortOrder::ThroughputDescending
                } else {
                    SortOrder::IdAscending
                };
                sort.dispatch_and_verify(Action::SetSort(order))
                    .expect("valid sort order");
            },
        ))
        .child(
            el("button")
                .id("tick-once")
                .attr("type", "button")
                .child("tick once")
                .on("click", move |_| {
                    once.dispatch_and_verify(Action::Tick(1))
                        .expect("one deterministic tick");
                }),
        )
        .child(
            el("button")
                .id("run-60-updates")
                .attr("type", "button")
                .child("run 60 updates")
                .on("click", move |_| {
                    burst
                        .dispatch_and_verify(Action::Tick(60))
                        .expect("bounded deterministic tick burst");
                }),
        );

    let metrics = el("div")
        .class("metrics")
        .child(metric("dataset", "dataset-size", move || {
            ROW_COUNT.to_string()
        }))
        .child(metric("visible", "visible-count", move || {
            visible_count.get().len().to_string()
        }))
        .child(metric("tick", "tick-value", move || {
            frame.get().tick.to_string()
        }))
        .child(metric("events", "event-count", move || {
            frame.get().events_applied.to_string()
        }));

    let chart = el("section")
        .class("panel")
        .child(el("h2").child("Throughput series"))
        .child(
            el("svg")
                .id("throughput-chart")
                .attr("viewBox", "0 0 640 160")
                .attr("role", "img")
                .child(
                    el("polyline")
                        .id("throughput-line")
                        .attr_dyn("points", move || chart_points.get()),
                ),
        );
    let detail = el("section")
        .class("panel")
        .child(el("h2").child("Selected row"))
        .child(el("output").id("selected-row").child(dyn_text(move || {
            selected_frame
                .get()
                .selected
                .map_or_else(|| "none".to_owned(), |id| id.to_string())
        })))
        .child(
            el("p")
                .id("replay-status")
                .attr("data-parity", "true")
                .child(dyn_text(move || {
                    let events = replay.verified_at.get();
                    let tail = events.saturating_sub(replay.snapshot_position);
                    format!("{events} typed events; snapshot tail {tail}; replay parity true")
                })),
        );

    el("section")
        .id("stress-dashboard")
        .child(controls)
        .child(metrics)
        .child(
            el("div")
                .class("dashboard-grid")
                .child(table_view(bindings, visible, selected, Rc::clone(&session)))
                .child(el("div").child(chart).child(detail)),
        )
        .into_view()
}

fn metric(label: &str, id: &str, value: impl Fn() -> String + 'static) -> View {
    el("div")
        .class("metric")
        .child(el("span").class("metric-label").child(label.to_owned()))
        .child(el("strong").id(id).child(dyn_text(value)))
        .into_view()
}

fn mount_dashboard(session: Rc<Session>) -> Result<MountedRoot, JsValue> {
    let host = document()?
        .get_element_by_id("dashboard-host")
        .ok_or_else(|| JsValue::from_str("dashboard host is unavailable"))?;
    let dashboard = Owner::new();
    let table = dashboard.child().map_err(js_error)?;
    let detail = table.child().map_err(js_error)?;
    let chart = dashboard.child().map_err(js_error)?;
    let initial = session.projection.try_get().map_err(js_error)?;
    let frame = dashboard
        .signal(Rc::new(initial.clone()))
        .map_err(js_error)?;
    let selected = detail.signal(initial.selected).map_err(js_error)?;
    let chart_points = chart
        .signal(chart_path(&initial.series))
        .map_err(js_error)?;
    let bindings = Rc::new(
        initial
            .rows
            .iter()
            .map(|row| {
                Ok(RowBinding {
                    id: row.id,
                    region: row.region,
                    severity: row.severity,
                    display: table.signal(RowDisplay::from(row))?,
                })
            })
            .collect::<Result<Vec<_>, pliego_reactive::OwnerError>>()
            .map_err(js_error)?,
    );

    let projection_frame = Rc::clone(&session.projection);
    dashboard
        .effect(move || frame.set(Rc::new(projection_frame.get())))
        .map_err(js_error)?;
    let row_frame = frame;
    let row_bindings = Rc::clone(&bindings);
    table
        .effect(move || {
            let state = row_frame.get();
            for (row, binding) in state.rows.iter().zip(row_bindings.iter()) {
                let next = RowDisplay::from(row);
                if binding.display.get_untracked() != next {
                    binding.display.set(next);
                }
            }
        })
        .map_err(js_error)?;
    let selected_frame = frame;
    detail
        .effect(move || selected.set(selected_frame.get().selected))
        .map_err(js_error)?;
    let chart_frame = frame;
    chart
        .effect(move || chart_points.set(chart_path(&chart_frame.get().series)))
        .map_err(js_error)?;
    let visible_frame = frame;
    let visible = table
        .memo(move || visible_frame.get().visible_ids())
        .map_err(js_error)?;
    let view = dashboard_view(
        Rc::clone(&session),
        frame,
        visible,
        Rc::clone(&bindings),
        selected,
        chart_points,
    );
    let root = mount(&view, host.as_ref()).map_err(js_error)?;
    root.scope()
        .on_cleanup(move || {
            OwnedDashboard {
                dashboard,
                table,
                detail,
                chart,
            }
            .dispose()
        })
        .map_err(js_error)?;
    Ok(root)
}

fn chart_path(series: &[u16]) -> String {
    let width = 640_u32;
    let height = 160_u32;
    let denominator = series.len().saturating_sub(1).max(1) as u32;
    series
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = index as u32 * width / denominator;
            let y = height.saturating_sub(u32::from(*value).saturating_mul(height) / 1_000);
            format!("{x},{y}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn harness_view(session: Rc<Session>) -> View {
    let status = Rc::clone(&session);
    el("section")
        .id("fixture-harness")
        .child(
            el("div")
                .class("controls")
                .child(
                    el("button")
                        .id("mount-dashboard")
                        .attr("type", "button")
                        .child("mount dashboard")
                        .on("click", |_| {
                            fixture_mount().expect("mount stress dashboard");
                        }),
                )
                .child(
                    el("button")
                        .id("unmount-dashboard")
                        .attr("type", "button")
                        .child("unmount dashboard")
                        .on("click", |_| fixture_unmount()),
                ),
        )
        .child(el("output").id("mount-status").child(dyn_text(move || {
            if status.mounted.get() {
                "mounted".to_owned()
            } else {
                "unmounted".to_owned()
            }
        })))
        .child(el("div").id("dashboard-host"))
        .child(el("div").id("plateau-host"))
        .into_view()
}

fn wasm_memory_bytes() -> Result<u64, JsValue> {
    let memory = wasm_bindgen::memory().dyn_into::<WebAssembly::Memory>()?;
    let buffer = memory.buffer().dyn_into::<js_sys::ArrayBuffer>()?;
    Ok(u64::from(buffer.byte_length()))
}

fn run_probe_cycle(
    host: &web_sys::Element,
    detached_listener_calls: Rc<Cell<u32>>,
    retain_button: bool,
) -> Result<Option<web_sys::Element>, JsValue> {
    let owner = Owner::new();
    let child = owner.child().map_err(js_error)?;
    let grandchild = child.child().map_err(js_error)?;
    let value = grandchild.signal(0_u32).map_err(js_error)?;
    grandchild
        .effect(move || {
            let _ = value.get();
        })
        .map_err(js_error)?;
    let calls = Rc::clone(&detached_listener_calls);
    let view = el("button")
        .class("plateau-probe")
        .child(dyn_text(move || value.get().to_string()))
        .on("click", move |_| calls.set(calls.get() + 1))
        .into_view();
    let root = mount(&view, host.as_ref()).map_err(js_error)?;
    let button = if retain_button {
        host.first_element_child()
    } else {
        None
    };
    value.set(1);
    root.dispose();
    owner.dispose();
    drop((child, grandchild));
    if host.child_element_count() != 0 {
        return Err(JsValue::from_str("lifecycle probe left DOM residue"));
    }
    Ok(button)
}

#[wasm_bindgen]
pub fn run_lifecycle_plateau() -> Result<String, JsValue> {
    let host = document()?
        .get_element_by_id("plateau-host")
        .ok_or_else(|| JsValue::from_str("plateau host is unavailable"))?;
    let detached_calls = Rc::new(Cell::new(0_u32));
    let detached = run_probe_cycle(&host, Rc::clone(&detached_calls), true)?;
    if let Some(button) = detached {
        button.dispatch_event(&web_sys::Event::new("click")?)?;
    }
    for _ in 1..LIFECYCLE_WARMUP {
        run_probe_cycle(&host, Rc::clone(&detached_calls), false)?;
    }
    let mut observations = Vec::with_capacity(LIFECYCLE_BATCHES as usize);
    for batch in 1..=LIFECYCLE_BATCHES {
        for _ in 0..LIFECYCLE_BATCH_CYCLES {
            run_probe_cycle(&host, Rc::clone(&detached_calls), false)?;
        }
        observations.push(LifecycleObservation {
            completed_cycles: batch * LIFECYCLE_BATCH_CYCLES,
            linear_memory_bytes: wasm_memory_bytes()?,
            dom_child_nodes: host.child_element_count(),
        });
    }
    let tail = observations
        .iter()
        .rev()
        .take(3)
        .map(|item| item.linear_memory_bytes)
        .collect::<Vec<_>>();
    let memory_plateau = tail.len() == 3
        && tail.windows(2).all(|pair| pair[0] == pair[1])
        && observations.iter().all(|item| item.dom_child_nodes == 0);
    serde_json::to_string(&LifecycleReport {
        contract: "dev.pliegors.next-stress-lifecycle/v1",
        warmup_cycles: LIFECYCLE_WARMUP,
        measured_cycles: LIFECYCLE_BATCHES * LIFECYCLE_BATCH_CYCLES,
        batch_cycles: LIFECYCLE_BATCH_CYCLES,
        observations,
        memory_plateau,
        dom_residue: host.child_element_count(),
        detached_listener_calls: detached_calls.get(),
    })
    .map_err(js_error)
}

#[wasm_bindgen]
pub fn fixture_mount() -> Result<(), JsValue> {
    if DASHBOARD_ROOT.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let session = SESSION
        .with(|slot| slot.borrow().as_ref().cloned())
        .ok_or_else(|| JsValue::from_str("fixture session is unavailable"))?;
    let root = mount_dashboard(Rc::clone(&session))?;
    DASHBOARD_ROOT.with(|slot| *slot.borrow_mut() = Some(root));
    session.mounted.set(true);
    Ok(())
}

#[wasm_bindgen]
pub fn fixture_unmount() {
    if let Some(root) = DASHBOARD_ROOT.with(|slot| slot.borrow_mut().take()) {
        root.dispose();
    }
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow().as_ref() {
            session.mounted.set(false);
        }
    });
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    if SESSION.with(|slot| slot.borrow().is_some()) {
        return Err(JsValue::from_str("fixture started more than once"));
    }
    let session = Rc::new(Session::seeded().map_err(js_error)?);
    SESSION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&session)));
    let root = mount_to("fixture-root", &harness_view(session)).map_err(js_error)?;
    HARNESS_ROOT.with(|slot| *slot.borrow_mut() = Some(root));
    fixture_mount()
}
