# ADR-009: StateRoot v1 derives from verified projection snapshots

**Status:** Accepted for source-preview implementation
**Decision date:** 2026-08-09
**Scope:** deterministic projection identity for adoption, HMR, inspection, and receipts

## Context

PliegoRS already has stronger state inputs than the initial Next proposal:
exact `LogCursor`, sealed schema-set digest, reducer identity and configuration,
codec identity and configuration, canonical state bytes, state digest, and a
snapshot digest over the complete snapshot envelope.

SSR adoption and causal HMR need one small comparable value. Reusing only the
state digest would ignore history and executable contracts. Reusing the snapshot
digest directly would couple the public concept to the persisted snapshot format
and make future StateRoot evolution implicit.

## Decision

`StateRoot` is owned by `pliego-fold` while it has one consumer boundary. It is
not a new umbrella crate.

Version 1 is SHA-256 over this exact preimage:

```text
"pliego-fold/state-root/1\0"
+ snapshot_format:u16 big-endian
+ history_position:u64 big-endian
+ history_head:32 bytes
+ schema_set_digest:32 bytes
+ reducer_id_length:u16 big-endian
+ reducer_id:UTF-8 bytes
+ reducer_revision:u64 big-endian
+ reducer_config_hash:32 bytes
+ codec_id_length:u16 big-endian
+ codec_id:UTF-8 bytes
+ codec_revision:u64 big-endian
+ codec_config_hash:32 bytes
+ state_digest:32 bytes
+ snapshot_digest:32 bytes
```

The input is an integrity-checked `ProjectionSnapshot`. No constructor accepts
independent unverified fields. `Projection::state_root()` first creates the same
snapshot used by checkpointing and derives the root from it.

The root string form is:

```text
sha256:<64 lowercase hexadecimal characters>
```

`StateRoot` is deterministic integrity evidence. It is not a signature,
authority statement, provenance receipt, or replacement for accepted event
history.

## Consequences

- Equal history, schemas, reducer, codec, canonical state, and snapshot contract
  produce the same root.
- Any bound snapshot mutation changes the root or fails snapshot validation.
- StateRoot has its own domain/version and can evolve without reinterpreting
  `pliego-fold/snapshot/1`.
- Snapshot files, event hashes, and current SHA-256 fields do not change.
- Initial-state identity remains the reducer configuration responsibility fixed
  by ADR-005.
- Stream identity and signer authority remain external Hyphae/R2 contracts.

## Rejected alternatives

- **State digest only:** omits history and executable contracts.
- **Snapshot digest alias:** provides no independently versioned StateRoot
  contract.
- **BLAKE3 migration in place:** would reinterpret existing SHA-256 contracts.
- **Public field constructor:** could create roots from unverified combinations.
- **New `pliego-core` crate now:** premature for one projection-owned contract
  and would block coordinated publication while source-preview.

## References

- [Projection snapshot decision](ADR-005-projection-snapshots.md)
- [Event schema and snapshot contract](../30-event-schema-and-snapshot-contract.md)
- [PliegoRS Next transition](../52-pliegors-next-transition.md)
- [Phase 0 smoke](../evidence/next-phase0-smoke-2026-08-09.md)
