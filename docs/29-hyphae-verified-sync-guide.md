# Hyphae Verified Sync Guide

This guide shows the client-side protocol v2 workflow exposed by
`pliego-hyphae`. It does not configure a production gateway, persist an outbox,
or provide signing keys.

## Before You Start

A synchronized application owns four integrations:

1. a `BatchTransport` and/or `PullTransport` that performs bounded authenticated
   I/O;
2. a `ReceiptVerifier` backed by trusted key and authority policy;
3. an `EventVersionPolicy` for the reducer's accepted event schemas;
4. a transactional `ReplaySink` for verified events.

Do not use a verifier that returns success for unknown keys, and do not use a
version policy that accepts every positive integer. Both are explicit trust
boundaries, not convenience callbacks.

## Implement Authority Verification

`ReceiptVerifier` receives typed context for receipts, append attestations, and
page attestations. The implementation must verify the detached signature over
the supplied canonical bytes and return the stable logical authority that owns
the key.

```rust
use pliego_hyphae::{
    AuthorityId, ReceiptVerifier, VerificationContext, VerificationError,
};

struct ProjectKeys;

impl ReceiptVerifier for ProjectKeys {
    fn verify(
        &self,
        context: VerificationContext<'_>,
        signing_payload: &[u8],
        signature: &str,
    ) -> Result<AuthorityId, VerificationError> {
        // Resolve context.key_id(), enforce context.stream_id() scope and
        // revocation at context.signed_at(), then verify signing_payload.
        let _ = (signing_payload, signature);
        Err(VerificationError::Unavailable(
            "connect a production key store".to_owned(),
        ))
    }
}
```

`context.purpose()` distinguishes `Receipt`, `AppendAttestation`, and
`PageAttestation`. `context.claimed_authority()` is a claim to verify, never a
trusted lookup result. Rotated keys may differ, but every claim in one operation
must resolve to the same `AuthorityId`.

## Admit Event Versions

The application policy is keyed by both kind and schema version:

```rust
use pliego_hyphae::{EventVersionError, EventVersionPolicy};

struct TaskVersions;

impl EventVersionPolicy for TaskVersions {
    fn validate(&self, kind: &str, version: u32) -> Result<(), EventVersionError> {
        match (kind, version) {
            ("app_task_added" | "app_task_completed", 1) => Ok(()),
            _ => Err(EventVersionError::new(kind, version, "not supported")),
        }
    }
}
```

Protocol v2 rejects unknown versions. It does not upcast them. Introduce
upcasters only through the R3 snapshot and schema contract.

## Implement The Replay Sink

The reducer receives only authenticated values:

```rust
use pliego_hyphae::{ReplaySink, VerifiedAcceptedEvent};

#[derive(Default)]
struct TaskProjection {
    applied: Vec<String>,
}

impl ReplaySink for TaskProjection {
    fn apply_batch(&mut self, events: &[VerifiedAcceptedEvent]) -> Result<(), String> {
        let mut candidate = self.applied.clone();
        for event in events {
            candidate.push(event.envelope().client_event_id.clone());
        }
        self.applied = candidate;
        Ok(())
    }
}
```

Build application changes in a candidate value and commit only after the whole
batch succeeds. PliegoRS keeps `ReplayState` unchanged when the sink returns an
error, but it cannot roll back mutations performed by arbitrary sink code.

## Pull, Verify, And Apply

Create replay state for exactly one stream:

```rust
use pliego_hyphae::ReplayState;

let mut replay = ReplayState::new("project:alpha")?;
```

The first request asks the authority to fix the latest snapshot:

```rust
use pliego_hyphae::{
    PROTOCOL_V2, PullRequest, PullSelection, PullTransport, SnapshotSelection,
};

let request = PullRequest {
    protocol: PROTOCOL_V2.to_owned(),
    request_id: next_uuid_v7(),
    stream_id: replay.stream_id().to_owned(),
    after: replay.cursor().clone(),
    snapshot: SnapshotSelection::Latest,
    selection: PullSelection::WholeStream,
    limit: 256,
};
```

Treat the transport response as raw input and cross every state explicitly:

```rust
use pliego_hyphae::{AppliedPullPage, SyncError, UntrustedPullPage};

let raw_page = transport
    .pull_page(&request)
    .map_err(SyncError::Transport)?;
let validated = UntrustedPullPage::new(request, raw_page).validate(&replay)?;
let verified = validated.verify(&ProjectKeys, &TaskVersions)?;
let applied: AppliedPullPage = verified.apply(&mut replay, &mut projection)?;
```

`validate` binds shape, stream, sequence, request cursor, fixed snapshot, and
existing replay anchors. `verify` authenticates the page attestation and every
receipt under one authority, then applies event-version policy. `apply` consumes
the verified page and sends only previously unseen events to the sink.

Inspect the result through accessors:

```rust
println!("fresh events: {}", applied.applied_count());
println!("cursor: {}", applied.cursor().position);
println!("snapshot: {}", applied.snapshot_cursor().position);
println!("complete: {}", applied.complete());
println!("authority: {}", applied.authority().as_str());
```

For an incomplete result, continue against the exact signed snapshot:

```rust
let continuation = PullRequest {
    protocol: PROTOCOL_V2.to_owned(),
    request_id: next_uuid_v7(),
    stream_id: replay.stream_id().to_owned(),
    after: applied.cursor().clone(),
    snapshot: SnapshotSelection::Exact(applied.snapshot_cursor().clone()),
    selection: PullSelection::WholeStream,
    limit: 256,
};
```

Use a new request ID for every request. Do not switch back to `Latest` while the
current snapshot remains incomplete. After reaching it, a new pull cycle may
request `Latest` to discover a newer signed head.

## Append And Verify

Append retry and cryptographic verification are separate transitions:

```rust
use pliego_hyphae::{VerifiedAppendResponse, append_with_retry};

let validated = append_with_retry(&mut transport, &batch, 3)?;
let verified: VerifiedAppendResponse = validated.verify(&ProjectKeys)?;
persist_verified_cursor(verified.next_cursor())?;
```

Never mark an outbox item durable from `ValidatedAppendResponse::response()`.
That accessor exposes structurally valid but still untrusted wire data for
diagnostics. Only `VerifiedAppendResponse` establishes authority.

Persist the complete in-flight batch before its first send. In-memory retry can
handle a lost acknowledgement during one process lifetime, but cannot survive a
reload or crash by itself.

## Handle Failures By Class

`SyncError` keeps trust and operational failures separate:

- `Validation`: malformed or inconsistent wire/state input;
- `Verification`: unknown/revoked key, unauthorized stream, invalid signature,
  unavailable verifier, or authority mismatch;
- `EventVersion`: reducer does not support the event contract;
- `Transport`: non-retryable transport conflict or rejection;
- `AttemptsExhausted`: all allowed transient attempts failed;
- `Reducer`: verified events were rejected by application state.

Never catch one of these errors and continue by applying the raw page. Keep the
last authenticated cursor, surface a bounded diagnostic, and retry or recover
according to the specific class.

## Empty Pages And Completeness

An empty page is not self-authenticating. It is accepted only when:

- `next_cursor` equals the request cursor;
- `next_cursor` equals the signed `snapshot_cursor`;
- `complete` is true; and
- the mandatory page attestation verifies.

This proves completion at one signed checkpoint. It does not promise that the
stream will remain unchanged after `issued_at`.

## Migrate From The Legacy Seam

The historical API is disabled by default. A project that still compiles the M5
spike must opt in explicitly:

```toml
[dependencies.pliego-hyphae]
version = "=0.2.0-beta.1"
features = ["experimental-legacy"]
```

That feature exposes `Ack`, `SyncState`, `JournalTransport`, `push_pending`, and
the WASM `fetch` module. These values are unverified compatibility data. They
cannot be converted into v2 receipts, attestations, verified pages, or trusted
provenance.

Migration requires a real v2 endpoint and proceeds as a new trust establishment:

1. retain legacy state only for audit/debug display and label it unverified;
2. configure a v2 transport, trusted authority verifier, and explicit event
   version policy;
3. create stream-bound `ReplayState` at genesis;
4. pull and verify a complete v2 snapshot;
5. compare the resulting application projection before cutting reads over;
6. persist future v2 outbox batches and authenticated replay state;
7. remove `experimental-legacy` after the historical spike is no longer built.

There is no downgrade fallback and no way to manufacture v2 authority from an
old sequence/hash ACK.

## Production Checklist

Before calling synchronization production-ready, separately verify:

- gateway authentication, tenant and stream authorization, exact CORS, and
  quotas;
- durable idempotency and atomic append on the service;
- canonical v2 receipt, append-attestation, and page-attestation signatures;
- bounded request/response bodies, timeout, cancellation, and retry policy;
- key discovery, pinning, rotation, revocation, and incident recovery;
- durable outbox and replay-state persistence across crash and migration;
- adversarial end-to-end tests against the deployed gateway and Hyphae service.

The library's green client tests do not substitute for this production gate.

## References

- [Protocol v2 reference](01-hyphae-protocol.md)
- [ADR-002: Hyphae active plane](adr/ADR-002-hyphae-active-plane.md)
- [ADR-004: verified sync protocol v2](adr/ADR-004-hyphae-verified-sync-v2.md)
