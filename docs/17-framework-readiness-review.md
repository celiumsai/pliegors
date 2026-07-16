# Framework readiness review

**Reviewed:** 2026-07-16
**Verdict:** ready for a public `0.0.1` pre-release with an external durable
flagship. Production Hyphae synchronization and 1.0 API stability remain out of
scope.

## Implemented foundation

- Generic `pliego.toml` discovery and `new`, `check`, `build`, `dev`, `preview`,
  and `inspect` commands.
- Deterministic staged SSG output with guarded ownership and SHA-256 ledger.
- Typed views, content collections, safe CommonMark, routes, metadata, and SEO.
- Rust/WASM clients and plugin API v1 with lazy admission, capability policy,
  reduced motion, Save-Data, cancellation, update/unmount, and LIFO cleanup.
- Reproducible adaptive media plans for images, video, fonts, and 3D.
- Hyphae client protocol v2 with idempotent batches, signed append/page
  attestations, receipt authority, snapshot-consistent pull, event-version
  admission, and type-gated replay.
- Verified typed/versioned local events, explicit upcasting, transactional
  projections, and history/contract-bound snapshots, with cross-target R3
  acceptance evidence.
- Exact DOM ownership, retained keyed reconciliation, strict versioned SSR
  adoption, adapter scope cancellation, and 10,000-cycle R4 evidence.
- A replayable default application, receipt-bound causal build graph, native
  event watcher, typed CSS/content/adapter HMR, structured diagnostics, and
  measured install-to-first-replayable-app path.
- A five-target release candidate with two matching native binary builds per
  target, a final signed exact-set manifest, native installer lifecycles, and a
  distribution-only first-application gate.
- Cairn, an independent human-agent decision dossier built only from that
  candidate, with exact replay, explicit forks, effect receipts, provenance,
  actor-scoped audit, selective verified sync, and tamper rejection.
- The official PliegoRS site and neutral references as native framework evidence.
- Manual five-target GitHub Actions candidate workflow and installer lifecycle.

## Trust boundary

The static review at commit `934a5cf` identified five P0 areas:

1. reactive child reclamation and arena retention;
2. panic safety and update reentrancy;
3. normalized output-path collisions;
4. complete exact-set artifact ledgers with deterministic integrity;
5. cryptographically verified and type-gated Hyphae replay.

All five are now closed within their documented boundaries by the committed R0,
R1, R2, R3, and R4 evidence. The historical findings and acceptance sequence
remain in the [hardening roadmap](28-hardening-roadmap.md) as regression targets.
Production Hyphae service operation remains a separate gate. Public framework
distribution is closed for `0.0.1` within the signed release, crates.io, legal,
documentation, and support boundaries. External product acceptance is closed
only within the bounded Cairn R7 evidence.

## Claims PliegoRS can make now

- It authors complete static documents and focused Rust/WASM interaction.
- It integrates native JavaScript libraries through an explicit lifecycle API.
- It owns mounted DOM, keyed identity, SSR adoption, and adapter teardown through
  a verified lifecycle contract.
- It has deterministic build and adaptive-asset foundations with automated
  tests.
- It has a fail-closed Hyphae protocol v2 client contract without claiming a
  production gateway, key distribution system, durable browser outbox, or
  deployed conforming Hyphae service.

## Claims blocked after R7

- forge-resistant provenance from a content hash alone;
- production-verified Hyphae synchronization;
- production support for unsigned macOS or Windows artifacts;
- broad server-framework parity;
- 1.0 API stability or public support commitments.

## Next review

R6 is closed by the [candidate distribution contract](33-candidate-distribution-contract.md)
and its [committed acceptance evidence](evidence/r6-candidate-distribution.md).
R7 is closed by the [external flagship evidence](evidence/r7-external-flagship.md).
Client-side verified sync remains separate from production sync readiness.
Repeat this review for the next release line, a production Hyphae conformance
gate, or a change to platform signing and support commitments.
