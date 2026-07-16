# PliegoRS execution backlog

**Updated:** 2026-07-16
**Objective:** reach a verified release candidate by closing trust and lifecycle
gates before adding broad framework surface.

The normative hardening sequence is defined in
[`28-hardening-roadmap.md`](28-hardening-roadmap.md). A task is complete only
when its acceptance evidence is committed or attached to the exact release
candidate.

## Current order

| Order | Gate | Status | Acceptance evidence |
| --- | --- | --- | --- |
| 1 | R0 Reactive safety | Complete | [Committed R0 evidence](evidence/r0-reactive-safety.md) covers ownership reclamation, unwind safety, deterministic nested updates, scheduler bounds, and arena plateau. |
| 2 | R1 Artifact trust | Complete | [Committed R1 evidence](evidence/r1-artifact-trust.md) covers the portable namespace, exact output receipts, build inputs, capability-based publication, deterministic starters, and adversarial verification. |
| 3 | R2 Verified sync | Complete | [Committed R2 evidence](evidence/r2-verified-sync.md) covers typestate, authority, attestations, version policy, cursor continuity, bounded replay, and adversarial verification. |
| 4 | R3 Snapshot and schema contract | Complete | [Contract](30-event-schema-and-snapshot-contract.md), [ADR](adr/ADR-005-projection-snapshots.md), and [committed evidence](evidence/r3-snapshot-schema.md) cover exact typed admission, schema evolution, transactional projections, snapshots, and cross-target gates. |
| 5 | R4 DOM lifecycle | Complete | [Contract](31-dom-lifecycle-contract.md) and [committed evidence](evidence/r4-dom-lifecycle.md) cover exact ownership, LIFO cleanup, keyed identity, SSR adoption, adapters, reduced motion, and the 10,000-cycle plateau. |
| 6 | R5 Golden developer experience | Complete | [Contract](32-golden-developer-experience.md) and [committed evidence](evidence/r5-golden-developer-experience.md) cover replayable scaffolding, receipt-bound causal graphs, native watching, typed HMR, why commands, structured diagnostics, and measured first-app p50/p95. |
| 7 | R6 Candidate distribution | Complete | [Contract](33-candidate-distribution-contract.md) and [committed evidence](evidence/r6-candidate-distribution.md) cover five targets, two native replicas, exact binary hashes, a final Ed25519 manifest, installer lifecycles, and a distribution-only golden path. |
| 8 | R7 External flagship | Complete | [Committed R7 evidence](evidence/r7-external-flagship.md) covers Cairn, an independent durable human-agent decision dossier built only from the accepted candidate, with replay, forks, effects, receipts, provenance, audit, selective sync, tamper rejection, and browser acceptance. |

## Already implemented foundations

- `pliego new`, `check`, `build`, `dev`, `preview`, `inspect`, `why artifact`,
  `why-rebuilt`, official starters,
  typed content, deterministic SSG, Rust/WASM clients, adaptive assets, and
  plugin lifecycle API v1.
- The official site exercises native Rust routes and focused JavaScript
  ecosystem adapters without shipping private acceptance applications.
- The manual GitHub Actions release workflow builds Linux x64/ARM64 production
  candidates and macOS x64/ARM64 plus Windows x64 development candidates.
- GitHub Releases is the sole canonical distribution authority; no mirror or
  secondary download origin exists.

These foundations are not substitutes for the R0-R7 evidence gates.

## Explicit non-goals after R7

- No generic JavaScript bundler, identity system, database, SQL layer, or CRDT.
- No event logging for ephemeral pointer frames, hover states, or animation ticks.
- No requirement for Hyphae in static-only projects.
- No promise of a total global event order.
- No hash presented as provenance without authority and verification context.
- No broad server-framework expansion before reactive safety and artifact trust.
