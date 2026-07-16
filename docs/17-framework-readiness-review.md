# Framework readiness review

**Reviewed:** 2026-07-16
**Verdict:** credible pre-release foundation for native static and focused
Rust/WASM sites; not ready for a public compatibility or trust claim.

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
Production Hyphae service operation, public distribution, and external product
acceptance remain separate gates.

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

## Claims blocked before R6

- forge-resistant provenance from a content hash alone;
- production-verified Hyphae synchronization;
- reproducible public binaries with independent authenticity verification;
- broad server-framework parity;
- 1.0 API stability or public support commitments.

## Next review

Repeat this review after R5 closes with the measured first-replayable-app golden
path and developer diagnostics. Client-side R2 closure remains separate from
production sync readiness. Release readiness is decided against R5-R6, and
external product credibility against R7, not visual completeness.
