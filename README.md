<!-- SPDX-License-Identifier: Apache-2.0 -->

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/pliegors-symbol-reversed.svg">
    <img src="brand/pliegors-symbol.svg" alt="PliegoRS logo" width="112" height="112">
  </picture>
</p>

<h1 align="center">PliegoRS</h1>

<p align="center"><strong>A Rust-native web framework for verifiable, replayable, durable interfaces.</strong></p>

<p align="center">
  <a href="https://pliegors.dev/">Website</a> &middot;
  <a href="https://pliegors.dev/docs/">Documentation</a> &middot;
  <a href="https://pliegors.dev/changelog/">Changelog</a> &middot;
  <a href="https://pliegors.dev/security/">Security</a>
</p>

<p align="center">
  <a href="https://github.com/celiumsai/pliegors/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/celiumsai/pliegors/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/celiumsai/pliegors/actions/workflows/codeql.yml"><img alt="CodeQL" src="https://github.com/celiumsai/pliegors/actions/workflows/codeql.yml/badge.svg"></a>
  <a href="https://crates.io/crates/pliego-cli"><img alt="crates.io" src="https://img.shields.io/crates/v/pliego-cli.svg"></a>
  <a href="https://docs.rs/pliego-cli"><img alt="docs.rs" src="https://img.shields.io/docsrs/pliego-cli"></a>
  <a href="https://github.com/celiumsai/pliegors/releases"><img alt="GitHub release" src="https://img.shields.io/github/v/release/celiumsai/pliegors"></a>
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/crates/l/pliego-cli.svg"></a>
  <a href="https://doc.rust-lang.org/stable/releases.html"><img alt="rustc 1.86+" src="https://img.shields.io/badge/rustc-1.86%2B-b7410e.svg"></a>
  <img alt="Public beta" src="https://img.shields.io/badge/status-public_beta-d8ff2f.svg">
</p>

PliegoRS folds append-only event logs into interfaces. State is projected from
typed events, the projection advances as events arrive, and replay must produce
the same result. Useful HTML is emitted first; Rust/WASM resumes only the
behavior the document needs. Mature browser libraries such as GSAP, Lenis, and
Three.js remain JavaScript behind explicit lifecycle adapters.

The current public beta is [`v0.3.0-beta.1`](https://github.com/celiumsai/pliegors/releases/tag/v0.3.0-beta.1),
published on 2026-07-22. All twenty-one framework crates are coordinated on
crates.io at the exact `0.3.0-beta.1` version. PliegoRS remains pre-1.0
software: documented contracts are deliberate, while incompatible changes may
still arrive in a new prerelease or minor version with a changelog entry and
migration guidance.

This beta unifies the previous `v0.0.2` CLI, G1 native rendering, G2 data, and
G3 portable deployment in one exact package graph. RFC-006 and RFC-007 remain
Draft, and ADR-006 remains Proposed; OpenSDK and PBOC are public preview
contracts, not stable or accepted 1.0 APIs. The current MSRV is Rust `1.86`.

[`product.capabilities.json`](product.capabilities.json) is the canonical,
machine-readable inventory of what is released, available only from source,
partial, not released, or external to this repository. CI rejects drift between
that inventory, Cargo metadata, the support matrix, public documentation, and
the official site.

## Project status

| Surface | Status | Evidence |
| --- | --- | --- |
| PliegoRS `v0.3.0-beta.1` | Current signed public beta | [Release](https://github.com/celiumsai/pliegors/releases/tag/v0.3.0-beta.1) and [changelog](CHANGELOG.md) |
| R0-R7 framework hardening | Complete; preserved as regression gates | [Hardening roadmap](docs/28-hardening-roadmap.md) |
| P8 trust and adoption | Preserved as a release gate | [P8 contract](docs/35-p8-trust-and-adoption-contract.md) and [signed release](https://github.com/celiumsai/pliegors/releases/tag/v0.3.0-beta.1) |
| G1 native runtime and dynamic rendering | Public beta; G1 complete | [Runtime contract](docs/49-native-runtime-preview.md) and [transport/load/security evidence](docs/evidence/g1-transport-load-security.md) |
| G2 data, actions, sessions, and cache | Public beta; G2 complete | [G2 evidence](docs/evidence/g2-fullstack-beta.md), [umbrella RFC](docs/rfc/RFC-010-data-actions-cache.md), and [ASVS ownership map](security/asvs-v5.0.0-g2.json) |
| G3 PBOC and portable deployment | Public beta; G3 complete | [Portable deployment guide](docs/50-pboc-portable-deployment.md), [provider evidence](docs/evidence/g3-pboc-provider-conformance.md), and [ASVS ownership map](security/asvs-v5.0.0-g3.json) |
| OpenSDK `0.3.0-beta.1` | Public beta crate; governance pending | [OpenSDK foundation](docs/42-opensdk-foundation.md) and [execution backlog](docs/19-product-execution-backlog.md) |
| Hyphae integration | Optional verified protocol boundary | [Verified sync guide](docs/29-hyphae-verified-sync-guide.md); no production gateway claim |
| PliegoCSS `0.1.0-rc.2` | Optional experimental build-time companion | [Integration evidence](docs/evidence/pliegocss-optional-integration.md); never a runtime or starter requirement |

## What exists

- deterministic static generation with typed heads, routes, assets, and ledgers;
- escaped DOM/view construction and typed `view!` components;
- signals, memos, effects, ownership scopes, typed/versioned events with exact
  schema-value round trips, transactional projections, and contract-bound
  snapshots;
- typed Markdown, JSON, and TOML content with bounded discovery;
- Rust/WASM clients and a versioned mount/update/unmount adapter contract;
- lazy loading, capability policy, Save-Data, reduced motion, cancellation, and
  automatic cleanup for external browser libraries;
- reproducible image, video, font, and 3D asset plans with device budgets;
- a protocol v2 Hyphae client boundary with signed append/page attestations,
  stream-bound typestate replay, and no claim of a production gateway;
- `pliego new`, `check`, `build`, native-event `dev`, `preview`, `inspect`,
  `why artifact`, `why-rebuilt`, causal graphs, typed HMR, and maintained
  replayable default, minimal, editorial, and cinematic starters;
- default-disabled, identifier-free voluntary funnel telemetry with local
  preview, explicit export, a 64-event bound, and complete deletion;
- `pliego doctor`, deterministic redacted support bundles, and read-only
  compatibility checks through `pliego upgrade --check`;
- six maintained libFuzzer targets, reproducible build/browser/memory
  benchmarks, and a release-only nine-environment golden matrix;
- a five-target, two-replica release pipeline with reproducible archives, a
  signed exact-set manifest, CycloneDX SBOM, SLSA-compatible provenance,
  Sigstore identity, and a distribution-only golden path;
- an independently committed external flagship that exercises durable events,
  replay, forks, effects, receipts, provenance, audit, and selective sync;
- an official bilingual site, documentation system, security center, and
  evidence-linked changelog authored by PliegoRS itself;
- an experimental OpenSDK preview with typed Wasm Component admission,
  resource budgets, effect receipts, Rust/TypeScript/Python conformance,
  React/Svelte/Lit fixtures, and JSON-RPC/MCP tooling contracts;
- a public G1 native-runtime preview with a sealed dynamic router, bounded
  HTTP/1.1 and HTTP/2 transport, complete/ordered/async-boundary SSR,
  route-owned complete and streamed layouts, structured completion events,
  operator-enabled OpenTelemetry, real-socket adversarial cases, and fixed-load
  Linux RSS evidence.
- a public G2 beta with capability-scoped resources and loaders,
  progressive actions, truthful commit/cancellation state, idempotency, secure
  server-side sessions, bounded uploads, SSRF policy, explicit public/private
  runtime cache, causal invalidation, application contract manifests, and
  receipt-driven CLI diagnostics.
- a public G3 PBOC preview with exact bundle verification, pre-upload host
  admission, explicit rolling and rollback compatibility, a least-privilege
  Linux OCI target, and one same-build conformance corpus for native and
  Cloudflare Workers hosts.

## Direction

PliegoRS is not a Vite, Astro, Next.js, or Leptos clone. Its differentiator is
the trust model across events, folds, effects, artifacts, and lifecycles.
Hyphae is the first-class durable data plane when a project needs it, but static
projects do not require Hyphae.

R0-R7 and P8 are complete and remain regression gates: reactive safety,
artifact trust, verified sync, snapshots and schemas, DOM lifecycle, developer
golden path, reproducible distribution, external proof, diagnostics,
adversarial validation, benchmarks, clean environments, and voluntary-only
telemetry. See the [execution backlog](docs/19-product-execution-backlog.md),
[hardening roadmap](docs/28-hardening-roadmap.md), and bounded
[R7 evidence](docs/evidence/r7-external-flagship.md). Production Hyphae
operation remains a separate system boundary.

G1 is complete and available in the coordinated public beta. The
native router and runtime cover bounded connection and request lifecycles,
HTTP/1.1 and HTTP/2,
inherited middleware, authored errors, and complete, ordered, and
asynchronous-boundary SSR. Complete and streamed documents bind structural
layouts and head metadata to the sealed route ownership chain. Slow peers,
overload, ambiguous framing, unsupported encoded/multipart bodies, shutdown,
and fixed-load RSS are exercised on real Linux sockets. Operators can attach
their global OpenTelemetry providers and tracing subscriber; the runtime does
not select exporters, storage, retention, or inbound trace trust.
OpenSDK continues as the provider-neutral extension boundary required by that
runtime; public preview publication is not permission to call either API stable.

G2 is complete in `0.3.0-beta.1`. Its two-runtime
reference application proves progressive authenticated mutation, session
rotation and revocation, idempotent replay, typed failures, cache isolation,
read-your-writes invalidation, and redacted diagnostics. The included stores
and invalidation coordinator are development/conformance adapters; production
durability and cross-process delivery remain provider work. Its bundled stores
are deliberately conformance-oriented rather than production durability
providers.

G3 is complete at preview stability in `0.3.0-beta.1`. PBOC v1alpha1 seals the
route graph, runtime contract, artifacts, provenance, required host features,
cache policy, secret references, and compatibility chain. The same bundle is
verified before execution by the native/OCI and Cloudflare adapters. Portable
databases, queues, schedules, durable objects, provider billing, and automatic
state migrations remain outside this preview.

## Packages

All twenty-one workspace crates are public at the exact `0.3.0-beta.1` version.
Applications must keep every `pliego-*` dependency on that same version; mixed
framework graphs are outside the compatibility contract.

| Package | Responsibility |
| --- | --- |
| `pliego-log` | Typed/versioned local history, canonical payloads, exact cursors, and sealed schema catalogs |
| `pliego-fold` | Transactional projection, replay, canonical state codecs, and contract-bound snapshots |
| `pliego-reactive` | Signals, memos, effects, ownership, and disposal |
| `pliego-dom` | Escaped view and DOM construction |
| `pliego-macros` | Typed `view!` and component props |
| `pliego-content` | Typed content, safe CommonMark, limits, and diagnostics |
| `pliego-artifact` | Portable namespaces, build receipts, and exact-set verification |
| `pliego-ssg` | Documents, routes, assets, SEO, and staged builds |
| `pliego-resume` | Resumable standard browser actions |
| `pliego-adapters` | External ESM lifecycle and WASM bootstrap |
| `pliego-assets` | Adaptive media plans, budgets, and manifests |
| `pliego-inspect` | Artifact integrity and budget inspection |
| `pliego-hyphae` | Protocol v2 attestations, authority policy, and type-gated verified replay |
| `pliego-starters` | Maintained embedded starter projects |
| `pliego-cli` | Project creation, build, dev server, preview, and inspection |

| Beta package | Responsibility | Status |
| --- | --- | --- |
| [`pliego-router`](https://crates.io/crates/pliego-router/0.3.0-beta.1) | Sealed route graph, scopes, parameters, middleware capabilities, and error-boundary identity | Public `0.3.0-beta.1` |
| [`pliego-runtime`](https://crates.io/crates/pliego-runtime/0.3.0-beta.1) | Bounded HTTP/1.1 and HTTP/2 lifecycle, route-owned complete/streamed layouts, structured events, operator-enabled OTel, and three SSR modes | Public `0.3.0-beta.1` |
| [`pliego-data`](https://crates.io/crates/pliego-data/0.3.0-beta.1) | Provider-neutral resources, loaders, actions, sessions, idempotency, secrets, outbound HTTP policy, cache, and invalidation | Public `0.3.0-beta.1` |
| [`pliego-pboc`](https://crates.io/crates/pliego-pboc/0.3.0-beta.1) | Provider-neutral output manifest, artifact verification, host admission, routing, rolling compatibility, and rollback safety | Public `0.3.0-beta.1` |
| [`pliego-cloudflare`](https://crates.io/crates/pliego-cloudflare/0.3.0-beta.1) | Rust Cloudflare Workers host adapter for one admitted PBOC bundle | Public `0.3.0-beta.1` |
| [`pliego-sdk`](https://crates.io/crates/pliego-sdk/0.3.0-beta.1) | OpenSDK manifests, capability admission, typed Wasm Component runtime, effect receipts, compatibility, and tooling protocols | Public `0.3.0-beta.1` |

## Install

Install the CLI from crates.io:

```sh
cargo install pliego-cli --version 0.3.0-beta.1 --locked
pliego new my-site
cd my-site
pliego check
pliego dev
```

Pin all framework crates to the coordinated beta explicitly:

```toml
[dependencies]
pliego-router = "=0.3.0-beta.1"
pliego-runtime = "=0.3.0-beta.1"
pliego-data = "=0.3.0-beta.1"
pliego-pboc = "=0.3.0-beta.1"
pliego-sdk = "=0.3.0-beta.1"
```

Diagnose an environment, create a redacted local reproduction archive, and
check exact package alignment without modifying the project:

```sh
pliego doctor
pliego report --bundle
pliego upgrade --check
pliego telemetry status
```

The commands above are part of the published `0.3.0-beta.1` CLI. They run locally and
do not upload project data.

### Optional PliegoCSS companion

PliegoRS works with ordinary CSS and does not require PliegoCSS. To opt into
the separately released compiler for typed styles and static validation:

```sh
cargo install pliego-cssc --version =0.1.0-rc.2 --locked
pliego css check --seed
```

`pliego css check` only delegates from the canonical project root. PliegoCSS
compilation, watch mode, manifests, and route/island bundles remain explicit
`pliego-cssc` workflows and produce static CSS rather than a styling runtime.

## Evaluate OpenSDK

OpenSDK ships in the coordinated beta. Its complete repository conformance path
can be reproduced from an exact release checkout:

```sh
git clone https://github.com/celiumsai/pliegors.git
cd pliegors
cargo run -p pliego-cli --locked -- sdk compatibility
npm ci
npm run check:opensdk:all
```

The compatibility report is portable. The complete conformance path compiles a
Rust Wasm Component and runs real browser fixtures; it is a release-blocking
Linux CI gate and can be reproduced from Linux or WSL.

The Rust Wasm Component toolchain is the reference sandboxed implementation.
The TypeScript and Python process bridges are conformance implementations, not
sandboxed Component Model SDKs. Browser fixtures prove the explicit adapter
lifecycle for React, Svelte, and Lit without replacing those ecosystems.

## Evaluate the G2 beta

G2 is published as part of the exact coordinated package graph. Its complete
reference application remains in the release source:

```sh
git clone https://github.com/celiumsai/pliegors.git
cd pliegors
cargo test -p pliego-data
cargo test -p fullstack-pliego --test two_replicas
cargo run -p fullstack-pliego
```

The reference server binds loopback by default. Its in-memory resources are
conformance fixtures rather than production identity, storage, or cache
services. Read the [G2 evidence](docs/evidence/g2-fullstack-beta.md) before
adopting the source API.

## Evaluate portable deployment

Validate a sealed output, prove that a host supports every required feature,
and check a rolling transition before any upload:

```sh
pliego pboc validate dist/pliego.pboc.json --root dist
pliego pboc admit dist/pliego.pboc.json --root dist --host native
pliego pboc compatibility active/pliego.pboc.json \
  candidate/pliego.pboc.json --direction rolling
npm run check:provider-tck
```

The provider TCK builds the same PBOC for static musl/OCI and Cloudflare
Workers, exercises seven equivalent HTTP cases, rejects unsupported required
features, replays rollback, and scans the complete bundle for a secret
sentinel. See the [portable deployment guide](docs/50-pboc-portable-deployment.md).

## Release trust

The `v0.3.0-beta.1` release contains artifacts covering five platform targets, two
installer formats, checksums, a reproducible source archive, verification
tools, a CycloneDX SBOM, SLSA-compatible provenance, and the signed P8 golden
matrix. Linux x86_64 and ARM64 are production targets; macOS x86_64/ARM64 and
Windows x86_64 are development targets.

Installers verify the pinned Ed25519 release identity, manifest, selected
archive, checksum sidecar, and exact asset set before extraction. Separate
Sigstore bundles bind the supply-chain attestations and nine-environment golden
evidence. Review the [distribution guide](docs/27-distribution-and-release.md),
[supply-chain contract](docs/37-supply-chain-attestations.md), and
[security policy](SECURITY.md) before production use.

Installers require Node.js. Download them to disk and verify the complete
release bundle before running them; never pipe a network response directly into
a shell.

## Local development

```powershell
cargo build -p pliego-cli
target\debug\pliego.exe new ..\my-site --framework-path .
cd ..\my-site
..\pliegors\target\debug\pliego.exe check
..\pliegors\target\debug\pliego.exe dev 4400
```

The server binds `127.0.0.1` by default. Use `--lan` only for deliberate access
from a trusted local network.

## Quality gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo clippy --target wasm32-unknown-unknown --locked -p pliegors-site-client -p spike -- -D warnings
npm ci
npm run check:benchmarks
npm run check:fuzz
npm run check:golden-path
npm run check:telemetry
npm run check:opensdk:all
npm run check:docs
npm run check:distribution
npm run check:phase-1
npm run test:phase-1
npm run check:site
npm run check:site-deployment
npm run check:wasm-lifetimes
npm run check:pboc
npm run check:provider-tck
```

The complete OpenSDK Component and browser conformance gate runs on Linux in
CI. Windows contributors can reproduce that exact path through WSL while still
using the native Windows CLI for normal project development.

## Core documents

- [Online documentation](https://pliegors.dev/docs/)
- [Public changelog](https://pliegors.dev/changelog/)
- [Security and trust center](https://pliegors.dev/security/)
- [Current execution backlog](docs/19-product-execution-backlog.md)
- [Founding specification](docs/00-pliegors-spec.md)
- [PliegoRS and Hyphae target protocol](docs/01-hyphae-protocol.md)
- [Hyphae verified sync guide](docs/29-hyphae-verified-sync-guide.md)
- [Event schema and projection snapshot contract](docs/30-event-schema-and-snapshot-contract.md)
- [DOM lifecycle contract](docs/31-dom-lifecycle-contract.md)
- [Golden developer experience contract](docs/32-golden-developer-experience.md)
- [Candidate distribution contract](docs/33-candidate-distribution-contract.md)
- [Product constitution and stability policy](docs/34-product-constitution.md)
- [P8 trust and adoption contract](docs/35-p8-trust-and-adoption-contract.md)
- [Diagnostics, reproduction reports, and upgrade checks](docs/36-diagnostics-reports-and-upgrades.md)
- [Supply-chain attestations](docs/37-supply-chain-attestations.md)
- [Fuzzing and adversarial testing](docs/38-fuzzing-and-adversarial-testing.md)
- [Reproducible benchmarks](docs/39-reproducible-benchmarks.md)
- [Release-only golden environment matrix](docs/40-release-only-golden-matrix.md)
- [Voluntary telemetry and local funnel report](docs/41-voluntary-telemetry.md)
- [OpenSDK foundation and security model](docs/42-opensdk-foundation.md)
- [Multilanguage conformance](docs/43-opensdk-multilang-conformance.md)
- [Browser framework conformance](docs/44-browser-framework-conformance.md)
- [JSON-RPC and MCP tooling protocol](docs/45-opensdk-tooling-protocol.md)
- [Compatibility and deprecation policy](docs/46-opensdk-compatibility-and-deprecation.md)
- [Canonical product capability manifest](docs/47-product-capability-manifest.md)
- [Full-stack runtime threat model](docs/48-fullstack-threat-model.md)
- [G2 full-stack beta evidence](docs/evidence/g2-fullstack-beta.md)
- [Optional PliegoCSS companion evidence](docs/evidence/pliegocss-optional-integration.md)
- [OpenSDK planes and capability RFC](docs/rfc/RFC-006-opensdk-planes-and-capabilities.md)
- [Portable build output RFC](docs/rfc/RFC-007-pliego-build-output-contract.md)
- [Native HTTP runtime RFC](docs/rfc/RFC-008-native-runtime.md)
- [Full-stack route graph RFC](docs/rfc/RFC-009-route-graph.md)
- [Data, actions, sessions, and cache RFC](docs/rfc/RFC-010-data-actions-cache.md)
- [Wasmtime security-floor decision](docs/adr/ADR-006-opensdk-wasmtime-security-floor.md)
- [Projection snapshot decision](docs/adr/ADR-005-projection-snapshots.md)
- [R3 acceptance evidence](docs/evidence/r3-snapshot-schema.md)
- [R4 acceptance evidence](docs/evidence/r4-dom-lifecycle.md)
- [R5 acceptance evidence](docs/evidence/r5-golden-developer-experience.md)
- [R6 acceptance evidence](docs/evidence/r6-candidate-distribution.md)
- [Framework API boundaries](docs/15-framework-api-boundaries.md)
- [Native migration gate](docs/16-native-migration-gate.md)
- [Framework readiness review](docs/17-framework-readiness-review.md)
- [Security, plugins, and adaptive media](docs/26-security-plugins-and-adaptive-media.md)
- [Distribution and release](docs/27-distribution-and-release.md)
- [Hardening roadmap](docs/28-hardening-roadmap.md)

## Project policies

`main` is the repository's sole persistent branch. Contributions use
short-lived pull-request branches, pass the protected checks, merge linearly,
and delete the branch immediately after integration. Automated security alerts,
CodeQL, secret scanning, `cargo audit`, and `npm audit` remain active.

- [Changelog](CHANGELOG.md)
- [Governance](GOVERNANCE.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [Support](SUPPORT.md)
- [Community code of conduct](CODE_OF_CONDUCT.md)
- [Trademark policy](TRADEMARKS.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
- [Identity assets](brand/README.md)
- [Public mailbox Worker](workers/pliegors-email/README.md)

Apache-2.0. A Celiums Solutions LLC project. Contact
`hello@pliegors.dev`.
