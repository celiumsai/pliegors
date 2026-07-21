# PliegoRS execution backlog

**Updated:** 2026-07-19
**Objective:** preserve R0-R7 and P8 as regression gates while PliegoRS executes
the G0-G7 full-stack evolution program. OpenSDK remains a bounded preview and
advances only where the runtime requires a public extension contract.

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
| 9 | P8 Trust and adoption | Complete | [P8 contract and audited baseline](35-p8-trust-and-adoption-contract.md), the signed [`v0.0.2`](https://github.com/celiumsai/pliegors/releases/tag/v0.0.2) release, and the release-only golden matrix cover product stability, CLI diagnostics, release identity, adversarial suites, benchmarks, clean environments, and opt-in-only telemetry. |
| 10 | P9 OpenSDK preview | Implemented; governance pending | [OpenSDK foundation](42-opensdk-foundation.md), [multilang conformance](43-opensdk-multilang-conformance.md), [browser-framework conformance](44-browser-framework-conformance.md), [tooling protocol](45-opensdk-tooling-protocol.md), and [compatibility policy](46-opensdk-compatibility-and-deprecation.md) cover the preview implementation. RFC-006 and RFC-007 remain Draft, and ADR-006 remains Proposed, until formal governance review. |

The [product constitution](34-product-constitution.md) governs admission of all
work after R7. P9 entered preview only after the P8 release gates closed. Its
implemented conformance surface does not imply RFC or ADR acceptance, stable
API status, or release of the preview crate.

## Full-stack evolution program

| Gate | Status | Exit evidence |
| --- | --- | --- |
| G0 Product truth | Complete | [Accepted G0 evidence](evidence/g0-product-truth.md) covers the canonical manifest, public-surface consistency checker, runtime/route/data RFCs, threat model, Linux site build, and external release/registry snapshot. |
| G1 Native runtime and dynamic rendering | In progress | [Runtime](evidence/g1-native-runtime-foundation.md), [native-socket](evidence/g1-native-socket-foundation.md), [complete-render](evidence/g1-complete-render-foundation.md), [ordered-render](evidence/g1-ordered-render-foundation.md), [middleware/error](evidence/g1-middleware-error-foundation.md), and [dynamic-reference](evidence/g1-dynamic-reference-foundation.md) evidence cover the unreleased route graph, lifecycle, raw TCP HTTP/1.1 path, two bounded SSR modes, pre-route plus inherited group/layout/route middleware, exact capability admission and effect mediation, safe authored errors, and one launchable native application. Gate closure still requires async boundaries, layout-owned document composition, HTTP/2, fixed-load and bounded-memory evidence, OTel, security evidence, and no unresolved P0. |
| G2 Data, actions, and cache | Not started | Progressive authenticated mutation across two instances with idempotency, cancellation, cache isolation, and bounded invalidation lag. |
| G3 Portable deployment | Not started | The same sealed build and conformance corpus pass native/OCI and Cloudflare hosts. |
| G4 Adoption | Not started | An unaffiliated team completes a greenfield application and partial migration using public resources only. |
| G5 OpenSDK ecosystem | Preview foundation only | Reviewed server plane, package lock/resolution, generated SDKs, registry/discovery, and independent non-Rust extension. |
| G6 Operational maturity | Not started | Exercised release/support/security policy with owner redundancy and incident drill. |
| G7 Competitive claim | Not started | Reproducible comparison, three external production deployments, public limitations, and approved claim wording. |

The critical path is `G0 -> G1 -> G2 -> G3`. G4 may perform read-only migration
analysis during G1/G2. G5 cannot stabilize the buffered HTTP preview ahead of
the runtime lifecycle. G6 is designed from G0 and closes only when it can be
staffed. G7 cannot close from internal demos or tests.

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
- No Pliego.run control-plane, billing, dashboard, or infrastructure code in this
  open-source repository.
- No required PliegoCSS dependency. Its separately installed compiler may be
  used as an experimental build-time companion without entering the G1-G3
  critical path.
