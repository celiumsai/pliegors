// SPDX-License-Identifier: AGPL-3.0-only

#![forbid(unsafe_code)]

use pliego_fold::{
    CanonicalJsonCodec, Projection, ReactiveLog, Reducer, ReducerError, ReducerIdentity,
};
use pliego_log::{EventCatalogBuilder, EventSchema, SealedEventCatalog};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io;

pub const ROW_COUNT: u32 = 1_536;
pub const ROWS_PER_TICK: u32 = 48;
pub const CHART_POINTS: usize = 64;
pub const MAX_TICKS_PER_ACTION: u32 = 60;
pub const MAX_LOG_EVENTS: u64 = 512;

pub type AppResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    North,
    South,
    East,
    West,
}

impl Region {
    fn from_id(id: u32) -> Self {
        match id % 4 {
            0 => Self::North,
            1 => Self::South,
            2 => Self::East,
            _ => Self::West,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Normal,
    Elevated,
    Warning,
    Critical,
}

impl Severity {
    fn from_id(id: u32) -> Self {
        match (id / 4) % 4 {
            0 => Self::Normal,
            1 => Self::Elevated,
            2 => Self::Warning,
            _ => Self::Critical,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    IdAscending,
    ThroughputDescending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowState {
    pub id: u32,
    pub region: Region,
    pub severity: Severity,
    pub throughput: u16,
    pub latency_ms: u16,
    pub errors: u16,
    pub revision: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardState {
    pub tick: u32,
    pub events_applied: u64,
    pub rows: Vec<RowState>,
    pub region_filter: Option<Region>,
    pub severity_filter: Option<Severity>,
    pub sort: SortOrder,
    pub selected: Option<u32>,
    pub series: Vec<u16>,
}

impl DashboardState {
    pub fn seeded() -> Self {
        let rows = (0..ROW_COUNT).map(seed_row).collect::<Vec<_>>();
        let initial_average = average_throughput(&rows);
        Self {
            tick: 0,
            events_applied: 0,
            rows,
            region_filter: None,
            severity_filter: None,
            sort: SortOrder::IdAscending,
            selected: None,
            series: vec![initial_average],
        }
    }

    pub fn visible_ids(&self) -> Vec<u32> {
        let mut ids = self
            .rows
            .iter()
            .filter(|row| self.region_filter.is_none_or(|region| row.region == region))
            .filter(|row| {
                self.severity_filter
                    .is_none_or(|severity| row.severity == severity)
            })
            .map(|row| row.id)
            .collect::<Vec<_>>();
        if self.sort == SortOrder::ThroughputDescending {
            ids.sort_by(|left, right| {
                self.rows[*right as usize]
                    .throughput
                    .cmp(&self.rows[*left as usize].throughput)
                    .then_with(|| left.cmp(right))
            });
        }
        ids
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardTickedV1 {
    pub tick: u32,
}

impl EventSchema for DashboardTickedV1 {
    const KIND: &'static str = "app_dashboard_ticked";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next.stress/dashboard-ticked/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionFilterSetV1 {
    pub region: Option<Region>,
}

impl EventSchema for RegionFilterSetV1 {
    const KIND: &'static str = "app_dashboard_region_filter_set";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next.stress/region-filter-set/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeverityFilterSetV1 {
    pub severity: Option<Severity>,
}

impl EventSchema for SeverityFilterSetV1 {
    const KIND: &'static str = "app_dashboard_severity_filter_set";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next.stress/severity-filter-set/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortOrderSetV1 {
    pub order: SortOrder,
}

impl EventSchema for SortOrderSetV1 {
    const KIND: &'static str = "app_dashboard_sort_order_set";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next.stress/sort-order-set/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowSelectedV1 {
    pub id: u32,
}

impl EventSchema for RowSelectedV1 {
    const KIND: &'static str = "app_dashboard_row_selected";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "fixtures.next.stress/row-selected/1";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum AppEvent {
    Ticked(DashboardTickedV1),
    RegionFilterSet(RegionFilterSetV1),
    SeverityFilterSet(SeverityFilterSetV1),
    SortOrderSet(SortOrderSetV1),
    RowSelected(RowSelectedV1),
}

#[derive(Clone, Copy)]
pub enum Action {
    Tick(u32),
    SetRegion(Option<Region>),
    SetSeverity(Option<Severity>),
    SetSort(SortOrder),
    SelectRow(u32),
}

#[derive(Serialize)]
struct WorkloadConfig {
    algorithm: &'static str,
    rows: u32,
    rows_per_tick: u32,
    chart_points: usize,
}

pub fn catalog() -> AppResult<SealedEventCatalog<AppEvent>> {
    let mut catalog = EventCatalogBuilder::new();
    catalog.register_current::<DashboardTickedV1, _>(
        "fixtures.next.stress/dashboard-ticked/current/1",
        AppEvent::Ticked,
    )?;
    catalog.register_current::<RegionFilterSetV1, _>(
        "fixtures.next.stress/region-filter-set/current/1",
        AppEvent::RegionFilterSet,
    )?;
    catalog.register_current::<SeverityFilterSetV1, _>(
        "fixtures.next.stress/severity-filter-set/current/1",
        AppEvent::SeverityFilterSet,
    )?;
    catalog.register_current::<SortOrderSetV1, _>(
        "fixtures.next.stress/sort-order-set/current/1",
        AppEvent::SortOrderSet,
    )?;
    catalog.register_current::<RowSelectedV1, _>(
        "fixtures.next.stress/row-selected/current/1",
        AppEvent::RowSelected,
    )?;
    Ok(catalog.seal()?)
}

pub fn reducer() -> AppResult<Reducer<DashboardState, AppEvent>> {
    let identity = ReducerIdentity::from_serializable_config(
        "fixtures.next.stress/dashboard-state",
        1,
        &WorkloadConfig {
            algorithm: "stress-dashboard-v1",
            rows: ROW_COUNT,
            rows_per_tick: ROWS_PER_TICK,
            chart_points: CHART_POINTS,
        },
    )?;
    Ok(Reducer::new(
        identity,
        |state: &mut DashboardState, event: &AppEvent| {
            match event {
                AppEvent::Ticked(event) => apply_tick(state, event.tick)?,
                AppEvent::RegionFilterSet(event) => state.region_filter = event.region,
                AppEvent::SeverityFilterSet(event) => state.severity_filter = event.severity,
                AppEvent::SortOrderSet(event) => state.sort = event.order,
                AppEvent::RowSelected(event) => {
                    if event.id >= ROW_COUNT {
                        return Err(ReducerError::new("selected row is outside the dataset"));
                    }
                    state.selected = Some(event.id);
                }
            }
            state.events_applied = state
                .events_applied
                .checked_add(1)
                .ok_or_else(|| ReducerError::new("event counter overflow"))?;
            Ok(())
        },
    ))
}

pub fn projection(log: ReactiveLog) -> AppResult<Projection<DashboardState, AppEvent>> {
    Ok(Projection::new(
        log,
        DashboardState::seeded(),
        catalog()?,
        reducer()?,
        CanonicalJsonCodec::with_max_bytes(1_048_576)?,
    )?)
}

pub fn dispatch(
    log: ReactiveLog,
    projection: &Projection<DashboardState, AppEvent>,
    action: Action,
) -> AppResult<()> {
    let additional = match action {
        Action::Tick(count) => u64::from(count),
        _ => 1,
    };
    if log
        .len()
        .checked_add(additional)
        .is_none_or(|total| total > MAX_LOG_EVENTS)
    {
        return Err(input_error("fixture event limit exceeded"));
    }
    match action {
        Action::Tick(count) => {
            if count == 0 || count > MAX_TICKS_PER_ACTION {
                return Err(input_error("tick count must be between one and sixty"));
            }
            let start = projection.try_get()?.tick;
            for offset in 1..=count {
                log.append_typed(&DashboardTickedV1 {
                    tick: start
                        .checked_add(offset)
                        .ok_or_else(|| input_error("tick overflow"))?,
                })?;
            }
        }
        Action::SetRegion(region) => log.append_typed(&RegionFilterSetV1 { region })?,
        Action::SetSeverity(severity) => log.append_typed(&SeverityFilterSetV1 { severity })?,
        Action::SetSort(order) => log.append_typed(&SortOrderSetV1 { order })?,
        Action::SelectRow(id) => log.append_typed(&RowSelectedV1 { id })?,
    }
    Ok(())
}

pub fn replay_contract(
    log: ReactiveLog,
    live: &Projection<DashboardState, AppEvent>,
    snapshot_bytes: &[u8],
    snapshot_position: u64,
) -> AppResult<DashboardState> {
    log.with(|history| history.verify())?;
    let live_state = live.try_get()?;
    let live_snapshot = live.snapshot()?;
    let live_root = *live_snapshot.state_digest();

    let genesis = projection(log)?;
    let genesis_state = genesis.try_get()?;
    if genesis_state != live_state || genesis.events_folded() != log.len() {
        return Err(input_error("genesis replay diverged from live state"));
    }
    if *genesis.snapshot()?.state_digest() != live_root {
        return Err(input_error("genesis replay state digest diverged"));
    }

    let restored = Projection::<DashboardState, AppEvent>::restore_bytes(
        log,
        snapshot_bytes,
        catalog()?,
        reducer()?,
        CanonicalJsonCodec::with_max_bytes(1_048_576)?,
    )?;
    if restored.try_get()? != live_state || *restored.snapshot()?.state_digest() != live_root {
        return Err(input_error("snapshot-tail replay diverged from live state"));
    }
    let expected_tail = log
        .len()
        .checked_sub(snapshot_position)
        .ok_or_else(|| input_error("snapshot lies after current history"))?;
    if restored.events_folded() != expected_tail {
        return Err(input_error(
            "snapshot-tail replay folded the wrong event count",
        ));
    }
    Ok(live_state)
}

fn seed_row(id: u32) -> RowState {
    RowState {
        id,
        region: Region::from_id(id),
        severity: Severity::from_id(id),
        throughput: 100 + ((id * 17) % 900) as u16,
        latency_ms: 20 + ((id * 13) % 180) as u16,
        errors: ((id * 7) % 20) as u16,
        revision: 0,
    }
}

fn apply_tick(state: &mut DashboardState, tick: u32) -> Result<(), ReducerError> {
    let expected = state
        .tick
        .checked_add(1)
        .ok_or_else(|| ReducerError::new("tick overflow"))?;
    if tick != expected {
        return Err(ReducerError::new("tick sequence is not contiguous"));
    }
    let start = tick.wrapping_mul(24) % ROW_COUNT;
    for offset in 0..ROWS_PER_TICK {
        let id = (start + offset) % ROW_COUNT;
        let row = &mut state.rows[id as usize];
        row.throughput = 100 + ((u32::from(row.throughput - 100) + 23 + (tick % 17)) % 900) as u16;
        row.latency_ms = 20 + ((u32::from(row.latency_ms - 20) + 7 + (id % 11)) % 180) as u16;
        row.errors = ((u32::from(row.errors) + ((tick + id) % 5)) % 100) as u16;
        row.revision = row
            .revision
            .checked_add(1)
            .ok_or_else(|| ReducerError::new("row revision overflow"))?;
    }
    state.tick = tick;
    state.series.push(average_throughput(&state.rows));
    if state.series.len() > CHART_POINTS {
        state.series.remove(0);
    }
    Ok(())
}

fn average_throughput(rows: &[RowState]) -> u16 {
    let total = rows
        .iter()
        .map(|row| u64::from(row.throughput))
        .sum::<u64>();
    (total / rows.len() as u64) as u16
}

fn input_error(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_filters_have_expected_cardinality() {
        let mut state = DashboardState::seeded();
        assert_eq!(state.visible_ids().len(), 1_536);
        state.region_filter = Some(Region::West);
        assert_eq!(state.visible_ids().len(), 384);
        state.severity_filter = Some(Severity::Critical);
        assert_eq!(state.visible_ids().len(), 96);
        assert!(state.visible_ids().contains(&15));
    }

    #[test]
    fn live_genesis_and_snapshot_tail_replay_agree() -> AppResult<()> {
        let log = ReactiveLog::new();
        let live = projection(log)?;
        dispatch(log, &live, Action::Tick(8))?;
        let snapshot = live.snapshot()?.encode();
        let snapshot_position = live.history()?.position;
        dispatch(log, &live, Action::Tick(2))?;
        dispatch(log, &live, Action::SetRegion(Some(Region::West)))?;
        dispatch(log, &live, Action::SetSeverity(Some(Severity::Critical)))?;
        let state = replay_contract(log, &live, &snapshot, snapshot_position)?;
        assert_eq!(state.tick, 10);
        assert_eq!(state.events_applied, 12);
        assert_eq!(state.visible_ids().len(), 96);
        assert_eq!(snapshot_position, 8);
        Ok(())
    }

    #[test]
    fn invalid_tick_fails_without_publishing_state() -> AppResult<()> {
        let log = ReactiveLog::new();
        let live = projection(log)?;
        log.append_typed(&DashboardTickedV1 { tick: 2 })?;
        assert!(live.try_get().is_err());
        assert_eq!(live.stable_state(), DashboardState::seeded());
        assert_eq!(live.stable_history().position, 0);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
mod web;
