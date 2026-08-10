# ADR-011: Integrate Hyphae Native through a loopback sidecar

**Status:** Accepted for fixture and adapter implementation
**Decision date:** 2026-08-10
**Scope:** Reduced Hyphae Console, Native integration, process boundary, MSRV, and acceptance claims

## Context

Hyphae `v1.0.1` is published at commit
`84161cf067141b60f4847b965ef77c5b749749c0`. Its retained G6 evidence covers the
bounded embedded product, local daemon, HTTP `/v2`, SDKs, administration,
backup/restore, and cross-surface conformance on Linux, macOS, and Windows.

Hyphae requires Rust `1.89`; PliegoRS supports Rust `1.86`. Directly linking
`hyphae-client`, `hyphae-native-product`, or the current `hyphae-pliegors` crate
would raise the framework MSRV. The current `hyphae-pliegors` crate also wraps
the legacy `/v1` format-2 client rather than the Native product.

Native HTTP `/v2` is an optional loopback-first adapter over one existing
product owner. It uses canonical bounded product envelopes and exposes
capabilities, catalog, SQL, structures, search, transactions, proofs,
telemetry, doctor, backup, restore, and administration. Its OpenAPI version is
still `2.0.0-alpha`.

Hyphae's formal source status leaves G7 and G8 open. G7 controls dedicated-host
performance and scale certification. Hosted G8 workflows ran for `v1.0.1`, but
Hyphae has not reconciled those runs into its durable source-level gate status.
The pending Native V3/G7 change does not alter the HTTP `/v2` route set or local
protocol, but it is not a released integration target.

## Decision

1. The reduced Hyphae Console uses a separately installed Hyphae process and a
   PliegoRS-owned server-side adapter. No Hyphae crate enters the PliegoRS Cargo
   graph.
2. The first adapter targets exact release `v1.0.1` and exact commit
   `84161cf067141b60f4847b965ef77c5b749749c0`. Floating tags, branches, and
   prerelease source are not accepted inputs.
3. The selected transport is HTTP `/v2` on loopback. PliegoRS does not expose
   that listener to browsers and does not forward raw Hyphae credentials.
4. The adapter checks process identity, capabilities, Product API version,
   Native directory format, media types, request bounds, deadlines, and typed
   errors before admitting the sidecar. Unknown error codes remain errors and
   use a bounded fallback representation.
5. PliegoRS owns simulated application authentication, authorization, tenant
   scope, process supervision, startup and shutdown, and redaction. Hyphae owns
   its data directory and product operation semantics.
6. The adapter presents Pliego-owned application types. Hyphae protocol,
   OpenAPI, SDK, and runtime types do not enter public framework APIs.
7. `hyphae-console` advances from `deferred` to `specified`. It becomes
   `executable` only after the adapter passes exact-release, loopback,
   cancellation, persistence, restart, shutdown, browser-isolation, and hostile-
   response tests.
8. G7 does not block adapter implementation. It blocks latency, saturation,
   production-scale, and comparative-performance claims.
9. G8 durable source-level closure and the completed PliegoRS sidecar adapter
   remain required for final fixture acceptance and a complete Phase 0 baseline.
10. A Hyphae release other than `v1.0.1` requires an explicit compatibility
    review and a refreshed fixture source identity before measurement.

## Consequences

- PliegoRS keeps its Rust `1.86` MSRV and does not inherit Hyphae's dependency
  graph.
- Adapter design and fixture implementation can proceed without waiting for the
  Native V3/G7 performance work.
- The sidecar adds process lifecycle, installation, and local transport failure
  modes that must be tested and diagnosed.
- HTTP `/v2` is simpler to isolate than the binary UDS/named-pipe protocol but
  is not accepted as a primary Hyphae performance surface.
- The existing PliegoRS verified-sync v2 contract remains separate. It governs
  signed append/pull replay and is not reinterpreted as the Native product API.
- A green reduced Console does not by itself prove Hyphae production-scale
  performance, hosted security, replication, clustering, or G7/G8 closure.

## Rejected alternatives

- **Direct Rust dependency:** rejected because it raises the PliegoRS MSRV from
  `1.86` to `1.89` and couples the fixture to Hyphae's internal crate graph.
- **Current `hyphae-pliegors` crate:** rejected because it targets the legacy
  `/v1` format-2 client, not Native HTTP `/v2`.
- **Embedded Native product:** rejected for the initial fixture because it has
  the same MSRV coupling and gives PliegoRS direct data-directory ownership.
- **Browser-to-Hyphae connection:** rejected because browser code must not hold
  raw Hyphae credentials or bypass PliegoRS authorization and scope.
- **Wait for G7 before any work:** rejected because G7 governs performance
  claims, while G6 already closes the bounded functional integration surfaces.
- **Build against the pending V3 branch:** rejected because branches are not
  release authority and its changes require a new version after merge.

## References

- [Hyphae release v1.0.1](https://github.com/celiumsai/hyphae/releases/tag/v1.0.1)
- [Hyphae Native HTTP v2 contract](https://github.com/celiumsai/hyphae/blob/v1.0.1/docs/native/http-v2.md)
- [Hyphae Native gate status](https://github.com/celiumsai/hyphae/blob/v1.0.1/docs/gates/native-gate-status.md)
- [Hyphae pending Native V3/G7 work](https://github.com/celiumsai/hyphae/pull/136)
- [PliegoRS verified-sync v2 decision](ADR-004-hyphae-verified-sync-v2.md)
- [PliegoRS Next transition](../52-pliegors-next-transition.md)
