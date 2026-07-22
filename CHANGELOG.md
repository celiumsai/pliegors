<!-- SPDX-License-Identifier: Apache-2.0 -->

# Changelog

All notable PliegoRS changes are recorded here. The project follows Semantic
Versioning. Before 1.0, minor releases may contain breaking API changes.

## Unreleased

No changes yet.

## 0.2.0-beta.1 - 2026-07-22

This coordinated beta publishes all nineteen framework crates at one exact
version and unifies the CLI, G1 native runtime, G2 data contracts, and OpenSDK
preview. G3 portability and PBOC remain explicitly unreleased.

### Fixed

- Stream source-archive listings through a bounded, UTF-8-strict validator so
  golden-path verification preserves every entry beyond 32 KiB while rejecting
  traversal, non-canonical paths, excessive entries, and oversized listings.

### Added

- Add a bounded native HTTP/1.1 and HTTP/2 accept loop with explicit
  connection, slow-head, read/write idle, multiplexing, flow-control, and
  graceful-drain policies plus real-socket conformance.
- Add route-owned layout shells for ordered and asynchronous boundary streams,
  preserving sealed ownership, pre-commit slot validation, one output budget,
  cancellation, cleanup, and receipt identity.
- Add bounded `pliegors::request` structured completion events, panic-isolated
  receipt sinks, a machine-checked OWASP ASVS 5.0.0 G1 ownership map, and a
  reproducible fixed-load Linux latency/RSS harness.
- Add operator-enabled OpenTelemetry server spans and HTTP metrics across the
  full response-body lifecycle, with default-new traces, opt-in W3C parent
  acceptance, bounded method/route/error cardinality, secret-redaction
  evidence, and coarse timing buckets in runtime receipts.
- Add route-owned complete-document layout composition with typed single-child
  frames, deterministic head merging, exact asset deduplication,
  pre-commit ownership failures, and layout identities in runtime receipts.
- Add bounded asynchronous SSR boundaries with stable inert placeholders,
  declaration-order delivery, configurable concurrency and timeout ceilings,
  post-commit failure semantics, runtime receipts, and a no-JavaScript reference
  route.
- Reactivate the separately installed PliegoCSS `0.1.0-rc.2` companion as an
  optional experimental integration, with a current revision-pinned SSG,
  route/island bundle, manifest, and Rust/WASM cross-repository gate.
- Route scopes now provide bounded group and layout inheritance: middleware
  enters from outermost scope to route and unwinds in reverse, while error
  boundaries resolve route-first without duplicate registration.

- Add the G1 native runtime foundation with a sealed dynamic route
  graph, bounded complete, ordered, and boundary SSR, raw HTTP/1.1 lifecycle evidence,
  pre-route and route middleware, exact capability admission and effect mediation, safe authored
  error boundaries, receipts, cancellation, and a launchable reference app.
- Add the OpenSDK `0.2.0-beta.1` contracts, conformance CLI, typed Wasmtime
  Component host, effect broker, multilang/browser/tooling fixtures, and
  compatibility matrix.
- Add the G2 `pliego-data` public beta with capability-scoped typed
  resources and loaders, progressive actions, truthful commit/cancellation
  state, principal-bound idempotency, server-side sessions, CSRF, opaque
  secrets, outbound HTTP admission, explicit runtime cache domains, and causal
  invalidation.
- Add an executable no-JavaScript full-stack reference application whose two
  native runtime instances share versioned session and idempotency contracts,
  isolate private cache entries, acknowledge read-your-writes invalidation,
  and emit redacted application-contract-bound receipts.
- Add `pliego why request`, `pliego why cache`, and `pliego inspect action`
  diagnostics over bounded versioned receipt and runtime-contract documents.

### Changed

- Raise the next-release MSRV from Rust `1.85.0` to `1.86.0` so the OpenSDK host
  can use Wasmtime `36.0.8`, the first compatible patch line that resolves the
  RustSec advisories affecting the original `34.0.2` prototype.
- Replace the stale website release summary with a bilingual, evidence-linked
  changelog for `0.2.0-beta.1`, the component preview, `0.0.2`, and `0.0.1`, and align current-release copy
  across the site and documentation.
- Keep `main` as the sole persistent repository branch by replacing automated
  dependency-update branches with maintainer-batched, short-lived updates;
  dependency alerts and security scanners remain active.
- Rewrite the repository README around the coordinated `v0.2.0-beta.1`, G1/G2,
  OpenSDK, and preserved P8 gates, with current release-trust, documentation,
  platform, and branch-policy guidance.
- Register the native filesystem watcher before exposing the development port,
  and keep the HMR acceptance project on Cargo's trusted target directory so
  startup readiness is deterministic on Windows and Linux.

### Security

- Reject conflicting `Content-Length` plus `Transfer-Encoding`, implicit
  request decompression, and multipart parsing before handlers until their
  independent bounded policies exist; add slowloris, stalled-reader,
  connection-exhaustion, header-exhaustion, and HTTP/2 overload cases.
- Resolve the `fast-uri` host-confusion advisory and override Cloudflare's
  vulnerable transitive `sharp` pin with `0.35.3`; all three npm audit surfaces
  now pass at high severity.
- Prevent the OpenSDK browser conformance server from exposing internal
  exception details in HTTP 500 responses.
- Add a machine-checked OWASP ASVS 5.0.0 G2 ownership map plus adversarial
  coverage for session fixation/rotation/revocation, CSRF, form and multipart
  bounds, SSRF policy, mass assignment, idempotent replay, cache partitioning,
  invalidation targets, and receipt redaction.
- Remove the vulnerable transitive `cookie -> time 0.3.44` chain reported by
  `RUSTSEC-2026-0009`; use a bounded session-cookie parser/serializer that
  preserves the Rust 1.86 MSRV and is covered by fail-closed runtime tests.

## Preview components 0.1.0-preview.1 - 2026-07-21

This [component prerelease](https://github.com/celiumsai/pliegors/releases/tag/preview-components-v0.1.0-preview.1)
does not replace the complete `v0.0.2` CLI release.

### Published

- Publish [`pliego-router`](https://crates.io/crates/pliego-router/0.1.0-preview.1),
  [`pliego-runtime`](https://crates.io/crates/pliego-runtime/0.1.0-preview.1),
  and [`pliego-sdk`](https://crates.io/crates/pliego-sdk/0.1.0-preview.1) as
  exact `0.1.0-preview.1` crates reconstructed from the tagged source.
- Promote G1 native routing, HTTP/1.1 and HTTP/2, complete/ordered/boundary SSR,
  complete and streamed layouts, bounded completion signals, and the scoped
  security corpus to public-preview capability status.
- Publish the OpenSDK build/browser/tooling foundation as a preview crate while
  retaining Draft RFC-006/RFC-007, Proposed ADR-006, and an unreleased server
  extension plane.

### Verification

- Protected CI, CodeQL for Rust/JavaScript/Actions, six fuzz targets, Chromium
  lifecycle tests, Rustdoc, OpenSDK conformance, and site validation passed on
  the tagged revision.
- crates.io independently resolved `pliego-router` while reconstructing and
  compiling the `pliego-runtime` package before publication.

## 0.0.2 - 2026-07-18

### Added

- Add default-disabled, identifier-free voluntary funnel telemetry with exact
  local preview/export, a 64-event bound, no network collector, and complete
  disable/delete controls.
- Add a signed release-only golden runner and exact matrix validator for Linux
  x64/ARM64, macOS x64/ARM64, Windows, Unicode, long paths, a pinned container,
  and required WSL2 promotion evidence.
- Add clean-revision P8 benchmark harnesses for cold and incremental builds,
  real browser DOM application, lifecycle memory plateau, raw observations,
  atomic same-environment resume, nearest-rank summaries, and schema-validated
  evidence merging.
- Add a separate P8 attestation package with a pinned CycloneDX SBOM,
  SLSA-compatible provenance, exact-set verification, and keyless Sigstore
  identity, while preserving the existing Ed25519 release bundle.
- Add six maintained libFuzzer targets with reviewed corpora and bounded CI for
  routes, manifests, event JSON, snapshots, DOM adoption, and adapters.
- Add `pliego doctor` with versioned human/JSON checks for the CLI, Rust
  toolchain, project manifest, lockfile, package alignment, and WASM tools.
- Add deterministic, local-only `pliego report --bundle` archives with an exact
  manifest, redacted diagnostics, dependency digests, and an omission ledger.
- Add read-only `pliego upgrade --check` compatibility reports for an explicit
  target version without editing manifests or lockfiles.
- Define the five-pillar product constitution, open/closed repository boundary,
  stability tiers, release channels, compatibility scope, and telemetry policy.
- Add the audited P8 trust and adoption contract for CLI diagnostics, release
  identity, adversarial validation, benchmarks, and clean-machine evidence.

### Changed

- Make release ZIPs and the signed framework source archive byte-reproducible,
  and require one release-manifest digest across candidate and draft evidence.
- Make canary, beta, and stable release channels explicit in the manual release
  workflow, enforce prerelease tag semantics, and generate version-neutral notes.
- Mark delegated PliegoCSS interoperability experimental and paused rather than
  part of the supported quickstart.

### Security

- Make both installers verify the Ed25519 manifest, pinned public-key
  fingerprint, selected archive, and signed sidecar before extraction.
- Require a complete nine-environment promotion matrix, including registry-based
  WSL2 evidence, before a stable GitHub Release draft can be created.

## 0.0.1 - 2026-07-16

### Added

- A Rust-native SSG, typed view system, event log, folds, reactive runtime,
  browser adapters, adaptive assets, typed content, Hyphae protocol boundary,
  diagnostics, CLI, and maintained starters.
- A bilingual documentation site generated by PliegoRS.
- Cross-platform releases for Linux, macOS, and Windows with SHA-256
  manifests and installer lifecycle checks.
- A Cloudflare Email Worker for the public project mailbox, with its production
  route and verified forwarding destination configured outside the repository.
- Typed, versioned application events with bounded canonical JSON, exact
  serialize/decode/value admission, exact local cursors, sealed schema catalogs,
  stable mapper/upcaster identities, and explicit adjacent upcasting.
- Transactional projections with reducer and codec identities, pre-commit state
  encoding, bounded fail-closed snapshot restore, exact-tail replay, and
  automatic reactive cleanup on drop.
- A bilingual security Trust Center with explicit trust boundaries, R0-R7
  evidence, release verification, scoped limitations, supported-version and
  advisory status, coordinated disclosure, and RFC 9116 `security.txt`.

### Changed

- Expanded the official bilingual site to 18 documentation topics covering the
  R0-R7 contracts, including causal development, schemas and snapshots,
  verified Hyphae sync, DOM ownership, artifact trust, and crate/API ownership.
- Published the accepted R0-R7 evidence and aligned the repository, website,
  crates.io packages, and signed GitHub Release around `0.0.1`.
- Added hierarchical documentation breadcrumbs, `TechArticle` and
  `SoftwareSourceCode` structured data, machine-local path rejection, and
  separate Cloudflare public and protected-preview delivery profiles.
- Made documentation filtering and scroll reveals progressively enhanced: a
  failed or absent client runtime cannot hide authored content.

### Security

- Explicit capability boundaries, safe HTML defaults, bounded content inputs,
  release provenance, dependency audits, and automatic browser cleanup.
- Hyphae protocol v2 makes append and page attestations mandatory, verifies
  every receipt under one logical authority, binds replay to a stream and fixed
  snapshot, and admits events only through a consuming typestate.
- The unauthenticated M5 one-event ACK seam is disabled by default behind the
  `experimental-legacy` feature and cannot enter verified replay.
- Projection snapshots bind the exact local content head, schema set, reducer,
  codec configuration, and canonical state bytes. Their SHA-256 digests provide
  integrity only; stream authority and signatures remain external trust
  contracts.
- Static preview delivery serves `security.txt` as UTF-8 plain text and keeps
  its disclosure metadata available under `/.well-known/security.txt`.

[Unreleased]: https://github.com/celiumsai/pliegors/compare/v0.2.0-beta.1...HEAD
[0.2.0-beta.1]: https://github.com/celiumsai/pliegors/compare/preview-components-v0.1.0-preview.1...v0.2.0-beta.1
[0.0.2]: https://github.com/celiumsai/pliegors/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/celiumsai/pliegors/releases/tag/v0.0.1
