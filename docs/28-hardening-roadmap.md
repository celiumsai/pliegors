# PliegoRS hardening roadmap

**Adopted:** 2026-07-14
**Input review:** repository state at commit `934a5cf`
**Purpose:** make PliegoRS a framework for verifiable, replayable, durable web
software rather than a generic clone of an existing frontend framework.

This roadmap is normative for work after the current native framework
foundation. Findings from the source review remain open until reproduced or
closed by tests against the current tree.

## Product thesis

PliegoRS owns five durable contracts:

1. **Events:** typed facts with explicit authority and schema versions.
2. **Folds:** deterministic projections whose live and replayed state match.
3. **Effects:** external work represented by requests and verifiable receipts.
4. **Artifacts:** deterministic output bound to sources, toolchains, and bytes.
5. **Lifecycles:** mounted scopes own their reactive children, DOM, listeners,
   adapters, cancellation, and cleanup.

Hyphae is optional for static projects and first-class when durable sync,
objects, documents, or provenance are required.

## P0 findings

| ID | Risk | Closure gate |
| --- | --- | --- |
| P0-01 | Reactive child cleanup can be lost and arena storage can retain unreachable nodes. | Ownership-tree tests reclaim children and memory plateaus after 10,000 create/dispose cycles. |
| P0-02 | A panic or nested update can poison scheduler state or produce reentrant execution. | Panic recovery restores scheduler invariants; update order is deterministic and covered by adversarial tests. |
| P0-03 | Distinct routes can collide after output-path normalization. | Route registration rejects every normalized collision before staging or replacing output. |
| P0-04 | The build ledger can be shallow or internally inconsistent. | The final ledger binds all emitted bytes, source identities, toolchain pins, configuration, and verified previous ownership evidence without claiming signature-based authenticity. |
| P0-05 | Hyphae replay can accept data without complete cryptographic and type verification. | Replay is type-gated and rejects invalid signatures, receipts, gaps, forks, authorities, and unsupported event versions. |

P0-01 and P0-02 are closed by the committed
[R0 reactive safety evidence](evidence/r0-reactive-safety.md) and runtime commit
`ff60575f8c0f16164ecfc1754eac165ae776d33a`. The historical risks remain above
so later changes can be checked against the original failure modes.

## P1 and P2 findings

- Validate element and attribute names at DOM construction boundaries.
- Bind listeners and external adapters to mounted scopes with automatic cleanup.
- Make keyed reconciliation prove node identity and minimal operations.
- Bind snapshots to history head, schema set, reducer identity, and canonical
  encoding; never trust an isolated state hash.
- Lock compiler and external tool versions for reproducible builds.
- Make release evidence exercise the full golden path on every supported target
  and sign the final uploaded manifest, not an earlier local approximation.
- Reject values outside JavaScript's safe integer range at JS/WASM boundaries.
- Define event taxonomy, authority, conflicts, selective sync, compaction,
  privacy/erasure, effects, receipts, and unknown-version policy explicitly.

## Architecture boundary

| Plane | Responsibility |
| --- | --- |
| Kernel | events, schemas, folds, effects, snapshots, canonical encoding |
| Runtime | reactive graph, DOM ownership, SSG, resumability, adapters |
| Sync | outbox, cursors, partial sync, conflicts, verified replay |
| Tools | timeline, replay, diff, fork, diagnostics, provenance |

The Studio/devtools surface is central: developers must be able to inspect an
event, replay a state, compare projections, fork history, and understand the
authority behind a value.

## Ordered gates

### R0 - Reactive safety

Close ownership reclamation, panic safety, reentrancy, and scheduler invariants.
Status: complete. See [R0 reactive safety evidence](evidence/r0-reactive-safety.md).

### R1 - Artifact trust

Close normalized-path collisions, strengthen the content-addressed ledger, and
lock the build/toolchain identity.

### R2 - Verified sync

Make Hyphae receipts, authority, cursors, event versions, gaps, forks, and
selective replay fail closed.

### R3 - Snapshot and schema contract

Introduce explicit schema evolution and upcasters; bind snapshots to the exact
history and projection contract.

### R4 - DOM lifecycle

Validate names, make mounted scopes own listeners and nodes, prove keyed
reconciliation, and demonstrate cleanup plateaus.

### R5 - Golden developer experience

Generate event and projection scaffolding, offer progressive operating modes,
produce compile-time diagnostics, and ship replay/timeline tests in starters.

### R6 - Candidate distribution

Run the complete cross-platform golden path, create exact release assets, sign
the final manifest, verify installers, and keep the candidate private until a
separate opening decision.

### R7 - External flagship

Build an auditable human-agent workspace outside the framework repository. The
flagship must exercise events, effects, history, replay, provenance, and
selective sync; another marketing site is not sufficient evidence.

## Metrics

- Live projection equals replayed projection for every recorded scenario.
- Reactive and DOM allocations plateau after 10,000 lifecycle cycles.
- Scheduler recovers after injected panics without lost work or poisoned state.
- Snapshot restore invokes reducers in `O(tail)` rather than `O(history)`.
- Keyed updates perform the expected DOM operations and preserve node identity.
- No build artifact exists outside the final ledger.
- No fork, gap, invalid receipt, or unknown authority is accepted.
- Install-to-first-replayable-app time and build p50/p95 are measured.
- No public claim is made without committed or release-bound evidence.

## Golden path

Starters and documentation should expose four progressive modes:

1. static output only;
2. local event history;
3. durable outbox;
4. verified synchronization.

Unknown event versions must have an explicit reject, quarantine, or upcast
policy. Every maintained starter includes live-versus-replay parity tests and a
timeline inspection path.
