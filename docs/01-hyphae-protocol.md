# PliegoRS / Hyphae Client Protocol

**Wire version:** `pliego-hyphae/1`
**Client implementation:** `crates/pliego-hyphae`
**Status:** validated client and transport contract; production service boundary
not implemented in this repository
**Last reviewed:** 2026-07-12

## Boundary

PliegoRS is a general-purpose web framework. Hyphae is its privileged data and
provenance platform, not a mandatory dependency for static sites or applications
that choose another backend.

`pliego-hyphae` now implements the stable client-side protocol surface:

- strict Serde wire types with unknown-field rejection;
- version, UUIDv7, stream, kind, hash, timestamp, signature-shape, and limit
  validation;
- immutable idempotent append batches and typed cursor conflicts;
- typed receipts plus a cryptographic verifier boundary;
- bounded retry that resends the identical batch after a lost ACK;
- bounded pull pages, chain continuity checks, overlap deduplication, and replay;
- the old `JournalTransport` / `push_pending` seam for compatibility.

It does **not** implement or imply production credentials, tenant resolution,
authorization, key distribution, durable idempotency storage, rate limiting, or
a Hyphae service. Passing local tests proves the client contract, not that a
remote deployment honors it.

## Production Network Shape

```text
PliegoRS app
  local event log + durable outbox (future)
        |
        | authenticated, versioned sync
        v
Cloudflare gateway (not implemented here)
  identity -> tenant -> scope -> quotas -> exact CORS
        |
        | private service request
        v
Hyphae API (not implemented here)
  idempotency -> expected cursor -> durable append -> signed receipt
        |
        v
Hyphae journal + projections
```

A browser must never receive raw Hyphae credentials or call an unscoped journal
endpoint. The production gateway derives tenant, actor, permissions, and limits
from authenticated server state. Those authority fields are deliberately absent
from every client envelope.

## Event Envelope

`EventEnvelope` is the accepted client shape:

```json
{
  "protocol": "pliego-hyphae/1",
  "client_event_id": "01890f3e-9b4a-7cc0-8a1a-0123456789ab",
  "stream_id": "project:alpha",
  "schema_version": 1,
  "kind": "app_task_added",
  "payload": "{\"title\":\"First task\"}",
  "local_seq": 41,
  "local_prev_hash": "64 lowercase hex characters",
  "local_hash": "64 lowercase hex characters",
  "causal_parents": [],
  "created_at": "2026-07-12T20:00:00Z"
}
```

The payload field contains one serialized JSON value and is opaque to the sync
crate. `EventEnvelope::from_local_event` safely serializes an existing
`pliego_log::Event` payload as a JSON string. A product that uses structured
payloads may construct the envelope directly and must call `validate`.

Validation rules are intentionally narrow:

- protocol must equal `pliego-hyphae/1` exactly;
- event and batch ids are canonical lowercase UUIDv7;
- stream ids contain 1..128 safe ASCII token/path characters, never traversal;
- client kinds must use `app_*` and lowercase ASCII; engine opcodes never pass;
- local and durable hashes are 64-character lowercase SHA-256 hex;
- `EventEnvelope::wire_hash` canonically binds identity, kind, payload, local
  chain, causal metadata, and timestamp for the durable receipt;
- schema versions start at 1;
- event payload is valid JSON and at most 64 KiB;
- at most 16 unique, non-self causal parents are accepted;
- unknown JSON fields are rejected rather than ignored.

## Append Batch And Retry

`AppendBatch` is the unit of idempotency:

```json
{
  "protocol": "pliego-hyphae/1",
  "batch_id": "01890f3e-9b4a-7cc2-8a1a-0123456789ab",
  "stream_id": "project:alpha",
  "expected_cursor": {
    "position": 9182,
    "head_hash": "64 lowercase hex characters"
  },
  "events": []
}
```

The real wire transport should map this value to an authenticated endpoint such
as `POST /v1/sync/append`, using `batch_id` as its idempotency identity. A batch
contains 1..128 events, no more than 512 KiB of aggregate payload, one stream,
unique event ids, contiguous local sequences, and linked local hashes.

`append_with_retry` validates before I/O, retries only
`TransportError::Retryable`, and resends the same borrowed `AppendBatch`. It
never retries cursor conflicts or permanent rejections. The adversarial fake
server test commits a batch, loses the first ACK, receives the exact retry, and
returns the original receipts without a second durable append.

That test defines the required server behavior; it is not a server
implementation. Crash-safe retry also requires the application to persist the
complete in-flight batch before the first request. An in-memory retry cannot
survive reload or process loss.

`expected_cursor` is an optimistic concurrency precondition. A server must
atomically compare it with the authorized stream head, append the whole batch,
and store the idempotency response. Divergence returns
`TransportError::CursorConflict`; it never silently creates or overwrites a
branch.

An absent `expected_cursor` means verifiable genesis, not "accept any current
head": the first durable sequence must be one and the previous hash must be the
zero hash. Clients that do not know a non-empty remote head must pull it before
appending. `next_cursor.position` must equal the final signed `server_seq`, so a
transport cannot substitute cursor position while preserving valid receipts.

## Receipt And Verification

Each event receives one typed `Receipt`:

```json
{
  "client_event_id": "01890f3e-9b4a-7cc0-8a1a-0123456789ab",
  "stream_id": "project:alpha",
  "envelope_hash": "canonical hash of the exact accepted envelope",
  "server_seq": 9182,
  "server_hash": "64 lowercase hex characters",
  "server_prev_hash": "64 lowercase hex characters",
  "journal_head": "64 lowercase hex characters",
  "committed_at": "2026-07-12T20:00:01Z",
  "key_id": "hyphae-receipt-2026-01",
  "signature": "base64url"
}
```

`AppendResponse::validate_against` checks exact batch and stream identity,
receipt cardinality and order, cursor extension, durable hash continuity, and
the final journal head. This is untrusted-input shape validation only.

Cryptographic trust is explicit. `Receipt::signing_payload` produces canonical
bytes in this order:

1. length-prefixed `pliego-hyphae/1`, client event id, stream id, and canonical
   envelope hash;
2. big-endian `server_seq` as `u64`;
3. length-prefixed server hash, previous hash, journal head, committed time, and
   key id.

Each string length is a big-endian `u32` followed by its UTF-8 bytes. The
signature field is excluded. `verify_receipt` delegates those bytes to a
`ReceiptVerifier` implemented by the production key store. This crate does not
select a signature algorithm, ship public keys, or pretend a shape-valid
signature is authentic. Provenance UI may claim durability only after
verification against a pinned or securely rotated trusted key.

## Pull And Replay

`PullRequest` carries protocol, stream, an optional `after` cursor, and a limit
in `1..=256`. `PullPage` returns ordered `AcceptedEvent` pairs, a next cursor,
and an explicit completeness flag.

Before replay, `PullPage::validate_against` enforces:

- exact protocol and stream;
- requested page bound;
- envelope/receipt identity equality;
- no duplicate event ids inside a page;
- increasing server sequence and continuous durable hashes;
- cursor position equal to start plus returned event count;
- an incomplete page must return at least one event and advance.

`apply_pull_page` sends unseen events to `ReplaySink::apply_batch` as one bounded
transaction; returning an error must leave application state unchanged. An
overlapping page with the same event id and durable hash is a no-op. Reusing an
event id with another hash is rejected as equivocation. Replay cursors cannot
move backward or change hash at the same position. The dedupe window retains
only the latest validated page (at most 256 entries); stale replay outside that
window is rejected instead of growing memory without bound.

Receipt signatures must be verified before a product marks replayed state as
trusted. The pure replay helper intentionally does not hide that policy behind
an implicit global key store.

## Limits

| Value | Client limit |
| --- | ---: |
| Events per append batch | 128 |
| Payload per event | 64 KiB |
| Aggregate batch payload | 512 KiB |
| Causal parents per event | 16 |
| Events per pull page | 256 |
| Stream id | 128 bytes |
| Signature encoding | 512 bytes |
| Legacy in-memory acknowledgements | 1,000,000 |

The production gateway may enforce smaller tenant-scoped limits. It must reject,
not truncate, oversized values. Network response-body limits, request timeouts,
backoff, and rate quotas belong to concrete transports and the gateway.

## Legacy Compatibility

`JournalTransport`, `Ack`, `SyncState`, `push_pending`, and WASM
`fetch::append_remote` remain available for the existing M5 spike.
`push_pending` now validates the local hash chain, namespaced kind, payload size,
ordered ack cursor, ack hash, and a defensive allocation ceiling.

The legacy one-event route has no batch id and therefore cannot deduplicate a
lost ACK. It must remain experimental and private. New product code should use
the batch contract. Its WASM compatibility transport still enforces a 64 KiB
ACK ceiling incrementally through `ReadableStream` (and rejects an oversized
`Content-Length`) before accumulating the response body.

## Adversarial Coverage

The native crate tests cover:

- stream traversal, forged authority fields, reserved kinds, malformed hashes,
  invalid protocol versions, invalid UUIDs, and oversized payloads;
- duplicate ids, reordered events, broken local links, response substitution,
  discontinuous receipt chains, and forged cursors;
- bounded retry, lost-ACK idempotency, and non-retryable cursor conflict;
- discontinuous/non-advancing pull pages, replay overlap deduplication, and
  receipt claim tampering;
- legacy retry compatibility, ack gaps, conflicting acks, and allocation guards.

These are pure client/fake-transport tests. Before production, the same contract
suite must run against the authenticated Cloudflare gateway and a real Hyphae
instance, including tenant isolation, quota, revoke, timeout, partial write,
crash recovery, and signing-key rotation cases.

## Remaining Production Work

1. Cloudflare authentication, tenant/actor resolution, exact CORS, quotas, and
   secret isolation.
2. Hyphae batch transaction, durable idempotency records, expected-cursor compare,
   and signed receipts matching the canonical payload.
3. A concrete authenticated async `BatchTransport` with body/time limits,
   cancellation, jittered backoff, and telemetry redaction.
4. Durable browser outbox and replay state (IndexedDB), including schema migration
   and crash-safe in-flight batch retention.
5. Trusted public-key discovery/pinning, rotation, revocation, and receipt
   verification policy.
6. End-to-end adversarial tests against the real gateway and Hyphae deployment.

Until those items pass, documentation and UI must describe this as a stable
client contract with an experimental integration, never as completed production
sync or verified cross-tenant provenance.
