# PliegoRS / Hyphae Client Protocol

**Wire version:** `pliego-hyphae/2`
**Client implementation:** `crates/pliego-hyphae`
**Status:** fail-closed client contract; authenticated production transport,
gateway, key distribution, and Hyphae service are not implemented here
**Last reviewed:** 2026-07-15

## Boundary

PliegoRS is a general-purpose web framework. Hyphae is its privileged durable
data and provenance platform when an application needs synchronization; static
projects and applications using another backend do not depend on it.

Protocol v2 closes the client-side trust path:

```text
untrusted wire values
  -> request- and state-bound structural validation
  -> page/append attestation and receipt verification
  -> application event-version policy
  -> single-use replay application
```

The Rust type system prevents a raw or merely validated pull page from entering
`ReplaySink`. This proves a property of the client library. It does not prove
that a deployed gateway authenticates tenants correctly, that signing keys are
stored securely, or that a remote Hyphae implementation is durable.

## Production Network Shape

```text
PliegoRS application
  durable outbox + stream-bound replay state (production persistence pending)
        |
        | authenticated protocol v2 transport (pending)
        v
Cloudflare gateway or equivalent trusted boundary (pending)
  identity -> tenant -> scope -> quotas -> exact CORS
        |
        | private authenticated service request
        v
Hyphae service (pending)
  idempotency -> cursor compare -> append/pull -> signed attestations
```

A browser must never receive raw Hyphae credentials or call an unscoped journal
endpoint. Tenant, actor, permission, quota, and key-distribution policy belong
to the trusted gateway and are deliberately absent from client-authored event
envelopes.

## Version 2 And Downgrade Policy

Modern values carry `protocol: "pliego-hyphae/2"`. Unknown versions and v1
values fail validation; there is no silent downgrade. Version 2 is an
intentional pre-release wire and Rust API break because page completeness,
snapshot identity, and append transaction identity could not be added safely to
the v1 signature boundary.

The historical single-event M5 transport is isolated behind the non-default
`experimental-legacy` Cargo feature. It does not implement protocol v2 and its
ACK values can never become `VerifiedPullPage` or `VerifiedAppendResponse`.

## Event Envelope

`EventEnvelope` is the client-authored event shape:

```json
{
  "protocol": "pliego-hyphae/2",
  "client_event_id": "01890f3e-9b4a-7cc0-8a1a-0123456789ab",
  "stream_id": "project:alpha",
  "schema_version": 1,
  "kind": "app_task_added",
  "payload": "{\"title\":\"First task\"}",
  "local_seq": 41,
  "local_prev_hash": "0000000000000000000000000000000000000000000000000000000000000000",
  "local_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "causal_parents": [],
  "created_at": "2026-07-15T20:00:00Z"
}
```

Validation requires the exact protocol, canonical lowercase UUIDv7 event ID, a
bounded traversal-free stream ID, an `app_*` kind, a non-zero schema version,
valid JSON payload, RFC3339 timestamp, canonical hashes, and at most 16 unique
non-self causal parents. One payload is capped at 64 KiB. Unknown JSON fields
are rejected.

`EventEnvelope::wire_hash` is SHA-256 over a length-delimited canonical encoding
of every envelope field, including schema version, local chain, causal parents,
and timestamp. A receipt must bind this exact hash.

## Append Contract

`AppendBatch` is the immutable unit of idempotency:

```json
{
  "protocol": "pliego-hyphae/2",
  "batch_id": "01890f3e-9b4a-7cc2-8a1a-0123456789ab",
  "stream_id": "project:alpha",
  "expected_cursor": {
    "position": 9182,
    "head_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  },
  "events": [
    {
      "protocol": "pliego-hyphae/2",
      "client_event_id": "01890f3e-9b4a-7cc0-8a1a-0123456789ab",
      "stream_id": "project:alpha",
      "schema_version": 1,
      "kind": "app_task_added",
      "payload": "{\"title\":\"First task\"}",
      "local_seq": 41,
      "local_prev_hash": "0000000000000000000000000000000000000000000000000000000000000000",
      "local_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "causal_parents": [],
      "created_at": "2026-07-15T20:00:00Z"
    }
  ]
}
```

The expected cursor is always explicit. Genesis is position zero with the
all-zero hash; there is no blind append. A batch contains 1..128 events, no more
than 512 KiB of aggregate payload, one stream, unique event IDs, and a contiguous
local chain.

`append_with_retry` validates before I/O, retries only
`TransportError::Retryable`, and resends the same borrowed batch. Cursor
conflicts and permanent rejections are never retried. A successful call returns
`ValidatedAppendResponse`, not an authenticated result:

```rust
let validated = append_with_retry(&mut transport, &batch, 3)?;
let verified: VerifiedAppendResponse = validated.verify(&authority_verifier)?;
let durable_cursor = verified.next_cursor();
```

The raw `AppendResponse` contains one receipt per event, an exact next cursor,
and a mandatory `AppendAttestation`. Structural validation requires absolute
one-based `server_seq` values, contiguous durable hash links, exact envelope
identity, and a final cursor matching the signed journal head.

The append-attestation signing domain is `pliego-hyphae/2/append`. Its canonical
payload binds:

- protocol, logical authority, batch ID, and stream;
- expected and resulting cursors;
- event count and ordered digest of envelopes plus complete receipts;
- authoritative commit time and attestation key ID.

Every per-event receipt is verified separately. All receipt and attestation
keys must resolve to the same logical `AuthorityId`.

## Receipts And Authority

A `Receipt` carries client event ID, stream, envelope hash, absolute server
sequence, server hash and previous hash, journal head, authoritative commit
time, key ID, and detached base64url signature.

Its signing domain is `pliego-hyphae/2/receipt`. The canonical payload uses
big-endian numeric fields and length-prefixed UTF-8 fields in the order defined
by `Receipt::signing_payload`. The signature itself is excluded from its own
payload and is included in the enclosing ordered event digest.

`ReceiptVerifier` is the trust-injection boundary. It receives canonical bytes
plus `VerificationContext` containing `SignaturePurpose`, claimed authority,
stream, signed time, and key ID. It must return a stable `AuthorityId` or one of
the fail-closed `VerificationError` variants:

- `UnknownKey`;
- `RevokedKey`;
- `UnauthorizedStream`;
- `InvalidSignature`;
- `Unavailable`;
- `AuthorityMismatch`.

The crate deliberately does not select a signature algorithm or ship a public
key store. A production verifier must pin or securely discover keys, enforce
stream scope, apply revocation at the signed timestamp, and map rotated keys to
one stable logical authority.

## Snapshot-Consistent Pull

`PullRequest` binds every response to a UUIDv7 request ID, stream, exact current
cursor, snapshot policy, selection, and bounded page size:

```json
{
  "protocol": "pliego-hyphae/2",
  "request_id": "01890f3e-9b4a-7cc3-8a1a-0123456789ab",
  "stream_id": "project:alpha",
  "after": {
    "position": 9182,
    "head_hash": "64 lowercase hex characters"
  },
  "snapshot": { "mode": "latest" },
  "selection": "whole_stream",
  "limit": 256
}
```

The first request uses `SnapshotSelection::Latest`. Its signed response fixes a
`snapshot_cursor`. Continuations use
`SnapshotSelection::Exact(snapshot_cursor)` and a new request ID. Protocol v2
supports only `PullSelection::WholeStream`; selective sync is not inferred from
unknown fields or client-side filtering.

`PullPage` contains ordered `AcceptedEvent` values, a `next_cursor`, the fixed
`snapshot_cursor`, derived `complete`, and a mandatory `PageAttestation`:

```json
{
  "protocol": "pliego-hyphae/2",
  "stream_id": "project:alpha",
  "events": [],
  "next_cursor": {
    "position": 9182,
    "head_hash": "64 lowercase hex characters"
  },
  "snapshot_cursor": {
    "position": 9182,
    "head_hash": "64 lowercase hex characters"
  },
  "complete": true,
  "attestation": {
    "authority_id": "hyphae:production",
    "issued_at": "2026-07-15T20:00:01Z",
    "key_id": "hyphae-page-2026-07",
    "signature": "base64url"
  }
}
```

The page-attestation domain is `pliego-hyphae/2/page`. Its canonical payload
binds the authority, request ID, stream, request cursor, requested snapshot,
whole-stream selection, limit, next cursor, fixed snapshot cursor, completion,
event count, ordered event digest, issued time, and key ID.

The ordered event digest uses the domain
`pliego-hyphae/2/accepted-events`, the event count, each envelope wire hash,
each complete canonical receipt payload, and each receipt signature.

These invariants hold before signature verification:

- every receipt sequence equals `request.after.position + index + 1`;
- every receipt extends the preceding signed durable hash;
- `next_cursor` equals the final returned receipt, or the request cursor for an
  empty page;
- `next_cursor` never passes `snapshot_cursor`;
- `complete` is true exactly when both cursors are equal;
- an incomplete page is non-empty and advances;
- an exact continuation cannot change the fixed snapshot.

An empty complete page still requires a valid page attestation. It proves that
the requested cursor equaled the authority's fixed checkpoint when issued; it
does not prove that the stream can never receive another event.

## Pull Typestate

The public application path is linear and consuming:

```rust
let validated: ValidatedPullPage =
    UntrustedPullPage::new(request, raw_page).validate(&replay_state)?;
let verified: VerifiedPullPage =
    validated.verify(&authority_verifier, &event_versions)?;
let applied: AppliedPullPage = verified.apply(&mut replay_state, &mut reducer)?;
```

`ValidatedPullPage`, `VerifiedPullPage`, and their authenticated event values
have private fields and cannot be deserialized or constructed by consumers.
Only `VerifiedPullPage::apply` accepts a `ReplaySink`, and the sink receives
`&[VerifiedAcceptedEvent]`, never raw `AcceptedEvent` values. Applying consumes
the verified page, so it cannot be applied twice.

## Replay State, Gaps, Forks, And Overlap

`ReplayState::new(stream_id)` starts at explicit genesis and is permanently
bound to that stream. Its first applied page binds one logical authority. It
stores the authenticated cursor, active snapshot, and a bounded window of
absolute-position anchors containing durable head and event identity.

Before a reducer runs, replay rejects:

- another stream or logical authority;
- a request cursor ahead of local state;
- an unknown overlap position outside the bounded anchor window;
- a different hash at a known position;
- a snapshot rollback or change during an active pull cycle;
- a fresh position gap, moved event identity, or divergent overlap.

An exact repeated overlap is a no-op. Fresh events are collected first, the
candidate framework state is computed without mutation, and only then is
`ReplaySink::apply_batch` called. A reducer error does not advance
`ReplayState`. The sink contract itself requires application state to remain
unchanged on error; arbitrary user code cannot be rolled back by this crate.

## Event-Version Policy

Structural validation accepts positive schema versions because wire validity is
not application compatibility. `EventVersionPolicy` must explicitly admit each
`(kind, schema_version)` before a page becomes `VerifiedPullPage`. An unknown
pair returns `EventVersionError` and never reaches the reducer.

R2 implements rejection, not migration. The local upcaster, reducer identity,
schema-set, and transactional projection boundary is specified by the
[R3 event and snapshot contract](30-event-schema-and-snapshot-contract.md).
R2 still owns stream identity, signer authority, and verified replay admission.

## Limits

| Value | Client limit |
| --- | ---: |
| Events per append batch | 128 |
| Payload per event | 64 KiB |
| Aggregate append payload | 512 KiB |
| Causal parents per event | 16 |
| Events per pull page | 256 |
| Stream ID | 128 bytes |
| Signature encoding | 512 bytes |
| Retained replay anchors | 1,025 |

Concrete transports must additionally bound response bodies, timeouts,
cancellation, retry backoff, and tenant quotas before allocating full wire
values.

## Experimental Legacy Feature

The `experimental-legacy` feature exposes the historical `Ack`, `SyncState`,
`JournalTransport`, `push_pending`, and WASM `fetch::append_remote` API. It is
disabled by default and exists only for the maintained M5 spike.

Legacy ACKs validate shape and ordering but have no page/append attestation,
logical authority, snapshot, idempotent batch identity, or verified-replay
typestate. They must not be shown as provenance, imported into `ReplayState`, or
described as verified durability. There is no automatic v1-to-v2 trust upgrade.

See the [verified sync guide](29-hyphae-verified-sync-guide.md) for migration.

## What R2 Does Not Claim

The crate now supplies a fail-closed protocol v2 client boundary. The following
remain production work:

1. Cloudflare authentication, tenant/actor resolution, exact CORS, quotas, and
   secret isolation.
2. A real Hyphae v2 batch/pull implementation with durable idempotency and
   canonical receipt, append, and page signatures.
3. A bounded authenticated async transport with cancellation, jittered backoff,
   and telemetry redaction.
4. Durable browser outbox and `ReplayState` persistence with crash recovery and
   migration.
5. Production key discovery, pinning, rotation, revocation, and operational
   recovery.
6. End-to-end adversarial tests against the deployed gateway and service.

Until those exist, PliegoRS may claim a verified client contract, not completed
production synchronization or cross-tenant provenance.

## Decisions

- [ADR-002: Hyphae active data plane](adr/ADR-002-hyphae-active-plane.md)
- [ADR-004: Hyphae verified sync protocol v2](adr/ADR-004-hyphae-verified-sync-v2.md)
