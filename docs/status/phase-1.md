# Phase 1 status

**Phase:** manifests and physical measurement protocol
**State:** deterministic tooling complete; physical matrix pending
**Last updated:** 2026-07-14

## Completed

- JSON Schema 2020-12 contracts for asset manifests, device fingerprints,
  measurement plans, route runs, reports, trace metrics, and budget waivers.
- A deterministic public manifest for the official PliegoRS site.
- Rust `pliego-inspect` human and JSON reports with route-scoped budgets.
- Route Lab origin isolation, early probe injection, session continuity, raw
  segment ledgers, cache-state validation, immutable receipts, and source seals.
- Device Lab capture with review-before-accept semantics.
- Shared evidence auditing for daily and closure gates.
- CI validation through pinned Rust and Node dependencies.

Private product fixtures and physical-device fingerprints are deliberately not
part of the public source distribution. Public performance claims require a
separate anonymized evidence bundle that passes the same schemas and hashes.

## Reproducible gate

```sh
npm run check:phase-1
```

The gate checks Rust formatting, inspector tests, exact baseline bytes, Draft
2020-12 validation, and the accepted-evidence graph. Loose runs, incomplete
cases, missing artifacts, mixed collector versions, or mismatched trace session
IDs never increase accepted counts.

The stricter closure audit remains red until the required physical matrix is
complete:

```sh
npm run check:phase-1-closure
```

## Still open

- Capture anonymized physical references for modest Android, iPad Safari,
  integrated-GPU laptop, and capable discrete-GPU desktop classes.
- Produce five accepted cold and warm runs per canonical route and tier.
- Attach traces, screenshots, and run ledgers for transfer, decode, main-thread,
  VRAM, draw calls, triangles, frame time, LCP, INP, and CLS.

## Gate decision

The deterministic inventory and tooling gates are green. Phase 1 closure remains
blocked on physical evidence and cannot be replaced with emulation or estimates.
