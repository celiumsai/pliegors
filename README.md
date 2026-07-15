<!-- SPDX-License-Identifier: Apache-2.0 -->

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/pliegors-symbol-reversed.svg">
    <img src="brand/pliegors-symbol.svg" alt="PliegoRS logo" width="112" height="112">
  </picture>
</p>

<h1 align="center">PliegoRS</h1>

<p align="center"><strong>A Rust-native web framework for verifiable, replayable, durable interfaces.</strong></p>

PliegoRS folds append-only event logs into interfaces. State is projected from
typed events, the projection advances as events arrive, and replay must produce
the same result. Useful HTML is emitted first; Rust/WASM resumes only the
behavior the document needs. Mature browser libraries such as GSAP, Lenis, and
Three.js remain JavaScript behind explicit lifecycle adapters.

The repository is private pre-release software. No public version or install
channel is available yet.

## What exists

- deterministic static generation with typed heads, routes, assets, and ledgers;
- escaped DOM/view construction and typed `view!` components;
- signals, memos, effects, ownership scopes, event logs, folds, and snapshots;
- typed Markdown, JSON, and TOML content with bounded discovery;
- Rust/WASM clients and a versioned mount/update/unmount adapter contract;
- lazy loading, capability policy, Save-Data, reduced motion, cancellation, and
  automatic cleanup for external browser libraries;
- reproducible image, video, font, and 3D asset plans with device budgets;
- a protocol v2 Hyphae client boundary with signed append/page attestations,
  stream-bound typestate replay, and no claim of a production gateway;
- `pliego new`, `check`, `build`, `dev`, `preview`, `inspect`, and maintained
  default onboarding, minimal, editorial, and cinematic starters;
- an official bilingual site authored by PliegoRS itself.

## Direction

PliegoRS is not a Vite, Astro, Next.js, or Leptos clone. Its differentiator is
the trust model across events, folds, effects, artifacts, and lifecycles.
Hyphae is the first-class durable data plane when a project needs it, but static
projects do not require Hyphae.

The current release order is R0 reactive safety, R1 artifact trust, R2 verified
sync, R3 snapshots and schemas, R4 DOM lifecycle, R5 developer golden path, R6
candidate distribution, and R7 an external flagship. See the
[hardening roadmap](docs/28-hardening-roadmap.md).

## Packages

| Package | Responsibility |
| --- | --- |
| `pliego-log` | Verifiable local event log |
| `pliego-fold` | Projection, replay, snapshots, and cursors |
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
