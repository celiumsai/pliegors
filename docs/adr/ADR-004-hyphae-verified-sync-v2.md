# ADR-004: Hyphae verified sync protocol v2

**Status:** Accepted
**Decision date:** 2026-07-15
**Scope:** `pliego-hyphae` append, pull, authority, and replay contracts

## Context

The version 1 client types validated envelope and receipt shape, but shape was
not authority. A public helper could send a raw `PullPage` to a reducer without
verifying any signature. Sequence numbers only had to increase, replay state
was not bound to a stream, and cursor forks could be detected after the reducer
had already run.

Per-event receipts also cannot prove absence. An empty page has no receipt to
verify, and an intermediary can truncate a page of authentic receipts and flip
an unsigned `complete` flag. Verifying every event receipt is therefore
necessary but insufficient for verified replay.

## Decision

The modern sync contract uses the `pliego-hyphae/2` wire domain and makes trust
state explicit:

```text
untrusted wire value
    -> structurally validated and request-bound page
    -> authority- and version-verified page
    -> applied page
```

Only a verified page can enter the framework replay boundary. Validated and
verified wrappers have private fields, are not deserializable, and consume
themselves during each transition. A reducer receives read-only verified event
values rather than raw `AcceptedEvent` values.

### Signed page checkpoint

Every pull page, including an empty page, carries a page attestation. Its
domain-separated canonical payload binds at least:

- protocol, logical authority, request identity, stream, and selection;
- request cursor, requested snapshot policy, and page limit;
- next cursor, fixed snapshot cursor, and completeness;
- event count and the ordered digest of envelopes and complete receipts;
- issuance time and signing key identity.

The event digest includes each envelope hash, canonical receipt payload, and
receipt signature. `complete` is true exactly when the next cursor equals the
signed snapshot cursor. A valid empty page therefore proves that the requested
cursor is already at the signed checkpoint; it does not prove that no event can
be appended later.

The first page may select the latest signed checkpoint. Every continuation in
that pull cycle is bound to the same snapshot. A new cycle may advance to a
newer checkpoint, but cannot roll back or replace the hash at an equal
position.

### Signed append transaction

An append response carries a separate append attestation. Its canonical payload
uses a different signing domain and binds the logical authority, batch identity,
stream, expected and resulting cursors, ordered event and receipt digest,
transaction cardinality, and signing time. Per-event receipts remain mandatory.
This proves the exact accepted set without treating an unsigned batch echo as
authority.

### Authority and versions

Signature verification returns a validated logical authority, not a Boolean.
Errors distinguish unknown, revoked, stream-unauthorized, invalid-signature,
and unavailable-verifier cases. The verifier receives typed purpose, stream,
and signed-time context so a key store can enforce scope and rotation policy
without parsing canonical bytes. Key rotation is allowed only when every claim
resolves to the same logical authority.

An explicit event-version policy admits each `(kind, schema_version)` pair
before replay. Unknown versions fail closed. Upcasting remains an R3 concern;
R2 never guesses how to transform an event.

### Cursor and replay state

Durable sequence numbers are absolute and contiguous: the event at cursor
position `n` has `server_seq == n`. Genesis is position zero with the all-zero
hash, so the first accepted event has sequence one.

Replay state is constructed for one canonical stream and later binds to one
logical authority. It retains a bounded position-indexed overlap window.
Continuation, fork, gap, authority, snapshot, duplicate, and version checks all
finish before a reducer is called. A reducer failure cannot advance framework
cursor state.

Protocol v2 exposes `whole_stream` as its only selection. Unknown or partial
selection modes are rejected rather than interpreted locally.

### Legacy seam

The M5 single-event ACK transport is available only behind the non-default
`experimental-legacy` feature. Its values are explicitly unverified and cannot
enter the v2 typestate. It exists solely for the historical spike and cannot be
used as provenance or verified durability evidence.

## Consequences

- Version 2 is a deliberate pre-release wire and Rust API break.
- Hyphae or a compatible gateway must produce page and append attestations in
  addition to per-event receipts.
- A caller must supply an authority verifier and an event-version policy.
- An authenticated transport, durable outbox, replay-state persistence, key
  distribution, and a production Hyphae implementation remain separate work.
- The attestation proves what an authorized signer asserted at one checkpoint;
  it does not prove that the signer is honest or that the stream will never
  advance.
- Application-state atomicity remains a reducer contract until R3 supplies the
  transactional projection boundary. R2 guarantees that all framework trust
  checks happen before that contract is invoked.

## Rejected alternatives

- **Verify receipts as an optional helper:** rejected because the verified fact
  can be discarded while the raw page remains applicable.
- **Treat every shape-valid key ID as authority:** rejected because key lookup,
  scope, revocation, and signature validity are distinct decisions.
- **Infer completeness from a short or empty page:** rejected because transport
  metadata does not prove absence.
- **Let each page choose a moving head:** rejected because a pull cycle could
  chase a stream indefinitely or accept rollback and truncation ambiguity.
- **Keep v1 and add fields without a version change:** rejected because strict
  peers need a deterministic downgrade boundary.

## References

- [PliegoRS / Hyphae protocol](../01-hyphae-protocol.md)
- [Hyphae active-plane decision](ADR-002-hyphae-active-plane.md)
- [Execution backlog](../19-product-execution-backlog.md)
- [Hardening roadmap](../28-hardening-roadmap.md)
