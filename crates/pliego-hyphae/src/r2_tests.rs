use std::cell::Cell;
use std::collections::BTreeMap;

use pliego_log::{EventCatalogBuilder, EventSchema, Log, SealedEventCatalog};
use serde::{Deserialize, Serialize};

use super::*;

const ID_1: &str = "01890f3e-9b4a-7cc0-8a1a-0123456789ab";
const ID_2: &str = "01890f3e-9b4a-7cc1-8a1a-0123456789ab";
const BATCH_ID: &str = "01890f3e-9b4a-7cc2-8a1a-0123456789ab";
const REQUEST_ID: &str = "01890f3e-9b4a-7cc3-8a1a-0123456789ab";
const REQUEST_ID_2: &str = "01890f3e-9b4a-7cc4-8a1a-0123456789ab";
const STREAM: &str = "project:alpha";
const AUTHORITY_A: &str = "hyphae-primary";
const AUTHORITY_B: &str = "hyphae-secondary";
const KEY_A: &str = "key-a";
const KEY_A_ROTATED: &str = "key-a-rotated";
const SIGNATURE_PLACEHOLDER: &str = "cGxhY2Vob2xkZXItc2lnbmF0dXJl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TaskAdded(String);

impl EventSchema for TaskAdded {
    const KIND: &'static str = "app_task_added";
    const VERSION: u32 = 1;
    const SCHEMA_ID: &'static str = "pliego.test/task-added/1";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
enum FixtureEvent {
    TaskAdded(TaskAdded),
}

fn fixture_catalog() -> SealedEventCatalog<FixtureEvent> {
    let mut builder = EventCatalogBuilder::new();
    builder
        .register_current::<TaskAdded, _>(
            "pliego.test/fixture-task-added-map/1",
            FixtureEvent::TaskAdded,
        )
        .unwrap();
    builder.seal().unwrap()
}

fn authority(value: &str) -> AuthorityId {
    AuthorityId::try_new(value).unwrap()
}

fn payload_signature(key_id: &str, payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pliego-hyphae/test-signature/v1");
    hasher.update(key_id.as_bytes());
    hasher.update(payload);
    let digest: [u8; 32] = hasher.finalize().into();
    hex(&digest)
}

fn payload_digest(payload: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(payload).into();
    hex(&digest)
}

fn envelopes() -> Vec<EventEnvelope> {
    let mut log = Log::new();
    log.append_typed(&TaskAdded("first".to_owned())).unwrap();
    log.append_typed(&TaskAdded("second".to_owned())).unwrap();
    [ID_1, ID_2]
        .into_iter()
        .zip(log.events())
        .map(|(id, event)| {
            EventEnvelope::from_local_event(event, id, STREAM, "2026-07-12T20:00:00Z").unwrap()
        })
        .collect()
}

fn batch() -> AppendBatch {
    AppendBatch {
        protocol: PROTOCOL_V2.to_owned(),
        batch_id: BATCH_ID.to_owned(),
        stream_id: STREAM.to_owned(),
        expected_cursor: StreamCursor::genesis(),
        events: envelopes(),
    }
}

fn durable_hash(previous: &str, sequence: u64, envelope_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pliego-hyphae/test-durable/v1");
    hasher.update(previous.as_bytes());
    hasher.update(sequence.to_be_bytes());
    hasher.update(envelope_hash.as_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    hex(&digest)
}

fn sign_receipt(receipt: &mut Receipt) {
    receipt.signature = SIGNATURE_PLACEHOLDER.to_owned();
    let payload = receipt.signing_payload().unwrap();
    receipt.signature = payload_signature(&receipt.key_id, &payload);
}

fn response_for_with_keys(
    batch: &AppendBatch,
    authority_id: &str,
    attestation_key: &str,
    receipt_keys: &[&str],
) -> AppendResponse {
    assert_eq!(receipt_keys.len(), batch.events.len());
    let mut previous = batch.expected_cursor.head_hash.clone();
    let mut unsigned = Vec::new();
    for (index, (event, key_id)) in batch.events.iter().zip(receipt_keys).enumerate() {
        let sequence = batch.expected_cursor.position + index as u64 + 1;
        let envelope_hash = event.wire_hash().unwrap();
        let server_hash = durable_hash(&previous, sequence, &envelope_hash);
        unsigned.push((
            event,
            Receipt {
                client_event_id: event.client_event_id.clone(),
                stream_id: batch.stream_id.clone(),
                envelope_hash,
                server_seq: sequence,
                server_hash: server_hash.clone(),
                server_prev_hash: previous,
                journal_head: String::new(),
                committed_at: "2026-07-12T20:00:01Z".to_owned(),
                key_id: (*key_id).to_owned(),
                signature: SIGNATURE_PLACEHOLDER.to_owned(),
            },
        ));
        previous = server_hash;
    }
    let journal_head = previous;
    let mut receipts = Vec::new();
    for (_, mut receipt) in unsigned {
        receipt.journal_head.clone_from(&journal_head);
        sign_receipt(&mut receipt);
        receipts.push(receipt);
    }
    let mut response = AppendResponse {
        protocol: PROTOCOL_V2.to_owned(),
        batch_id: batch.batch_id.clone(),
        stream_id: batch.stream_id.clone(),
        receipts,
        next_cursor: StreamCursor {
            position: batch.expected_cursor.position + batch.events.len() as u64,
            head_hash: journal_head,
        },
        attestation: AppendAttestation {
            authority_id: authority(authority_id),
            committed_at: "2026-07-12T20:00:02Z".to_owned(),
            key_id: attestation_key.to_owned(),
            signature: SIGNATURE_PLACEHOLDER.to_owned(),
        },
    };
    sign_append(batch, &mut response);
    response
}

fn response_for(batch: &AppendBatch) -> AppendResponse {
    response_for_with_keys(batch, AUTHORITY_A, KEY_A, &[KEY_A, KEY_A_ROTATED])
}

fn sign_append(batch: &AppendBatch, response: &mut AppendResponse) {
    response.attestation.signature = SIGNATURE_PLACEHOLDER.to_owned();
    let payload = response
        .attestation
        .signing_payload(batch, response)
        .unwrap();
    response.attestation.signature = payload_signature(&response.attestation.key_id, &payload);
}

fn request(after: StreamCursor, snapshot: SnapshotSelection) -> PullRequest {
    PullRequest {
        protocol: PROTOCOL_V2.to_owned(),
        request_id: REQUEST_ID.to_owned(),
        stream_id: STREAM.to_owned(),
        after,
        snapshot,
        selection: PullSelection::WholeStream,
        limit: 10,
    }
}

fn page_for(
    request: &PullRequest,
    events: Vec<AcceptedEvent>,
    next_cursor: StreamCursor,
    snapshot_cursor: StreamCursor,
    authority_id: &str,
    key_id: &str,
) -> PullPage {
    let mut page = PullPage {
        protocol: PROTOCOL_V2.to_owned(),
        stream_id: request.stream_id.clone(),
        events,
        complete: next_cursor == snapshot_cursor,
        next_cursor,
        snapshot_cursor,
        attestation: PageAttestation {
            authority_id: authority(authority_id),
            issued_at: "2026-07-12T20:00:03Z".to_owned(),
            key_id: key_id.to_owned(),
            signature: SIGNATURE_PLACEHOLDER.to_owned(),
        },
    };
    sign_page(request, &mut page);
    page
}

fn sign_page(request: &PullRequest, page: &mut PullPage) {
    page.attestation.signature = SIGNATURE_PLACEHOLDER.to_owned();
    let payload = page.attestation.signing_payload(request, page).unwrap();
    page.attestation.signature = payload_signature(&page.attestation.key_id, &payload);
}

fn pull_fixture() -> (PullRequest, PullPage) {
    let batch = batch();
    let response = response_for(&batch);
    let request = request(batch.expected_cursor.clone(), SnapshotSelection::Latest);
    let events = batch
        .events
        .into_iter()
        .zip(response.receipts)
        .map(|(envelope, receipt)| AcceptedEvent { envelope, receipt })
        .collect();
    let page = page_for(
        &request,
        events,
        response.next_cursor.clone(),
        response.next_cursor,
        AUTHORITY_A,
        KEY_A,
    );
    (request, page)
}

#[derive(Default)]
struct TestVerifier {
    calls: Cell<usize>,
}

impl ReceiptVerifier for TestVerifier {
    fn verify(
        &self,
        context: VerificationContext<'_>,
        signing_payload: &[u8],
        signature: &str,
    ) -> Result<AuthorityId, VerificationError> {
        self.calls.set(self.calls.get() + 1);
        match context.key_id() {
            "unknown" => return Err(VerificationError::UnknownKey),
            "revoked" => return Err(VerificationError::RevokedKey),
            "unauthorized" => return Err(VerificationError::UnauthorizedStream),
            "unavailable" => return Err(VerificationError::Unavailable("offline".to_owned())),
            _ => {}
        }
        if context.stream_id() != STREAM {
            return Err(VerificationError::UnauthorizedStream);
        }
        if signature != payload_signature(context.key_id(), signing_payload) {
            return Err(VerificationError::InvalidSignature);
        }
        let resolved = match context.key_id() {
            KEY_A | KEY_A_ROTATED => AUTHORITY_A,
            "key-b" => AUTHORITY_B,
            other => panic!("unhandled test key {other}"),
        };
        Ok(authority(resolved))
    }
}

#[derive(Default)]
struct RecordingSink {
    calls: usize,
    ids: Vec<String>,
    authorities: Vec<String>,
    fail: bool,
}

impl ReplaySink for RecordingSink {
    fn apply_batch(&mut self, events: &[VerifiedAcceptedEvent]) -> Result<(), String> {
        self.calls += 1;
        if self.fail {
            return Err("reducer rejected transaction".to_owned());
        }
        self.ids.extend(
            events
                .iter()
                .map(|event| event.envelope().client_event_id.clone()),
        );
        self.authorities.extend(
            events
                .iter()
                .map(|event| event.authority().as_str().to_owned()),
        );
        Ok(())
    }
}

fn verified(state: &ReplayState, request: PullRequest, page: PullPage) -> VerifiedPullPage {
    let catalog = fixture_catalog();
    UntrustedPullPage::new(request, page)
        .validate(state)
        .unwrap()
        .verify(&TestVerifier::default(), &catalog)
        .unwrap()
}

#[test]
fn stream_segments_reject_empty_dot_and_dot_dot_only() {
    for valid in ["a", "a/b", "a..b/c.d", "project:alpha/items_1"] {
        assert!(validate_stream_id(valid).is_ok(), "rejected {valid}");
    }
    for invalid in ["", "/a", "a/", "a//b", ".", "..", "a/./b", "a/../b"] {
        assert!(validate_stream_id(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn timestamps_hashes_and_cursors_remain_strict() {
    for valid in [
        "2026-07-12T20:00:00Z",
        "2016-12-31t23:59:60z",
        "2026-07-12T20:00:00.123456+05:30",
    ] {
        assert!(validate_timestamp(valid).is_ok(), "rejected {valid}");
    }
    for invalid in [
        "2026-02-29T20:00:00Z",
        "2026-13-12T20:00:00Z",
        "2026-07-12T24:00:00Z",
        "2026-07-12T20:60:00Z",
        "2026-07-12T20:00:61Z",
        "2026-07-12T20:00:00Ztrailing",
    ] {
        assert!(validate_timestamp(invalid).is_err(), "accepted {invalid}");
    }
    assert!(validate_hash(&"A".repeat(64)).is_err());
    assert!(validate_hash(&"a".repeat(63)).is_err());
    assert!(StreamCursor::genesis().validate().is_ok());
    assert!(
        StreamCursor {
            position: 0,
            head_hash: "a".repeat(64),
        }
        .validate()
        .is_err()
    );
    assert!(
        StreamCursor {
            position: 1,
            head_hash: EMPTY_STREAM_HASH.to_owned(),
        }
        .validate()
        .is_err()
    );
}

#[test]
fn batch_rejects_duplicates_reordering_broken_links_and_unknown_fields() {
    let mut duplicate = batch();
    duplicate.events[1].client_event_id = ID_1.to_owned();
    assert!(duplicate.validate().is_err());

    let mut reordered = batch();
    reordered.events.swap(0, 1);
    assert!(reordered.validate().is_err());

    let mut broken = batch();
    broken.events[1].local_prev_hash = EMPTY_STREAM_HASH.to_owned();
    assert!(broken.validate().is_err());

    let mut value = serde_json::to_value(batch()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("tenant_id".to_owned(), serde_json::json!("forged"));
    assert!(serde_json::from_value::<AppendBatch>(value).is_err());
}

#[test]
fn v2_rejects_v1_and_unknown_selective_replay() {
    let (mut request, _) = pull_fixture();
    request.protocol = "pliego-hyphae/1".to_owned();
    assert_eq!(request.validate().unwrap_err().field(), "protocol");

    let value = serde_json::json!({
        "protocol": PROTOCOL_V2,
        "request_id": REQUEST_ID,
        "stream_id": STREAM,
        "after": StreamCursor::genesis(),
        "snapshot": {"mode": "latest"},
        "selection": "by_kind",
        "limit": 10
    });
    assert!(serde_json::from_value::<PullRequest>(value).is_err());
}

#[test]
fn request_requires_uuid_v7_and_non_regressing_exact_snapshot() {
    let mut request = request(StreamCursor::genesis(), SnapshotSelection::Latest);
    request.request_id = ID_1.replace("7cc0", "4cc0");
    assert_eq!(request.validate().unwrap_err().field(), "request_id");

    request.request_id = REQUEST_ID.to_owned();
    request.after = StreamCursor {
        position: 2,
        head_hash: "a".repeat(64),
    };
    request.snapshot = SnapshotSelection::Exact(StreamCursor {
        position: 1,
        head_hash: "b".repeat(64),
    });
    assert_eq!(request.validate().unwrap_err().field(), "snapshot");
}

#[test]
fn authority_id_validates_constructor_and_deserialize() {
    assert_eq!(authority(AUTHORITY_A).as_str(), AUTHORITY_A);
    assert!(AuthorityId::try_new("bad/authority").is_err());
    assert!(serde_json::from_str::<AuthorityId>("\"bad/authority\"").is_err());
}

#[test]
fn append_response_requires_exact_one_based_sequences() {
    let batch = batch();
    let mut response = response_for(&batch);
    response.receipts[0].server_seq = 0;
    assert_eq!(
        response.validate_against(&batch).unwrap_err().field(),
        "server_seq"
    );

    let mut response = response_for(&batch);
    response.receipts[1].server_seq += 1;
    assert_eq!(
        response.validate_against(&batch).unwrap_err().field(),
        "server_seq"
    );
}

#[test]
fn append_response_rejects_cursor_batch_and_atomic_set_substitution() {
    let batch = batch();
    let mut response = response_for(&batch);
    response.next_cursor.position += 1;
    assert_eq!(
        response.validate_against(&batch).unwrap_err().field(),
        "next_cursor"
    );

    let response = response_for(&batch);
    let mut substituted = batch.clone();
    substituted.events[0].payload = "\"substituted\"".to_owned();
    assert_eq!(
        response.validate_against(&substituted).unwrap_err().field(),
        "receipts"
    );
}

#[test]
fn append_verification_accepts_key_rotation_within_one_authority() {
    let batch = batch();
    let response = response_for(&batch);
    response.validate_against(&batch).unwrap();
    let verifier = TestVerifier::default();
    let verified = ValidatedAppendResponse { batch, response }
        .verify(&verifier)
        .unwrap();
    assert_eq!(verified.authority().as_str(), AUTHORITY_A);
    assert_eq!(verified.next_cursor().position, 2);
    assert_eq!(verifier.calls.get(), 3);
}

#[test]
fn append_attestation_blocks_batch_substitution_and_authority_mismatch() {
    let batch = batch();
    let response = response_for(&batch);
    let validated = ValidatedAppendResponse {
        batch: batch.clone(),
        response: response.clone(),
    };
    let mut substituted = validated.batch.clone();
    substituted.batch_id = REQUEST_ID.to_owned();
    let payload = response
        .attestation
        .signing_payload(&substituted, &response)
        .unwrap();
    assert_ne!(
        payload_signature(KEY_A, &payload),
        response.attestation.signature
    );

    let mut mismatch = response_for_with_keys(&batch, AUTHORITY_A, "key-b", &[KEY_A, KEY_A]);
    sign_append(&batch, &mut mismatch);
    let error = ValidatedAppendResponse {
        batch,
        response: mismatch,
    }
    .verify(&TestVerifier::default())
    .unwrap_err();
    assert!(matches!(
        error,
        SyncError::Verification(VerificationError::AuthorityMismatch)
    ));
}

#[test]
fn append_attestation_rejects_receipt_set_mutation_before_receipt_verification() {
    let batch = batch();
    let mut response = response_for(&batch);
    response.receipts[0].signature = "b".repeat(64);
    let verifier = TestVerifier::default();
    let error = ValidatedAppendResponse { batch, response }
        .verify(&verifier)
        .unwrap_err();
    assert!(matches!(
        error,
        SyncError::Verification(VerificationError::InvalidSignature)
    ));
    assert_eq!(verifier.calls.get(), 1, "append attestation fails first");
}

#[derive(Default)]
struct IdempotentServer {
    committed: BTreeMap<String, AppendResponse>,
    durable_appends: usize,
    lose_first_ack: bool,
}

impl BatchTransport for IdempotentServer {
    fn append_batch(&mut self, batch: &AppendBatch) -> Result<AppendResponse, TransportError> {
        if let Some(response) = self.committed.get(&batch.batch_id) {
            return Ok(response.clone());
        }
        let response = response_for(batch);
        self.durable_appends += batch.events.len();
        self.committed
            .insert(batch.batch_id.clone(), response.clone());
        if self.lose_first_ack {
            self.lose_first_ack = false;
            return Err(TransportError::Retryable("ack lost".to_owned()));
        }
        Ok(response)
    }
}

#[test]
fn append_retry_returns_only_validated_until_authority_verification() {
    let batch = batch();
    let mut server = IdempotentServer {
        lose_first_ack: true,
        ..IdempotentServer::default()
    };
    let validated = append_with_retry(&mut server, &batch, 2).unwrap();
    assert_eq!(validated.response().receipts.len(), 2);
    assert_eq!(server.durable_appends, 2);
    assert_eq!(server.committed.len(), 1);
    assert_eq!(
        validated
            .verify(&TestVerifier::default())
            .unwrap()
            .next_cursor()
            .position,
        2
    );
}

#[test]
fn append_retry_never_retries_cursor_conflicts() {
    struct Conflict(u8);
    impl BatchTransport for Conflict {
        fn append_batch(&mut self, batch: &AppendBatch) -> Result<AppendResponse, TransportError> {
            self.0 += 1;
            Err(TransportError::CursorConflict {
                expected: batch.expected_cursor.clone(),
                actual: StreamCursor {
                    position: 9,
                    head_hash: "a".repeat(64),
                },
            })
        }
    }
    let mut server = Conflict(0);
    assert!(matches!(
        append_with_retry(&mut server, &batch(), 5),
        Err(SyncError::Transport(TransportError::CursorConflict { .. }))
    ));
    assert_eq!(server.0, 1);
}

#[test]
fn pull_page_rejects_sequence_gaps_and_zero() {
    let (request, mut page) = pull_fixture();
    page.events[0].receipt.server_seq = 0;
    assert_eq!(
        page.validate_against(&request).unwrap_err().field(),
        "server_seq"
    );

    let (request, mut page) = pull_fixture();
    page.events[1].receipt.server_seq += 1;
    assert_eq!(
        page.validate_against(&request).unwrap_err().field(),
        "server_seq"
    );
}

#[test]
fn page_completion_is_derived_from_fixed_snapshot() {
    let (request, mut page) = pull_fixture();
    page.complete = false;
    assert_eq!(
        page.validate_against(&request).unwrap_err().field(),
        "complete"
    );

    let (request, mut page) = pull_fixture();
    page.snapshot_cursor = StreamCursor {
        position: page.next_cursor.position + 1,
        head_hash: "f".repeat(64),
    };
    page.complete = false;
    sign_page(&request, &mut page);
    page.validate_against(&request).unwrap();

    page.snapshot_cursor.position = page.next_cursor.position;
    assert_eq!(
        page.validate_against(&request).unwrap_err().field(),
        "snapshot_cursor"
    );
}

#[test]
fn empty_page_is_attested_and_never_calls_sink() {
    let request = request(StreamCursor::genesis(), SnapshotSelection::Latest);
    let page = page_for(
        &request,
        Vec::new(),
        StreamCursor::genesis(),
        StreamCursor::genesis(),
        AUTHORITY_A,
        KEY_A,
    );
    let json = serde_json::to_value(&page).unwrap();
    let mut missing = json.as_object().unwrap().clone();
    missing.remove("attestation");
    assert!(serde_json::from_value::<PullPage>(missing.into()).is_err());

    let mut state = ReplayState::new(STREAM).unwrap();
    let verified = verified(&state, request, page);
    let mut sink = RecordingSink::default();
    let applied = verified.apply(&mut state, &mut sink).unwrap();
    assert_eq!(applied.applied_count(), 0);
    assert!(applied.complete());
    assert_eq!(sink.calls, 0);
}

#[test]
fn page_attestation_binds_request_snapshot_limit_completion_and_events() {
    let (request, page) = pull_fixture();
    let original = page.attestation.signing_payload(&request, &page).unwrap();

    let mut changed_request = request.clone();
    changed_request.request_id = REQUEST_ID_2.to_owned();
    let changed = page
        .attestation
        .signing_payload(&changed_request, &page)
        .unwrap();
    assert_ne!(original, changed);

    changed_request = request.clone();
    changed_request.limit += 1;
    let changed = page
        .attestation
        .signing_payload(&changed_request, &page)
        .unwrap();
    assert_ne!(original, changed);

    let mut changed_page = page.clone();
    changed_page.snapshot_cursor = StreamCursor {
        position: changed_page.next_cursor.position + 1,
        head_hash: "f".repeat(64),
    };
    changed_page.complete = false;
    let changed = changed_page
        .attestation
        .signing_payload(&request, &changed_page)
        .unwrap();
    assert_ne!(original, changed);

    changed_page = page.clone();
    changed_page.events[0].receipt.signature = "b".repeat(64);
    let changed = changed_page
        .attestation
        .signing_payload(&request, &changed_page)
        .unwrap();
    assert_ne!(original, changed);
}

#[test]
fn signed_request_mutation_fails_verification() {
    let (request, page) = pull_fixture();
    let mut substituted = request.clone();
    substituted.request_id = REQUEST_ID_2.to_owned();
    let state = ReplayState::new(STREAM).unwrap();
    let catalog = fixture_catalog();
    let error = UntrustedPullPage::new(substituted, page)
        .validate(&state)
        .unwrap()
        .verify(&TestVerifier::default(), &catalog)
        .unwrap_err();
    assert!(matches!(
        error,
        SyncError::Verification(VerificationError::InvalidSignature)
    ));
}

#[test]
fn signed_snapshot_and_completion_mutation_fails_verification() {
    let (request, mut page) = pull_fixture();
    page.snapshot_cursor = StreamCursor {
        position: page.next_cursor.position + 1,
        head_hash: "f".repeat(64),
    };
    page.complete = false;
    let state = ReplayState::new(STREAM).unwrap();
    let catalog = fixture_catalog();
    let error = UntrustedPullPage::new(request, page)
        .validate(&state)
        .unwrap()
        .verify(&TestVerifier::default(), &catalog)
        .unwrap_err();
    assert!(matches!(
        error,
        SyncError::Verification(VerificationError::InvalidSignature)
    ));
}

#[test]
fn authority_errors_are_structured_and_fail_closed() {
    for (key, expected) in [
        ("unknown", VerificationError::UnknownKey),
        ("revoked", VerificationError::RevokedKey),
        ("unauthorized", VerificationError::UnauthorizedStream),
        (
            "unavailable",
            VerificationError::Unavailable("offline".to_owned()),
        ),
    ] {
        let (request, mut page) = pull_fixture();
        page.attestation.key_id = key.to_owned();
        page.attestation.signature = SIGNATURE_PLACEHOLDER.to_owned();
        let state = ReplayState::new(STREAM).unwrap();
        let catalog = fixture_catalog();
        let error = UntrustedPullPage::new(request, page)
            .validate(&state)
            .unwrap()
            .verify(&TestVerifier::default(), &catalog)
            .unwrap_err();
        assert_eq!(error, SyncError::Verification(expected));
    }
}

#[test]
fn invalid_receipt_signature_never_produces_verified_page() {
    let (request, mut page) = pull_fixture();
    page.events[1].receipt.signature = "b".repeat(64);
    sign_page(&request, &mut page);
    let state = ReplayState::new(STREAM).unwrap();
    let verifier = TestVerifier::default();
    let catalog = fixture_catalog();
    let error = UntrustedPullPage::new(request, page)
        .validate(&state)
        .unwrap()
        .verify(&verifier, &catalog)
        .unwrap_err();
    assert!(matches!(
        error,
        SyncError::Verification(VerificationError::InvalidSignature)
    ));
    assert_eq!(
        verifier.calls.get(),
        3,
        "page and both receipts were reached"
    );
}

#[test]
fn unknown_event_version_fails_before_replay_sink_exists() {
    let catalog = fixture_catalog();
    assert!(EventVersionPolicy::validate(&catalog, "app_task_added", 1).is_ok());
    let catalog_error = EventVersionPolicy::validate(&catalog, "app_task_added", 2).unwrap_err();
    assert_eq!(catalog_error.kind(), "app_task_added");
    assert_eq!(catalog_error.schema_version(), 2);

    struct RejectAll;
    impl EventVersionPolicy for RejectAll {
        fn validate(&self, kind: &str, version: u32) -> Result<(), EventVersionError> {
            Err(EventVersionError::new(kind, version, "unsupported"))
        }
    }
    let (request, page) = pull_fixture();
    let state = ReplayState::new(STREAM).unwrap();
    let error = UntrustedPullPage::new(request, page)
        .validate(&state)
        .unwrap()
        .verify(&TestVerifier::default(), &RejectAll)
        .unwrap_err();
    assert!(matches!(error, SyncError::EventVersion(_)));
}

#[test]
fn verified_replay_applies_once_and_deduplicates_absolute_overlap() {
    let (request, page) = pull_fixture();
    let mut state = ReplayState::new(STREAM).unwrap();
    let mut sink = RecordingSink::default();
    let first = verified(&state, request.clone(), page.clone())
        .apply(&mut state, &mut sink)
        .unwrap();
    assert_eq!(first.applied_count(), 2);
    assert!(first.complete());
    assert_eq!(sink.calls, 1);
    assert_eq!(sink.authorities, vec![AUTHORITY_A, AUTHORITY_A]);

    let repeated = verified(&state, request, page)
        .apply(&mut state, &mut sink)
        .unwrap();
    assert_eq!(repeated.applied_count(), 0);
    assert_eq!(sink.calls, 1, "empty fresh set never reaches reducer");
    assert_eq!(state.dedupe_window_len(), 2);
}

#[test]
fn fork_verified_before_state_change_is_rejected_before_sink() {
    let (request_a, page_a) = pull_fixture();
    let initial = ReplayState::new(STREAM).unwrap();
    let verified_a = verified(&initial, request_a, page_a);

    let mut fork_batch = batch();
    fork_batch.events[0].payload = "\"fork\"".to_owned();
    let fork_response = response_for(&fork_batch);
    let fork_request = request(StreamCursor::genesis(), SnapshotSelection::Latest);
    let fork_events = fork_batch
        .events
        .into_iter()
        .zip(fork_response.receipts)
        .map(|(envelope, receipt)| AcceptedEvent { envelope, receipt })
        .collect();
    let fork_page = page_for(
        &fork_request,
        fork_events,
        fork_response.next_cursor.clone(),
        fork_response.next_cursor,
        AUTHORITY_A,
        KEY_A,
    );
    let verified_fork = verified(&initial, fork_request, fork_page);

    let mut state = initial;
    verified_a
        .apply(&mut state, &mut RecordingSink::default())
        .unwrap();
    let snapshot = state.clone();
    let mut sink = RecordingSink::default();
    let error = verified_fork.apply(&mut state, &mut sink).unwrap_err();
    assert!(matches!(error, SyncError::Validation(_)));
    assert_eq!(sink.calls, 0);
    assert_eq!(state, snapshot);
}

#[test]
fn stream_and_future_cursor_substitution_fail_before_verification() {
    let (mut request, mut page) = pull_fixture();
    request.stream_id = "project:other".to_owned();
    page.stream_id = request.stream_id.clone();
    for accepted in &mut page.events {
        accepted.envelope.stream_id = request.stream_id.clone();
        accepted.receipt.stream_id = request.stream_id.clone();
    }
    let state = ReplayState::new(STREAM).unwrap();
    assert!(
        UntrustedPullPage::new(request, page)
            .validate(&state)
            .is_err()
    );

    let (mut request, page) = pull_fixture();
    request.after = StreamCursor {
        position: 1,
        head_hash: "a".repeat(64),
    };
    assert!(
        UntrustedPullPage::new(request, page)
            .validate(&state)
            .is_err()
    );
}

#[test]
fn reducer_error_leaves_replay_state_unchanged() {
    let (request, page) = pull_fixture();
    let mut state = ReplayState::new(STREAM).unwrap();
    let before = state.clone();
    let verified = verified(&state, request, page);
    let mut sink = RecordingSink {
        fail: true,
        ..RecordingSink::default()
    };
    assert!(matches!(
        verified.apply(&mut state, &mut sink),
        Err(SyncError::Reducer(_))
    ));
    assert_eq!(sink.calls, 1);
    assert_eq!(state, before);
}

#[test]
fn authority_is_bound_to_replay_state() {
    let (initial_request, page) = pull_fixture();
    let mut state = ReplayState::new(STREAM).unwrap();
    verified(&state, initial_request, page)
        .apply(&mut state, &mut RecordingSink::default())
        .unwrap();

    let empty_request = request(state.cursor().clone(), SnapshotSelection::Latest);
    let empty_page = page_for(
        &empty_request,
        Vec::new(),
        state.cursor().clone(),
        state.cursor().clone(),
        AUTHORITY_B,
        "key-b",
    );
    let verified_b = verified(&state, empty_request, empty_page);
    let before = state.clone();
    let mut sink = RecordingSink::default();
    let error = verified_b.apply(&mut state, &mut sink).unwrap_err();
    assert!(matches!(
        error,
        SyncError::Verification(VerificationError::AuthorityMismatch)
    ));
    assert_eq!(sink.calls, 0);
    assert_eq!(state, before);
}

#[test]
fn exact_snapshot_continuation_cannot_change_cycle_head() {
    let batch = batch();
    let response = response_for(&batch);
    let first_request = request(StreamCursor::genesis(), SnapshotSelection::Latest);
    let first_event = AcceptedEvent {
        envelope: batch.events[0].clone(),
        receipt: response.receipts[0].clone(),
    };
    let first_cursor = StreamCursor {
        position: 1,
        head_hash: first_event.receipt.server_hash.clone(),
    };
    let first_page = page_for(
        &first_request,
        vec![first_event],
        first_cursor.clone(),
        response.next_cursor.clone(),
        AUTHORITY_A,
        KEY_A,
    );
    let mut state = ReplayState::new(STREAM).unwrap();
    let mut sink = RecordingSink::default();
    let applied = verified(&state, first_request, first_page)
        .apply(&mut state, &mut sink)
        .unwrap();
    assert!(!applied.complete());
    assert_eq!(applied.snapshot_cursor(), &response.next_cursor);

    let second_request = request(
        first_cursor,
        SnapshotSelection::Exact(response.next_cursor.clone()),
    );
    let second_page = page_for(
        &second_request,
        vec![AcceptedEvent {
            envelope: batch.events[1].clone(),
            receipt: response.receipts[1].clone(),
        }],
        response.next_cursor.clone(),
        response.next_cursor.clone(),
        AUTHORITY_A,
        KEY_A_ROTATED,
    );
    let applied = verified(&state, second_request, second_page)
        .apply(&mut state, &mut sink)
        .unwrap();
    assert!(applied.complete());
    assert_eq!(state.cursor().position, 2);

    let bad_request = request(
        state.cursor().clone(),
        SnapshotSelection::Exact(StreamCursor {
            position: 3,
            head_hash: "f".repeat(64),
        }),
    );
    let bad_page = page_for(
        &bad_request,
        vec![AcceptedEvent {
            envelope: batch.events[1].clone(),
            receipt: response.receipts[1].clone(),
        }],
        StreamCursor {
            position: 3,
            head_hash: response.receipts[1].server_hash.clone(),
        },
        StreamCursor {
            position: 3,
            head_hash: "f".repeat(64),
        },
        AUTHORITY_A,
        KEY_A,
    );
    assert!(
        UntrustedPullPage::new(bad_request, bad_page)
            .validate(&state)
            .is_err()
    );
}

#[test]
fn receipt_rejects_zero_hash_self_loop_and_impossible_journal_head() {
    let batch = batch();
    let mut receipt = response_for(&batch).receipts.remove(0);
    receipt.server_hash = EMPTY_STREAM_HASH.to_owned();
    assert_eq!(receipt.validate_shape().unwrap_err().field(), "server_hash");

    let mut receipt = response_for(&batch).receipts.remove(0);
    receipt.server_hash.clone_from(&receipt.server_prev_hash);
    assert_eq!(receipt.validate_shape().unwrap_err().field(), "server_hash");

    let mut receipt = response_for(&batch).receipts.remove(0);
    receipt.journal_head = EMPTY_STREAM_HASH.to_owned();
    assert_eq!(
        receipt.validate_shape().unwrap_err().field(),
        "journal_head"
    );

    let mut receipt = response_for(&batch).receipts.remove(0);
    receipt.journal_head.clone_from(&receipt.server_prev_hash);
    assert_eq!(
        receipt.validate_shape().unwrap_err().field(),
        "journal_head"
    );
}

#[test]
fn signature_shape_requires_canonical_base64url_padding() {
    for valid in [
        "a".repeat(16),
        format!("{}==", "a".repeat(18)),
        format!("{}=", "a".repeat(19)),
    ] {
        assert!(validate_signature(&valid).is_ok(), "rejected {valid}");
    }

    for invalid in [
        format!("{}=", "a".repeat(16)),
        format!("{}==", "a".repeat(16)),
        "a".repeat(17),
        "aaaaaaaa=aaaaaaaa".to_owned(),
        format!("{}===", "a".repeat(17)),
    ] {
        assert!(validate_signature(&invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn wire_limits_accept_max_and_reject_max_plus_one() {
    let mut event = envelopes().remove(0);
    event.payload = format!("\"{}\"", "a".repeat(MAX_EVENT_PAYLOAD_BYTES - 2));
    assert_eq!(event.payload.len(), MAX_EVENT_PAYLOAD_BYTES);
    assert!(event.validate().is_ok());
    event.payload.insert(event.payload.len() - 1, 'a');
    assert_eq!(event.payload.len(), MAX_EVENT_PAYLOAD_BYTES + 1);
    assert_eq!(event.validate().unwrap_err().field(), "payload");

    let mut event = envelopes().remove(0);
    event.causal_parents = (0..MAX_CAUSAL_PARENTS)
        .map(|index| format!("01890f3e-9b4a-7cc0-8a1a-{index:012x}"))
        .collect();
    assert!(event.validate().is_ok());
    event
        .causal_parents
        .push("01890f3e-9b4a-7cc0-8a1a-000000000010".to_owned());
    assert_eq!(event.validate().unwrap_err().field(), "causal_parents");

    let make_events = |count: usize, payload: &str| {
        (0..count)
            .map(|index| {
                let mut event = envelopes().remove(0);
                event.client_event_id = format!("01890f3e-9b4a-7cc0-8a1a-{:012x}", index + 0x100);
                event.payload = payload.to_owned();
                event.local_seq = index as u64;
                event.local_prev_hash = if index == 0 {
                    EMPTY_STREAM_HASH.to_owned()
                } else {
                    format!("{:064x}", index)
                };
                event.local_hash = format!("{:064x}", index + 1);
                event
            })
            .collect::<Vec<_>>()
    };

    let mut bounded_batch = batch();
    bounded_batch.events = make_events(MAX_BATCH_EVENTS, "null");
    assert!(bounded_batch.validate().is_ok());
    bounded_batch.events.push({
        let mut event = bounded_batch.events.last().unwrap().clone();
        event.client_event_id = "01890f3e-9b4a-7cc0-8a1a-000000000999".to_owned();
        event.local_seq += 1;
        event.local_prev_hash.clone_from(&event.local_hash);
        event.local_hash = "f".repeat(64);
        event
    });
    assert_eq!(bounded_batch.validate().unwrap_err().field(), "events");

    let max_payload = format!("\"{}\"", "a".repeat(MAX_EVENT_PAYLOAD_BYTES - 2));
    let mut aggregate_batch = batch();
    aggregate_batch.events = make_events(
        MAX_BATCH_PAYLOAD_BYTES / MAX_EVENT_PAYLOAD_BYTES,
        &max_payload,
    );
    assert_eq!(
        aggregate_batch
            .events
            .iter()
            .map(|event| event.payload.len())
            .sum::<usize>(),
        MAX_BATCH_PAYLOAD_BYTES
    );
    assert!(aggregate_batch.validate().is_ok());
    aggregate_batch.events.push({
        let mut event = aggregate_batch.events.last().unwrap().clone();
        event.client_event_id = "01890f3e-9b4a-7cc0-8a1a-000000000998".to_owned();
        event.local_seq += 1;
        event.local_prev_hash.clone_from(&event.local_hash);
        event.local_hash = "e".repeat(64);
        event.payload = "null".to_owned();
        event
    });
    assert_eq!(aggregate_batch.validate().unwrap_err().field(), "payload");

    let mut pull = request(StreamCursor::genesis(), SnapshotSelection::Latest);
    pull.limit = MAX_PULL_EVENTS;
    assert!(pull.validate().is_ok());
    pull.limit = MAX_PULL_EVENTS + 1;
    assert_eq!(pull.validate().unwrap_err().field(), "limit");
}

#[test]
fn replay_anchor_pruning_stays_at_the_exact_bound() {
    let mut state = ReplayState::new(STREAM).unwrap();
    let newest = MAX_REPLAY_ANCHORS as u64 + 8;
    state.cursor = StreamCursor {
        position: newest,
        head_hash: "f".repeat(64),
    };
    for position in 1..=newest {
        state.anchors.insert(
            position,
            ReplayAnchor {
                head_hash: format!("{position:064x}"),
                event_identity: Some((format!("event-{position}"), format!("hash-{position}"))),
            },
        );
    }

    state.prune_anchors();

    assert_eq!(state.anchors.len(), MAX_REPLAY_ANCHORS);
    assert_eq!(
        state.anchors.keys().next().copied(),
        Some(newest + 1 - MAX_REPLAY_ANCHORS as u64)
    );
    assert!(state.anchors.contains_key(&newest));
}

#[test]
fn canonical_signature_payloads_have_distinct_golden_vectors() {
    let batch = batch();
    let response = response_for(&batch);
    let (request, page) = pull_fixture();
    let receipt = response.receipts.first().unwrap();
    let receipt_digest = payload_digest(&receipt.signing_payload().unwrap());
    let append_digest = payload_digest(
        &response
            .attestation
            .signing_payload(&batch, &response)
            .unwrap(),
    );
    let page_digest = payload_digest(&page.attestation.signing_payload(&request, &page).unwrap());
    assert_eq!(
        (
            receipt_digest.as_str(),
            append_digest.as_str(),
            page_digest.as_str(),
        ),
        (
            "5eb2840c73ea66328886b1ddd24f2daba0470934aa988aeb7360de7c266acb2e",
            "fcdbd9db10656fda8ae0fe705a48fff7c0e997d1285ddef656d1460b103a185a",
            "2af67e0403817cb43824250e933fcf8153f643163db8bf03aef1b6a6fd03ccdf",
        )
    );
    assert_eq!(
        BTreeSet::from([receipt_digest, append_digest, page_digest]).len(),
        3
    );
}

#[cfg(feature = "experimental-legacy")]
#[test]
fn legacy_ack_is_explicitly_feature_gated_and_unverified() {
    let ack = Ack {
        seq: 0,
        hash: "a".repeat(64),
    };
    assert!(ack.validate().is_ok());
    let mut state = SyncState::new();
    state.try_confirm(0, ack).unwrap();
    assert_eq!(state.confirmed(), 1);
}
