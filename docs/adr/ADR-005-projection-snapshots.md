# ADR-005: Projection snapshots bind history and executable contracts

**Status:** Accepted and verified
**Decision date:** 2026-07-15
**Scope:** event schema evolution, projection transactions, and snapshot restore

## Context

The earlier fold implementation can resume from an accumulator and numeric
cursor. That is enough to demonstrate tail-only reduction, but it cannot prove
that the accumulator belongs to the current durable history hash, schema
catalog, reducer implementation, reducer configuration, or state codec.
Two histories may share a position and still have different heads. Two reducer
packages may decode the same state type and still implement different folds.

R2 makes accepted history fail closed at the client replay boundary. It admits
only verified event envelopes and explicit event versions, but deliberately
does not migrate event payloads or define a persistent projection snapshot.
R3 must preserve the R2 authority and ordering guarantees while making derived
state reproducible and safely reusable.

## Decision

### Typed and versioned application events

Application event identity is `(app_* kind, positive schema version)`. A sealed
catalog admits exact versions and defines the reducer-facing target version.
The catalog is immutable after validation and has a deterministic digest that
does not depend on registration order or function addresses.

Payload JSON is parsed and canonically encoded before it enters catalog
validation, upcasting, configuration hashing, or a JSON state codec.
Typed append additionally decodes the canonical bytes through the declared
schema type and requires exact value equality before retaining an event. This
closes observed asymmetric Serde cases without claiming custom serializer,
deserializer, or equality code is trustworthy by construction.

### Explicit adjacent upcasters

Every version transition is declared as one adjacent, same-kind edge. Catalog
construction and sealing reject cross-kind or non-adjacent edges, duplicate
schemas or edges, gaps, schemas past the current version, and missing current
schemas or transitions. Monotonic adjacent edges make cycles and downgrades
unrepresentable. Each edge output is canonicalized and decoded against its
destination schema.
There is no implicit latest-version conversion and no pass-through of an
unknown version.

Upcasters transform application payload semantics only. They cannot alter
verified event identity, authority, receipt, durable order, or history hashes.

### Transactional projection reducer

A reducer applies an ordered admitted batch to candidate state. Canonical state
bytes are encoded, decoded, compared, and re-encoded before state, bytes,
history cursor, and counters publish together. Any failure leaves the stable
checkpoint unchanged; snapshot creation reuses the committed bytes.

Reducers are deterministic and side-effect free. External work is represented
through explicit effect requests, not performed while folding history.

The projection contract carries a stable reducer identifier and semantic
revision. Configuration capable of changing output is canonicalized and bound
by digest. The application must include its initial state in that contract when
changing the initial value can change replay. Function addresses and host-
language type names are not stable identities.

### Contract-bound snapshots

Every projection snapshot binds:

- snapshot format and version;
- exact durable history position and head hash;
- complete sealed schema-catalog digest;
- reducer identifier and version;
- canonical reducer-configuration digest;
- state-codec identifier, semantic revision, and configuration digest;
- digest of exact encoded state bytes; and
- a snapshot digest over the complete binding metadata, exact state bytes, and
  state digest.

Restore requires an exact match for every binding and recomputes both digests
before publishing decoded state. It is atomic and fail closed. Mismatch does
not trigger hidden conversion or partial state installation. A caller may
explicitly discard the snapshot and perform full replay.

State snapshots are not automatically upcast. Event upcasters operate before
reduction; changing the projection contract invalidates the old snapshot.

### Parity is executable evidence

Deterministic generated-case tests exercise admitted version mixtures, batch
partitions, and snapshot split points. They compare live reduction, full replay,
and snapshot-plus-tail replay for equal canonical state and cursor. Mutation
tests show that changing any bound field or state byte causes restore failure
without changing active projection state.

Golden vectors pin canonical JSON, catalog descriptors, and snapshot digest
preimages. Cross-target agreement remains an acceptance gate, not an assumption.

### Integrity is not authority

Snapshot and state digests detect changed bytes under this encoding contract.
They are not signatures and do not establish signer identity, authorization,
or provenance. Hyphae receipt/page authority remains external, and a snapshot
that crosses a trust boundary requires a separate signature and PKI policy.

## Consequences

- Snapshot reuse is conservative: any contract mismatch costs a full replay
  instead of risking silently incorrect state.
- Schema evolution is reviewable as an explicit graph and cannot depend on a
  reducer guessing payload versions.
- Reducer and configuration changes must carry stable identity changes.
- Live and replay behavior become one tested product contract rather than two
  execution paths with assumed equivalence.
- A persisted snapshot is a performance cache, never the source of authority.
- Tail-only restore remains possible while preserving exact history identity.
- Implementations must bound decode work and atomically persist snapshot bytes.
- External snapshot signing, key distribution, and PKI are not implemented by
  this decision.
- R3 binds the local content-history head. Stream identity, signer authority,
  and replay acceptance remain the R2 contract and are not copied into a
  projection snapshot.
- Current mappers, upcasters, reducers, custom codecs, and application
  serializers, deserializers, and equality implementations remain trusted
  callbacks. Exact append round trips, double execution, and bounded observed-
  output caches are tripwires, not proofs of truthful semantics or purity.
- Panic conversion requires an unwinding target. `panic=abort` targets cannot
  turn a callback panic into a typed projection error.
- The built-in JSON codec preflights 8 MiB, depth 64, and 262,144 values, but
  valid raw-log import and full-tail synchronization have no aggregate time or
  event-count budget.

## Rejected alternatives

- **Accumulator plus numeric cursor:** rejected because equal positions can
  refer to different history heads and executable contracts.
- **Bind only a state hash:** rejected because it omits history, schema,
  reducer, configuration, and codec substitution.
- **Use Rust type names or function addresses as identities:** rejected because
  they are not portable or stable release contracts.
- **Let reducers decode arbitrary versions:** rejected because compatibility
  becomes implicit and cannot be sealed, audited, or hashed.
- **Skip intermediate upcast versions:** rejected because it hides migration
  semantics and makes graph validation ambiguous.
- **Migrate projection snapshots automatically:** rejected because state
  migration is a separate trust and determinism boundary.
- **Install state before checking all metadata:** rejected because restore
  failure could expose partial or incompatible application state.
- **Describe hashes as provenance:** rejected because a digest has no signer or
  authorization context.

## References

- [Event schema and snapshot contract](../30-event-schema-and-snapshot-contract.md)
- [R3 acceptance evidence](../evidence/r3-snapshot-schema.md)
- [Hyphae verified sync protocol decision](ADR-004-hyphae-verified-sync-v2.md)
- [Hardening roadmap](../28-hardening-roadmap.md)
