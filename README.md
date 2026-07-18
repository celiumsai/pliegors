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
  <a href="https://github.com/celiumsai/pliegors/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/celiumsai/pliegors/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/celiumsai/pliegors/actions/workflows/codeql.yml"><img alt="CodeQL" src="https://github.com/celiumsai/pliegors/actions/workflows/codeql.yml/badge.svg"></a>
  <a href="https://crates.io/crates/pliego-cli"><img alt="crates.io" src="https://img.shields.io/crates/v/pliego-cli.svg"></a>
  <a href="https://docs.rs/pliego-cli"><img alt="docs.rs" src="https://img.shields.io/docsrs/pliego-cli"></a>
  <a href="https://github.com/celiumsai/pliegors/releases"><img alt="GitHub release" src="https://img.shields.io/github/v/release/celiumsai/pliegors"></a>
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/crates/l/pliego-cli.svg"></a>
  <a href="https://doc.rust-lang.org/stable/releases.html"><img alt="rustc 1.85+" src="https://img.shields.io/badge/rustc-1.85%2B-b7410e.svg"></a>
  <img alt="Public preview" src="https://img.shields.io/badge/status-public_preview-d8ff2f.svg">
</p>

PliegoRS folds append-only event logs into interfaces. State is projected from
typed events, the projection advances as events arrive, and replay must produce
the same result. Useful HTML is emitted first; Rust/WASM resumes only the
behavior the document needs. Mature browser libraries such as GSAP, Lenis, and
Three.js remain JavaScript behind explicit lifecycle adapters.

The current public preview release is `0.0.1`. PliegoRS is pre-1.0 software: the
documented contracts are deliberate, while APIs may still evolve between minor
releases.

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
- a five-target, two-replica release pipeline with a signed exact-set
  manifest and a distribution-only golden path;
- an independently committed external flagship that exercises durable events,
  replay, forks, effects, receipts, provenance, audit, and selective sync;
- an official bilingual site authored by PliegoRS itself.

## Direction

PliegoRS is not a Vite, Astro, Next.js, or Leptos clone. Its differentiator is
the trust model across events, folds, effects, artifacts, and lifecycles.
Hyphae is the first-class durable data plane when a project needs it, but static
projects do not require Hyphae.

The R0-R7 hardening sequence is complete: reactive safety, artifact
trust, verified sync, snapshots and schemas, DOM lifecycle, developer golden
path, reproducible distribution, and an external flagship. See the
[hardening roadmap](docs/28-hardening-roadmap.md) and the bounded
[R7 evidence](docs/evidence/r7-external-flagship.md). Production Hyphae
operation remains a separate system boundary.

## Packages

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

## Install

Install the CLI from crates.io:

```sh
cargo install pliego-cli --version 0.0.1 --locked
pliego new my-site
cd my-site
pliego check
pliego dev
```

Diagnose an environment, create a redacted local reproduction archive, and
check exact package alignment without modifying the project:

```sh
pliego doctor
pliego report --bundle
pliego upgrade --check
pliego telemetry status
```

The delegated `pliego css check` surface is experimental interoperability with a
separately installed executable. PliegoCSS research is paused, is not published,
and is not part of the supported PliegoRS quickstart. PliegoRS accepts standard
CSS and remains independent of any CSS toolchain.

Linux production binaries and macOS/Windows development binaries are also
published in the [GitHub Release](https://github.com/celiumsai/pliegors/releases/tag/v0.0.1).
Installers require Node.js and verify their selected payload against the signed
release manifest before extraction. Download installers to disk and verify the
complete release bundle before running them; never pipe a network response
directly into a shell. See the
[distribution guide](docs/27-distribution-and-release.md).

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
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo clippy --target wasm32-unknown-unknown --locked -p pliegors-site-client -p spike -- -D warnings
npm ci
npm run check:benchmarks
npm run check:fuzz
npm run check:golden-path
npm run check:telemetry
npm run check:docs
npm run check:distribution
npm run check:phase-1
npm run test:phase-1
npm run check:site
```

## Core documents

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
- [Projection snapshot decision](docs/adr/ADR-005-projection-snapshots.md)
- [R3 acceptance evidence](docs/evidence/r3-snapshot-schema.md)
- [R4 acceptance evidence](docs/evidence/r4-dom-lifecycle.md)
- [R5 acceptance evidence](docs/evidence/r5-golden-developer-experience.md)
- [R6 acceptance evidence](docs/evidence/r6-candidate-distribution.md)
- [Framework API boundaries](docs/15-framework-api-boundaries.md)
- [Native migration gate](docs/16-native-migration-gate.md)
- [Framework readiness review](docs/17-framework-readiness-review.md)
- [Execution backlog](docs/19-product-execution-backlog.md)
- [Security, plugins, and adaptive media](docs/26-security-plugins-and-adaptive-media.md)
- [Distribution and release](docs/27-distribution-and-release.md)
- [Hardening roadmap](docs/28-hardening-roadmap.md)

## Project policies

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
