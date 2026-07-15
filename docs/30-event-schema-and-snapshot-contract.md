# Event schema and projection snapshot contract

**Contract version:** R3
**Status:** accepted; cross-target evidence recorded on 2026-07-15
**Scope:** typed application events, explicit schema evolution, transactional
projections, and fail-closed projection snapshots
**Last reviewed:** 2026-07-15

## Boundary

R3 defines how checked application history becomes reproducible application
state. That history is authority-verified when it arrives through R2; a purely
local `Log` supplies integrity and exact-cursor checks without claiming remote
authority. R3 does not change the Hyphae v2 signature domains, choose a signing
algorithm, or make a local state file authoritative.

Two different concepts use the word snapshot and must not be confused:

- A Hyphae pull snapshot is a signed stream checkpoint used to keep a paged
  pull cycle consistent.
- A projection snapshot is a local optimization containing derived state and
  the exact contract needed to reproduce it.

A projection snapshot is disposable. The accepted event history remains the
source of truth. Restore may avoid folding the prefix again only when every
binding described below matches exactly.

```text
verified accepted history
    -> sealed event catalog
    -> explicit upcast chain
    -> transactional reducer
    -> canonical projection state
    -> history- and contract-bound snapshot
```

## Application event identity

An application event is identified by the pair `(kind, schema_version)`.

- `kind` is a validated `app_*` discriminant. Framework and transport records
  cannot masquerade as application facts.
- `schema_version` is a positive integer. Version zero, an unknown version, or
  an unknown kind fails closed.
- The payload is valid JSON and is converted to the contract's canonical JSON
  bytes before schema validation, upcasting, digesting, or reduction.
- Event identity, durable order, authority, and receipt remain properties of
  the verified Hyphae envelope. Canonicalizing a payload does not replace those
  properties.

Typed application code should decode a catalog-admitted payload into the Rust
type registered for that exact kind and version. A reducer must never dispatch
on an unchecked string and unvalidated JSON value.

Typed append also requires the declared schema value to survive its own wire
representation exactly. `T` must implement `DeserializeOwned + PartialEq`; the
framework canonically serializes `&T`, decodes those bytes back to `T`, and
requires equality before it constructs or retains an `Event`. This rejects
asymmetric Serde contracts such as `skip_serializing` on a required field and a
skipped field whose deserialization default would silently lose a non-default
value.

## Canonical JSON

Canonical JSON removes irrelevant representation choices before a value enters
a digest or contract comparison. The canonical encoder operates on a parsed
JSON value and emits deterministic UTF-8 bytes:

- objects are serialized with keys in deterministic lexical order;
- arrays preserve their declared order;
- insignificant whitespace is absent;
- strings use one deterministic JSON escaping form;
- booleans and null use their lowercase JSON literals;
- decimal number lexemes are normalized exactly, including exponent arithmetic,
  without conversion through `f64`;
- the private `serde_json` arbitrary-precision sentinel key is rejected at
  every object depth so application JSON cannot impersonate an internal number;
  and
- source JSON plus recursively serialized Rust values admitted through
  `CanonicalJson::from_serialize` or `Log::append_typed` reject `NaN`, positive
  infinity, and negative infinity instead of silently producing a payload that
  its declared event type cannot decode.

Equivalent parsed objects therefore produce the same bytes regardless of
source whitespace, object insertion order, or equivalent decimal spelling.
Arrays, exact number values, string contents, and JSON types remain significant.
This contract does not claim that arbitrary non-JSON host values have a
canonical representation or RFC 8785 compatibility.

Canonical JSON has three distinct uses in R3:

1. canonical event payload bytes;
2. canonical reducer configuration bytes; and
3. canonical state bytes when the selected snapshot codec is JSON.

Callers must not compare or hash the original source text when a canonical
value is required.

## Sealed schema catalog

The event catalog is mutable only during construction. Sealing it validates
the complete schema graph and produces the catalog digest used by snapshots.
After sealing, registrations cannot be added, removed, or replaced.

For every application kind, the sealed catalog defines:

- every admitted schema version;
- the concrete payload validator for each version;
- the reducer-facing target version; and
- each explicit adjacent upcaster needed to reach that target.

Catalog construction or sealing fails on duplicate `(kind, version)`
registrations, invalid application kinds, missing current schemas, ambiguous
transitions, schemas beyond the current version, gaps, or an upcaster that is
cross-kind or non-adjacent. The adjacent, monotonically increasing API makes
cycles and downgrades unrepresentable. Registration order must not affect the
resulting catalog digest.

The digest is computed from a deterministic catalog descriptor, not from
function addresses or compiler-dependent type names. The descriptor binds each
kind, admitted version, target version, schema identity, and declared upcast
edge. Changing any of them changes the digest.

### Explicit upcasting

Upcasting is a pure, deterministic migration of one admitted event payload to
the next declared version of the same kind.

```text
app_document_saved@1
    -> declared 1-to-2 upcaster
app_document_saved@2
    -> declared 2-to-3 upcaster
app_document_saved@3 (reducer input)
```

There is no implicit "latest" conversion. Every intermediate edge must exist,
and the result of each edge is canonicalized and validated against the exact
destination schema before the next edge runs. Upcasters cannot change event
kind, durable identity, authority, order, receipt, or history hash.

An upcaster failure rejects the event before a reducer transaction begins.
Unknown versions are never guessed, truncated, or passed through as opaque
reducer input.

### Rust event surface

The R3 event surface lives in `pliego-log`:

- `EventSchema` supplies the stable `KIND`, positive `VERSION`, and portable
  `SCHEMA_ID` for one Rust payload type.
- `CanonicalJson::parse` admits untrusted bytes; `from_serialize` admits a Rust
  value. Both produce the same compact, recursively key-sorted representation.
- `CanonicalJson` rejects duplicate keys, values deeper than 64 levels, more
  than 65,536 JSON nodes, and source or output larger than 256 KiB.
- `Event` fields are private. Its accessors expose the zero-based sequence,
  kind, source schema version, canonical payload, previous hash, and event hash.
- `Log::append_typed` is the only normal application append path. There is no
  public free-form append. It must finish bounded canonical serialization first:
  a recursively encountered `NaN`, positive infinity, or negative infinity
  returns an error before an `Event` is created or the log is mutated. It then
  decodes the canonical bytes through the same schema type and requires exact
  `PartialEq` equality before mutation.
  `Log::import_raw` is the explicit wire/storage boundary: it canonicalizes each
  payload and verifies every serialized field plus the complete chain before
  returning a `Log`.
- `LogCursor` carries `position` plus `head_hash`. `Log::tail` requires an
  exact matching cursor and never clamps an out-of-range position.
- `EventCatalogBuilder::register_current(mapping_id, mapper)` registers the
  reducer-facing type plus a stable identity for the typed mapping;
  `register_upcaster(step_id, upcaster)` registers a named adjacent edge; and
  `seal` returns an immutable `SealedEventCatalog`.
- `SealedEventCatalog::decode` is the only catalog path to reducer-facing type
  `E`; `supports` and `schema_set_digest` expose exact admission and identity.

Current mappers and upcasters run twice for each decode and are compared with a
bounded cache of previously observed input/output pairs. This is an observed-
divergence tripwire, not a proof of determinism. The `mapping_id`, `step_id`,
schema identities, and graph shape are digest-bound; arbitrary callback code is
not hashed or sandboxed.

Local log event hashes use the literal `pliego-log/2` domain, fixed big-endian
integers, and length-prefixed variable fields. The schema-set digest uses the
separate `pliego-log/2/schema-set` domain. This local chain is not a substitute
for the authority-bearing Hyphae receipt chain.

## Transactional projection contract

A projection reducer consumes an ordered batch of catalog-admitted, target-
version application events. Applying a batch is atomic from the framework's
point of view:

1. clone or prepare a candidate state and candidate cursor;
2. reduce every event into that candidate in durable order;
3. encode, decode, compare, and re-encode the candidate with the selected codec;
4. calculate every fallible counter and checkpoint value needed to publish; and
5. publish state bytes, state, cursor, and counter together only after the
   entire batch succeeds.

If cursor validation, schema admission, upcasting, decoding, reduction, codec
validation, panic containment, or counter calculation fails, the prior
projection state, canonical state bytes, and cursor remain unchanged. Snapshot
creation reuses those already committed bytes and performs no new codec call. A
reducer must not perform network, filesystem, clock, random, DOM, or other
external effects. Those actions belong in explicit effect requests and
receipts.

Reducer identity is data, not a function address. A projection contract binds
a stable reducer identifier and revision plus the digest of canonical reducer
configuration. The initial state and every configuration value that can alter
the fold must be included in that configuration contract. A code, initial-
state, or configuration change that can alter the fold must change the revision
or configuration digest and invalidate older snapshots; the framework cannot
infer omitted application semantics.

## Projection snapshot bindings

A projection snapshot binds all information needed to decide whether its state
is reusable:

| Binding | Required meaning |
| --- | --- |
| Snapshot format | Exact projection-snapshot protocol and format version. |
| History head | Exact durable position and head hash folded into the state. |
| Schema catalog digest | Digest of the complete sealed event catalog. |
| Reducer identity | Stable reducer identifier and semantic revision. |
| Reducer configuration digest | Digest of canonical configuration that affects the fold. |
| Codec identity | Exact codec identifier, semantic revision, and configuration digest used for state bytes. |
| State digest | Digest of the exact encoded state bytes. |
| Snapshot digest | Digest binding all snapshot metadata, exact state bytes, and the state digest. |

The state digest detects state-byte corruption. The snapshot digest detects
metadata substitution and binds metadata, those exact state bytes, and their
state digest together. The snapshot digest excludes its own field from its
preimage.

Timestamps, filenames, transport headers, and storage locations are not
substitutes for any required binding. If optional metadata is retained, it
must not change the deterministic contract digest unless it is explicitly part
of that contract.

### Codec rules

A codec name without a revision and configuration digest is insufficient.
`CodecIdentity` binds all three values. Restore accepts only the exact identity
recorded by the snapshot and expected by the active projection. The codec must
be deterministic for a given admitted state.

The built-in `CanonicalJsonCodec` caps state at 8 MiB and preflights JSON depth
64 and at most 262,144 values before deserializing. A custom codec is still
subject to the framework's 8 MiB encoded-state ceiling, but it owns any
additional structural limits and canonicalization rules. A decoder that happens
to parse bytes from another codec or revision does not make them compatible.

## Fail-closed restore

Restore validates before installing any caller-visible state. `restore_bytes`
first decodes a bounded binary envelope; `restore` accepts only the resulting
integrity-checked value. The implemented order is:

1. enforce the complete envelope and encoded-state byte bounds;
2. validate magic, format, identifier shapes, lengths, UTF-8, and no trailing
   bytes before constructing the snapshot value;
3. recompute and compare the state digest, then the snapshot digest;
4. require the active sealed catalog digest;
5. require reducer identifier, revision, and configuration digest;
6. require codec identifier, revision, and configuration digest;
7. require the exact local history position plus head hash;
8. decode, compare, and re-encode state with the exact codec; and
9. construct the stable checkpoint and transactionally fold the checked tail.

Any mismatch is a typed restore failure. Restore does not silently fall back,
partially install state, accept a same-position history fork, search for a
"close enough" schema, or invoke hidden snapshot migration. The caller may
explicitly choose a full replay after observing the failure.

After a successful restore, only the checked history strictly after the bound
head is folded. The first tail event must extend that exact local content head.
A gap, fork, or overlapping event with changed identity remains a history-
verification failure. R3 does not put a stream identifier, signer, or authority
policy into the projection snapshot. When the local log is fed by Hyphae, stream
identity and authority remain bound by the R2 `ReplayState`; the projection
snapshot does not duplicate or replace that external trust state.

Projection state snapshots are not upcast in place. When a projection contract
changes, the safe default is full replay under the new sealed catalog and
reducer. A future explicit state-migration facility would need its own identity,
canonical bytes, audit evidence, and fail-closed transition contract.

### Rust projection surface

The R3 materialized projection surface lives in `pliego-fold`:

- `StateCodec<S>` supplies `CodecIdentity` (`id`, `revision`, `config_hash`) plus
  deterministic `encode` and exact `decode` operations. The built-in
  `CanonicalJsonCodec` uses ID `pliego/canonical-json/1`, revision 1, and a
  configuration digest that includes its byte/depth/node limits.
- `ReducerIdentity::from_serializable_config` derives `config_hash` from
  canonical JSON. `ReducerIdentity::new` accepts a caller-supplied hash only
  when the caller already owns an equivalent canonical configuration contract.
- `Reducer<S, E>` binds a fallible reducer closure to that identity.
- `Projection<S, E>` owns a `SealedEventCatalog<E>` directly; there is no raw
  resolver injection path around the sealed graph.
- `Projection::new` starts at genesis and validates the initial state with the
  codec before construction succeeds. `try_get` and `sync` settle a complete
  transaction, while `snapshot` emits a `ProjectionSnapshot` from the cached,
  atomically committed state bytes.
- `Projection::restore` accepts an integrity-checked value and
  `restore_bytes` begins at the bounded untrusted-byte decoder. Both verify
  catalog, reducer, codec, and exact `LogCursor` before installing state and
  folding the checked tail.
- `ProjectionSnapshot` has private fields. `decode` checks its binary envelope;
  `encode` emits the same canonical envelope; read-only accessors expose format,
  history, schema-set digest, reducer identity, complete codec identity, state
  length, state digest, and snapshot digest.
- `Projection::dispose` is available for explicit lifecycle control, and
  `Drop` automatically disposes the owned reactive memo.

`Fold<S, E>` remains a compatibility alias for `Projection<S, E>`; it does not
create a second reducer or snapshot contract. `ReactiveLog::append_typed`
replicates the `EventSchema + Serialize + DeserializeOwned + PartialEq` bound
and delegates to the same exact round-trip admission path rather than accepting
free-form call-site strings.

## Determinism and parity

The principal R3 invariant is:

```text
live_fold(history) == full_replay(history)
                   == restore(snapshot(prefix)) + replay(tail)
```

Equality means equal canonical state bytes, equal state digest, and equal final
history cursor. Deterministic generated-case tests cover valid histories,
version mixes, batch boundaries, and snapshot split points. They also assert:

- catalog registration order does not affect the sealed digest;
- repeated upcasting produces identical canonical payload bytes;
- reducing the same events in different valid batch partitions produces the
  same result;
- a failed transaction changes neither state nor cursor; and
- mutating any required snapshot binding makes restore fail.

Generated cases supplement fixed examples; they do not replace golden vectors
for canonical bytes and digests.

## Trusted application code and panic behavior

R3 validates data and makes publication transactional; it does not sandbox host
code. Application `Serialize`, `Deserialize`, and `PartialEq` implementations,
reducer closures, current mappers, upcasters, and custom codec methods are
trusted to implement the declared pure contract. Exact typed append round trips,
repeated transform execution, and canonical codec round trips catch observed
asymmetry or divergence, but custom code can lie and no finite test can prove
arbitrary callbacks pure.

Projection calls catch reducer, catalog, and codec panics only on targets built
with unwinding support. On `panic=abort` targets, including stable WASM builds
without WebAssembly exception handling, a panic aborts the instance and cannot
be converted into `ProjectionError`. R3 therefore promises transactional panic
recovery only where Rust unwinding is available; WASM acceptance is compile and
lint coverage, not a browser panic-recovery claim.

## Integrity, authority, and provenance

SHA-256 digests in this contract detect changed bytes under the stated
canonical encoding. They do not establish who created a snapshot, whether the
creator was authorized, or whether the contained history head is genuine.

Authority comes from separately verified Hyphae receipts and attestations or a
future external signature/PKI envelope over the projection snapshot. A local
digest, an unsigned manifest, HTTPS delivery, or possession of a matching hash
must never be described as provenance.

If projection snapshots cross a trust boundary, the deployment must add an
external signature and key policy that binds the complete snapshot digest,
stream scope, signer, and verification time. That PKI policy is outside R3.

## Operational requirements

- Bound snapshot bytes before allocation and reject trailing or unknown fields
  where the concrete format promises a closed shape.
- Persist snapshot bytes atomically; a partially replaced file must fail digest
  validation rather than become current state.
- Record restore mismatch categories without logging event payloads or state
  bytes by default.
- Treat snapshots as rebuildable cache entries. Deletion must never delete the
  accepted history needed for recovery.
- Keep the catalog, reducer package, codec, and generated parity tests in the
  same reviewed release boundary.
- Bound valid-history work at the caller or transport boundary when latency is
  adversarial. `Log::import_raw` has per-event limits but no aggregate event or
  byte ceiling for an otherwise valid iterator, and projection synchronization
  processes the complete valid tail without an event-count or time budget.

## References

- [PliegoRS / Hyphae client protocol](01-hyphae-protocol.md)
- [Hyphae verified sync guide](29-hyphae-verified-sync-guide.md)
- [Projection snapshot decision](adr/ADR-005-projection-snapshots.md)
- [R3 acceptance evidence](evidence/r3-snapshot-schema.md)
- [PliegoRS hardening roadmap](28-hardening-roadmap.md)
