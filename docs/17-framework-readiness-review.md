# Framework readiness review

**Reviewed:** 2026-07-15
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
- The official PliegoRS site and neutral references as native framework evidence.
- Manual five-target GitHub Actions candidate workflow and installer lifecycle.

## Trust boundary

The static review at commit `934a5cf` identified five P0 areas that supersede
the earlier readiness conclusion until they are reproduced or closed:

1. reactive child reclamation and arena retention;
2. panic safety and update reentrancy;
3. normalized output-path collisions;
4. complete exact-set artifact ledgers with deterministic integrity;
5. cryptographically verified and type-gated Hyphae replay.

The full findings and acceptance sequence are in the
[hardening roadmap](28-hardening-roadmap.md).

## Claims PliegoRS can make now

- It authors complete static documents and focused Rust/WASM interaction.
- It integrates native JavaScript libraries through an explicit lifecycle API.
- It has deterministic build and adaptive-asset foundations with automated
  tests.
- It has a fail-closed Hyphae protocol v2 client contract without claiming a
  production gateway, key distribution system, durable browser outbox, or
  deployed conforming Hyphae service.

## Claims blocked before R6

- complete memory safety across long-running reactive ownership cycles;
- forge-resistant provenance from a content hash alone;
- production-verified Hyphae synchronization;
- reproducible public binaries with independent authenticity verification;
- broad server-framework parity;
- 1.0 API stability or public support commitments.

## Next review

Repeat this review after R0-R4 close with committed adversarial evidence,
lifecycle plateau results, exact artifact verification, and rejected replay
fork/gap/authority/attestation fixtures. Client-side R2 closure must remain
separate from production sync readiness. Release readiness is then decided
against R5-R6, not visual completeness.
