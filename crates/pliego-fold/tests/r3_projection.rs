// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use pliego_fold::{
    CANONICAL_JSON_CODEC_ID, CanonicalJsonCodec, CodecError, CodecIdentity,
    MAX_CANONICAL_STATE_BYTES, MAX_PROJECTION_SNAPSHOT_BYTES, Projection, ProjectionError,
    ProjectionSnapshot, ReactiveLog, Reducer, ReducerError, ReducerIdentity, SnapshotError,
    StateCodec,
};
use pliego_log::{EventCatalogBuilder, EventSchema, Log, SealedEventCatalog};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Delta {
    amount: i64,
    mode: String,
}

impl EventSchema for Delta {
    const KIND: &'static str = "app_test_delta";
    const VERSION: u32 = 2;
    const SCHEMA_ID: &'static str = "test.delta/v2";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DeltaV1 {
    amount: i64,
}

impl EventSchema for DeltaV1 {
    const KIND: &'static str = Delta::KIND;
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "test.delta/v1";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AlternateDelta {
    amount: i64,
    mode: String,
}

impl EventSchema for AlternateDelta {
    const KIND: &'static str = Delta::KIND;
    const VERSION: u32 = Delta::VERSION;
    const SCHEMA_ID: &'static str = "test.delta-alternate/v2";
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Counter {
    value: i64,
    seen: u64,
}

fn catalog() -> SealedEventCatalog<Delta> {
    let mut builder = EventCatalogBuilder::new();
    builder
        .register_current::<Delta, _>("test.delta/current-map/1", |event| event)
        .unwrap();
    builder.seal().unwrap()
}

fn alternate_catalog() -> SealedEventCatalog<Delta> {
    let mut builder = EventCatalogBuilder::new();
    builder
        .register_current::<AlternateDelta, _>("test.delta/alternate-current-map/1", |event| {
            Delta {
                amount: event.amount,
                mode: event.mode,
            }
        })
        .unwrap();
    builder.seal().unwrap()
}

fn panicking_catalog() -> SealedEventCatalog<Delta> {
    let mut builder = EventCatalogBuilder::new();
    builder
        .register_current::<Delta, _>("test.delta/current-map/1", |event| event)
        .unwrap()
        .register_upcaster::<DeltaV1, Delta, _>("test.delta.panic-1-to-2", |event| {
            if event.amount == 7 {
                panic!("injected upcaster panic");
            }
            Ok(Delta {
                amount: event.amount,
                mode: "ok".to_owned(),
            })
        })
        .unwrap();
    builder.seal().unwrap()
}

fn versioned_catalog() -> SealedEventCatalog<Delta> {
    let mut builder = EventCatalogBuilder::new();
    builder
        .register_current::<Delta, _>("test.delta/current-map/1", |event| event)
        .unwrap()
        .register_upcaster::<DeltaV1, Delta, _>("test.delta.1-to-2", |event| {
            Ok(Delta {
                amount: event.amount,
                mode: "ok".to_owned(),
            })
        })
        .unwrap();
    builder.seal().unwrap()
}

fn identity(revision: u64) -> ReducerIdentity {
    ReducerIdentity::from_serializable_config(
        "test.counter",
        revision,
        &serde_json::json!({ "mode": "sum" }),
    )
    .unwrap()
}

fn reducer(revision: u64) -> Reducer<Counter, Delta> {
    Reducer::new(identity(revision), |state: &mut Counter, event: &Delta| {
        match event.mode.as_str() {
            "reject" => return Err(ReducerError::new("injected rejection")),
            "panic" => panic!("injected reducer panic"),
            _ => {}
        }
        state.value += event.amount;
        state.seen += 1;
        Ok(())
    })
}

fn events(values: impl IntoIterator<Item = (i64, &'static str)>) -> Vec<Delta> {
    values
        .into_iter()
        .map(|(amount, mode)| Delta {
            amount,
            mode: mode.to_owned(),
        })
        .collect()
}

fn log_of(events: &[Delta]) -> Log {
    let mut log = Log::new();
    for event in events {
        log.append_typed(event).unwrap();
    }
    log
}

fn mixed_log(events: &[Delta], seed: u64) -> Log {
    let mut log = Log::new();
    for (index, event) in events.iter().enumerate() {
        if (seed ^ index as u64).count_ones() % 2 == 0 {
            log.append_typed(&DeltaV1 {
                amount: event.amount,
            })
            .unwrap();
        } else {
            log.append_typed(event).unwrap();
        }
    }
    log
}

fn projection(log: Log) -> Projection<Counter, Delta> {
    Projection::new(
        ReactiveLog::from_log(log),
        Counter::default(),
        catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    )
    .unwrap()
}

#[test]
fn r3_a09_snapshot_binds_history_schema_reducer_codec_and_state() {
    let raw = log_of(&events([(2, "ok"), (-1, "ok")]));
    let expected_cursor = raw.cursor();
    let live = projection(raw);
    let snapshot = live.snapshot().unwrap();
    assert_eq!(snapshot.history(), &expected_cursor);
    assert_eq!(snapshot.schema_set_digest(), &catalog().schema_set_digest());
    assert_eq!(snapshot.reducer(), &identity(3));
    assert_eq!(snapshot.codec_id(), CANONICAL_JSON_CODEC_ID);
    assert_ne!(snapshot.state_digest(), &[0; 32]);
    assert_ne!(snapshot.snapshot_digest(), snapshot.state_digest());
    assert_eq!(ProjectionSnapshot::decode(&snapshot.encode()), Ok(snapshot));
}

#[test]
fn r3_a10_decoder_rejects_every_truncation_trailing_and_oversize_input() {
    let snapshot = projection(log_of(&events([(1, "ok")]))).snapshot().unwrap();
    let bytes = snapshot.encode();
    for cut in 0..bytes.len() {
        assert!(
            ProjectionSnapshot::decode(&bytes[..cut]).is_err(),
            "accepted truncation at {cut}"
        );
    }
    let mut trailing = bytes;
    trailing.extend_from_slice(&[1, 2, 3]);
    assert_eq!(
        ProjectionSnapshot::decode(&trailing),
        Err(SnapshotError::TrailingBytes(3))
    );
    let oversized = vec![0; MAX_PROJECTION_SNAPSHOT_BYTES + 1];
    assert_eq!(
        ProjectionSnapshot::decode(&oversized),
        Err(SnapshotError::TooLarge {
            actual: MAX_PROJECTION_SNAPSHOT_BYTES + 1,
            maximum: MAX_PROJECTION_SNAPSHOT_BYTES,
        })
    );
}

#[test]
fn r3_a11_state_and_snapshot_corruption_fail_closed() {
    let snapshot = projection(log_of(&events([(8, "ok")]))).snapshot().unwrap();
    let bytes = snapshot.encode();
    let marker = br#"{"seen":1,"value":8}"#;
    let state_offset = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap();
    let mut corrupt_state = bytes.clone();
    corrupt_state[state_offset + marker.len() - 2] ^= 1;
    assert_eq!(
        ProjectionSnapshot::decode(&corrupt_state),
        Err(SnapshotError::StateDigestMismatch)
    );
    let mut corrupt_envelope_digest = bytes;
    let last = corrupt_envelope_digest.len() - 1;
    corrupt_envelope_digest[last] ^= 1;
    assert_eq!(
        ProjectionSnapshot::decode(&corrupt_envelope_digest),
        Err(SnapshotError::SnapshotDigestMismatch)
    );
}

#[test]
fn r3_a12_restore_rejects_schema_reducer_and_codec_mismatch() {
    let events = events([(2, "ok")]);
    let raw = log_of(&events);
    let snapshot = projection(raw.clone()).snapshot().unwrap();
    let schema = Projection::restore(
        ReactiveLog::from_log(raw.clone()),
        snapshot.clone(),
        alternate_catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    );
    assert!(matches!(
        schema,
        Err(ProjectionError::SchemaSetMismatch { .. })
    ));
    let revision = Projection::restore(
        ReactiveLog::from_log(raw.clone()),
        snapshot.clone(),
        catalog(),
        reducer(4),
        CanonicalJsonCodec::default(),
    );
    assert!(matches!(
        revision,
        Err(ProjectionError::ReducerMismatch { .. })
    ));
    let codec = Projection::restore(
        ReactiveLog::from_log(raw),
        snapshot,
        catalog(),
        reducer(3),
        OtherCodec,
    );
    assert!(matches!(codec, Err(ProjectionError::CodecMismatch { .. })));
}

#[test]
fn restore_binds_codec_configuration_not_only_its_name() {
    let raw = log_of(&events([(5, "ok")]));
    let source = Projection::new(
        ReactiveLog::from_log(raw.clone()),
        Counter::default(),
        catalog(),
        reducer(3),
        OffsetCodec { offset: 0 },
    )
    .unwrap();
    let snapshot = source.snapshot().unwrap();
    assert!(matches!(
        Projection::restore(
            ReactiveLog::from_log(raw),
            snapshot,
            catalog(),
            reducer(3),
            OffsetCodec { offset: 10 },
        ),
        Err(ProjectionError::CodecMismatch { .. })
    ));
}

#[test]
fn r3_a13_restore_rejects_noncanonical_codec_output() {
    let raw = log_of(&events([(2, "ok")]));
    let snapshot = projection(raw.clone()).snapshot().unwrap();
    let restored = Projection::restore(
        ReactiveLog::from_log(raw),
        snapshot,
        catalog(),
        reducer(3),
        NonCanonicalCodec,
    );
    assert!(matches!(restored, Err(ProjectionError::NonCanonicalState)));
}

#[test]
fn r3_a14_restore_rejects_fork_at_snapshot_position() {
    let left = log_of(&events([(1, "ok"), (2, "ok")]));
    let right = log_of(&events([(1, "ok"), (99, "ok")]));
    let snapshot = projection(left).snapshot().unwrap();
    let result = Projection::restore(
        ReactiveLog::from_log(right),
        snapshot,
        catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    );
    assert!(matches!(result, Err(ProjectionError::Cursor(_))));
}

#[test]
fn r3_a15_restore_rejects_snapshot_ahead_of_history() {
    let long = log_of(&events([(1, "ok"), (2, "ok"), (3, "ok")]));
    let short = log_of(&events([(1, "ok")]));
    let snapshot = projection(long).snapshot().unwrap();
    let result = Projection::restore(
        ReactiveLog::from_log(short),
        snapshot,
        catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    );
    assert!(matches!(result, Err(ProjectionError::Cursor(_))));
}

#[test]
fn r3_a16_reducer_err_discards_the_whole_batch() {
    let raw = log_of(&events([(5, "ok"), (7, "reject"), (9, "ok")]));
    let projection = projection(raw);
    assert!(matches!(
        projection.try_get(),
        Err(ProjectionError::Reducer { sequence: 1, .. })
    ));
    assert_eq!(projection.stable_state(), Counter::default());
    assert_eq!(projection.stable_history().position, 0);
    assert_eq!(projection.events_folded(), 0);
}

#[test]
fn r3_a17_reducer_panic_discards_the_whole_batch_and_is_reported() {
    let raw = log_of(&events([(5, "ok"), (7, "panic"), (9, "ok")]));
    let projection = projection(raw);
    let result = catch_unwind(AssertUnwindSafe(|| projection.try_get()));
    assert!(result.is_ok(), "reducer panic escaped the transaction");
    assert!(matches!(
        result.unwrap(),
        Err(ProjectionError::ReducerPanicked { sequence: 1 })
    ));
    assert_eq!(projection.stable_state(), Counter::default());
    assert_eq!(projection.stable_history().position, 0);
    assert_eq!(projection.events_folded(), 0);
}

#[test]
fn r3_a18_schema_err_and_panic_never_publish_candidate_state() {
    let mut unknown_raw = Log::new();
    unknown_raw.append_typed(&DeltaV1 { amount: 5 }).unwrap();
    let unknown = Projection::new(
        ReactiveLog::from_log(unknown_raw),
        Counter::default(),
        catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    )
    .unwrap();
    assert!(matches!(
        unknown.try_get(),
        Err(ProjectionError::Schema { sequence: 0, .. })
    ));
    assert_eq!(unknown.stable_state(), Counter::default());
    assert_eq!(unknown.stable_history().position, 0);

    let mut panic_raw = Log::new();
    panic_raw.append_typed(&DeltaV1 { amount: 5 }).unwrap();
    panic_raw.append_typed(&DeltaV1 { amount: 7 }).unwrap();
    let projection = Projection::new(
        ReactiveLog::from_log(panic_raw),
        Counter::default(),
        panicking_catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    )
    .unwrap();
    assert!(matches!(
        projection.try_get(),
        Err(ProjectionError::SchemaPanicked { sequence: 1 })
    ));
    assert_eq!(projection.stable_state(), Counter::default());
    assert_eq!(projection.stable_history().position, 0);
}

#[test]
fn codec_rejection_and_panic_never_publish_candidate_state() {
    let initial = Counter { value: 1, seen: 0 };
    assert!(matches!(
        Projection::new(
            ReactiveLog::new(),
            initial,
            catalog(),
            reducer(3),
            CandidateCodec::Reject,
        ),
        Err(ProjectionError::Codec(CodecError::Encode(_)))
    ));

    for codec in [CandidateCodec::Reject, CandidateCodec::Panic] {
        let projection = Projection::new(
            ReactiveLog::from_log(log_of(&events([(1, "ok")]))),
            Counter::default(),
            catalog(),
            reducer(3),
            codec,
        )
        .unwrap();
        let result = catch_unwind(AssertUnwindSafe(|| projection.try_get()));
        assert!(result.is_ok(), "codec panic escaped the projection");
        assert!(matches!(
            result.unwrap(),
            Err(ProjectionError::Codec(_)) | Err(ProjectionError::CodecPanicked { .. })
        ));
        assert_eq!(projection.stable_state(), Counter::default());
        assert_eq!(projection.stable_history().position, 0);
        assert_eq!(projection.events_folded(), 0);
    }
}

#[test]
fn codec_global_limit_is_enforced_before_candidate_publication() {
    let projection = Projection::new(
        ReactiveLog::from_log(log_of(&events([(1, "ok")]))),
        Counter::default(),
        catalog(),
        reducer(3),
        CandidateCodec::Oversize,
    )
    .unwrap();
    assert!(matches!(
        projection.try_get(),
        Err(ProjectionError::Codec(CodecError::TooLarge {
            actual,
            maximum: MAX_CANONICAL_STATE_BYTES,
        })) if actual == MAX_CANONICAL_STATE_BYTES + 1
    ));
    assert_eq!(projection.stable_state(), Counter::default());
    assert_eq!(projection.stable_history().position, 0);
}

#[test]
fn snapshot_reuses_the_atomically_committed_state_bytes() {
    let forbid_encode = Rc::new(Cell::new(false));
    let projection = Projection::new(
        ReactiveLog::from_log(log_of(&events([(3, "ok")]))),
        Counter::default(),
        catalog(),
        reducer(3),
        GateCodec {
            forbid_encode: Rc::clone(&forbid_encode),
        },
    )
    .unwrap();
    assert_eq!(projection.get().value, 3);
    forbid_encode.set(true);
    let snapshot = catch_unwind(AssertUnwindSafe(|| projection.snapshot()));
    assert!(snapshot.is_ok(), "snapshot invoked codec after commit");
    assert!(snapshot.unwrap().is_ok());
}

#[test]
fn dropping_projection_releases_its_reactive_closure() {
    let dropped = Rc::new(Cell::new(false));
    {
        let probe = DropProbe(Rc::clone(&dropped));
        let reducer = Reducer::new(identity(3), move |_: &mut Counter, _: &Delta| {
            let _keep_probe_alive = &probe;
            Ok(())
        });
        let projection = Projection::new(
            ReactiveLog::new(),
            Counter::default(),
            catalog(),
            reducer,
            CanonicalJsonCodec::default(),
        )
        .unwrap();
        projection.sync().unwrap();
        assert!(!dropped.get());
    }
    assert!(dropped.get(), "projection drop retained its memo closure");
}

#[test]
fn r3_a19_restore_folds_exactly_the_tail() {
    let all = events((0..25).map(|value| (value, "ok")));
    let prefix = log_of(&all[..17]);
    let snapshot = projection(prefix.clone()).snapshot().unwrap();
    let mut full = prefix;
    for event in &all[17..] {
        full.append_typed(event).unwrap();
    }
    let restored = Projection::restore(
        ReactiveLog::from_log(full),
        snapshot,
        catalog(),
        reducer(3),
        CanonicalJsonCodec::default(),
    )
    .unwrap();
    assert_eq!(restored.events_folded(), 8);
    assert_eq!(restored.stable_history().position, 25);
    assert_eq!(restored.get().seen, 25);
}

#[test]
fn r3_a20_live_genesis_and_snapshot_tail_are_equal_for_deterministic_cases() {
    for seed in 0_u64..16 {
        let generated = generated_events(seed, 24);
        let full = mixed_log(&generated, seed);
        let live = Projection::new(
            ReactiveLog::from_log(full.clone()),
            Counter::default(),
            versioned_catalog(),
            reducer(3),
            CanonicalJsonCodec::default(),
        )
        .unwrap();
        let expected = live.get();
        let replay = Projection::new(
            ReactiveLog::from_log(full.clone()),
            Counter::default(),
            versioned_catalog(),
            reducer(3),
            CanonicalJsonCodec::default(),
        )
        .unwrap();
        assert_eq!(expected, replay.get(), "seed={seed}");
        for cut in 0..=generated.len() {
            let prefix = mixed_log(&generated[..cut], seed);
            let snapshot_source = Projection::new(
                ReactiveLog::from_log(prefix),
                Counter::default(),
                versioned_catalog(),
                reducer(3),
                CanonicalJsonCodec::default(),
            )
            .unwrap();
            let snapshot = snapshot_source.snapshot().unwrap();
            let restored = Projection::restore(
                ReactiveLog::from_log(full.clone()),
                snapshot,
                versioned_catalog(),
                reducer(3),
                CanonicalJsonCodec::default(),
            )
            .unwrap();
            assert_eq!(restored.get(), expected, "seed={seed}, cut={cut}");
            assert_eq!(
                restored.events_folded(),
                (generated.len() - cut) as u64,
                "seed={seed}, cut={cut}"
            );
            restored.dispose();
            snapshot_source.dispose();
        }
        replay.dispose();
        live.dispose();
    }
}

#[test]
fn r3_a21_snapshot_creation_refuses_a_rejected_tail() {
    let projection = projection(log_of(&events([(1, "reject")])));
    assert!(matches!(
        projection.snapshot(),
        Err(ProjectionError::Reducer { sequence: 0, .. })
    ));
    assert_eq!(projection.stable_history().position, 0);
}

#[test]
fn r3_a22_sealed_catalog_upcasts_version_mix_before_reduction() {
    let catalog = versioned_catalog();
    let expected_digest = catalog.schema_set_digest();
    let mut raw = Log::new();
    raw.append_typed(&DeltaV1 { amount: 3 }).unwrap();
    raw.append_typed(&Delta {
        amount: 4,
        mode: "ok".to_owned(),
    })
    .unwrap();
    let projection = Projection::new(
        ReactiveLog::from_log(raw),
        Counter::default(),
        catalog,
        reducer(3),
        CanonicalJsonCodec::default(),
    )
    .unwrap();
    assert_eq!(projection.get().value, 7);
    assert_eq!(
        projection.snapshot().unwrap().schema_set_digest(),
        &expected_digest
    );
}

#[test]
fn r3_a23_live_replay_is_invariant_across_batch_partitions() {
    let generated = generated_events(0xC0FFEE, 53);
    let expected = projection(log_of(&generated)).get();
    for width in [1_usize, 2, 3, 5, 8, 13, 53] {
        let log = ReactiveLog::new();
        let live = Projection::new(
            log,
            Counter::default(),
            catalog(),
            reducer(3),
            CanonicalJsonCodec::default(),
        )
        .unwrap();
        for batch in generated.chunks(width) {
            for event in batch {
                log.append_typed(event).unwrap();
            }
            live.sync().unwrap();
        }
        assert_eq!(live.get(), expected, "partition width={width}");
        assert_eq!(live.events_folded(), generated.len() as u64);
        live.dispose();
    }
}

fn generated_events(mut state: u64, count: usize) -> Vec<Delta> {
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            Delta {
                amount: ((state >> 32) % 19) as i64 - 9,
                mode: "ok".to_owned(),
            }
        })
        .collect()
}

struct OtherCodec;

impl StateCodec<Counter> for OtherCodec {
    fn identity(&self) -> CodecIdentity {
        CodecIdentity::new("test/other-codec", 1, [1; 32]).unwrap()
    }

    fn encode(&self, state: &Counter) -> Result<Vec<u8>, CodecError> {
        CanonicalJsonCodec::default().encode(state)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Counter, CodecError> {
        CanonicalJsonCodec::default().decode(bytes)
    }
}

struct NonCanonicalCodec;

impl StateCodec<Counter> for NonCanonicalCodec {
    fn identity(&self) -> CodecIdentity {
        <CanonicalJsonCodec as StateCodec<Counter>>::identity(&CanonicalJsonCodec::default())
    }

    fn encode(&self, state: &Counter) -> Result<Vec<u8>, CodecError> {
        let mut bytes = CanonicalJsonCodec::default().encode(state)?;
        bytes.push(b' ');
        Ok(bytes)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Counter, CodecError> {
        CanonicalJsonCodec::default().decode(bytes)
    }
}

#[derive(Clone, Copy)]
enum CandidateCodec {
    Reject,
    Panic,
    Oversize,
}

impl StateCodec<Counter> for CandidateCodec {
    fn identity(&self) -> CodecIdentity {
        let tag = match self {
            Self::Reject => 1,
            Self::Panic => 2,
            Self::Oversize => 3,
        };
        CodecIdentity::new("test/candidate-codec", 1, [tag; 32]).unwrap()
    }

    fn encode(&self, state: &Counter) -> Result<Vec<u8>, CodecError> {
        if state.value == 0 {
            return CanonicalJsonCodec::default().encode(state);
        }
        match self {
            Self::Reject => Err(CodecError::Encode("injected rejection".to_owned())),
            Self::Panic => panic!("injected codec panic"),
            Self::Oversize => Ok(vec![0; MAX_CANONICAL_STATE_BYTES + 1]),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<Counter, CodecError> {
        CanonicalJsonCodec::default().decode(bytes)
    }
}

struct GateCodec {
    forbid_encode: Rc<Cell<bool>>,
}

struct DropProbe(Rc<Cell<bool>>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

struct OffsetCodec {
    offset: i64,
}

impl StateCodec<Counter> for OffsetCodec {
    fn identity(&self) -> CodecIdentity {
        CodecIdentity::from_serializable_config("test/offset-codec", 1, &self.offset).unwrap()
    }

    fn encode(&self, state: &Counter) -> Result<Vec<u8>, CodecError> {
        let mut encoded = state.clone();
        encoded.value += self.offset;
        CanonicalJsonCodec::default().encode(&encoded)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Counter, CodecError> {
        let mut decoded: Counter = CanonicalJsonCodec::default().decode(bytes)?;
        decoded.value -= self.offset;
        Ok(decoded)
    }
}

impl StateCodec<Counter> for GateCodec {
    fn identity(&self) -> CodecIdentity {
        CodecIdentity::new("test/gate-codec", 1, [4; 32]).unwrap()
    }

    fn encode(&self, state: &Counter) -> Result<Vec<u8>, CodecError> {
        assert!(!self.forbid_encode.get(), "encode called after commit");
        CanonicalJsonCodec::default().encode(state)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Counter, CodecError> {
        CanonicalJsonCodec::default().decode(bytes)
    }
}
