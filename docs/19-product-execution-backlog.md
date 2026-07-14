# PliegoRS execution backlog

**Updated:** 2026-07-14
**Objective:** reach a verified release candidate by closing trust and lifecycle
gates before adding broad framework surface.

The normative hardening sequence is defined in
[`28-hardening-roadmap.md`](28-hardening-roadmap.md). A task is complete only
when its acceptance evidence is committed or attached to the exact release
candidate.

## Current order

| Order | Gate | Status | Acceptance evidence |
| --- | --- | --- | --- |
| 1 | R0 Reactive safety | Active | Ownership trees reclaim children, panics cannot poison the scheduler, and nested updates are deterministic. |
| 2 | R1 Artifact trust | Pending | Output paths are collision-safe; the ledger binds every byte, toolchain input, and source identity. |
| 3 | R2 Verified sync | Pending | Hyphae replay rejects gaps, forks, invalid receipts, unknown authority, and incompatible event versions. |
| 4 | R3 Snapshot and schema contract | Pending | Snapshot identity binds schema, reducer, history head, and canonical encoding; upcasters are explicit. |
| 5 | R4 DOM lifecycle | Pending | Mounted scopes own listeners and nodes; keyed reconciliation is precise; 10,000 mount/dispose cycles plateau. |
| 6 | R5 Golden developer experience | Pending | A starter reaches its first replayable app with generated events, projections, diagnostics, and timeline tests. |
| 7 | R6 Candidate distribution | Pending | Five target artifacts, final signed manifest, installer lifecycle, reproducibility evidence, and private review are green. |
| 8 | R7 External flagship | Pending | An auditable human-agent workspace is built outside the monorepo using only the candidate distribution. |

## Already implemented foundations

- `pliego new`, `check`, `build`, `dev`, `preview`, `inspect`, official starters,
  typed content, deterministic SSG, Rust/WASM clients, adaptive assets, and
  plugin lifecycle API v1.
- The official site exercises native Rust routes and focused JavaScript
  ecosystem adapters without shipping private acceptance applications.
- The manual GitHub Actions release workflow builds Linux x64/ARM64 production
  candidates and macOS x64/ARM64 plus Windows x64 development candidates.
- GitHub Releases is the sole canonical distribution authority; no mirror or
  secondary download origin exists.

These foundations are not substitutes for the R0-R7 evidence gates.

## Explicit non-goals before R6

- No generic JavaScript bundler, identity system, database, SQL layer, or CRDT.
- No event logging for ephemeral pointer frames, hover states, or animation ticks.
- No requirement for Hyphae in static-only projects.
- No promise of a total global event order.
- No hash presented as provenance without authority and verification context.
- No broad server-framework expansion before reactive safety and artifact trust.
