# Security, plugin, and adaptive-media hardening

**Status:** complete for the pre-release static/Rust-WASM surface
**Reviewed:** 2026-07-12

This milestone closes the security review, plugin API v1, adaptive asset
contracts, and content resource limits required before public distribution.
It does not claim that arbitrary third-party JavaScript is sandboxed or that a
future server implementation is secure before its own review.

## Closed findings

| Severity | Finding | Resolution |
| --- | --- | --- |
| P0 | `project.output = "."` could reach `Site::build` and recursively remove project files before failing. | CLI-generated paths must be below `target/<name>`. SSG independently refuses the current directory, ancestors, links, non-directories, and outputs without a valid `pliego.build.json` ownership marker. |
| P1 | Preview followed linked files and created one unbounded thread per request. | Canonical root confinement, link rejection, a fixed worker pool, a bounded queue, request-target limits, and finite live-reload heartbeats. |
| P1 | Development and preview bound every network interface by default. | Loopback is now the default. LAN exposure requires `--lan` or an explicit `--host <ip>`. |
| P1 | Active head URLs accepted executable or ambiguous schemes. | Canonical, icon, manifest, alternate, stylesheet, module, and redirect URLs fail closed on dangerous schemes, traversal, controls, and protocol-relative ambiguity. |
| P1 | Adapter imports could finish after disposal; dynamic islands, update/unmount, and cleanup isolation were incomplete. | Versioned plugin API v1 with cancellation, dynamic discovery, stable lifecycle order, LIFO cleanup, policy admission, and runtime path validation. |
| P1 | Overlapping imports and updates could erase a newer controller, reorder state, or revive a disposed island; async cleanup could overlap remount. | Generation-bound pending records, serialized update queues, updates retained during import, terminal-state guards, and awaited teardown barriers. |
| P1 | First-party adapters selected tier after loader, a reference defaulted upward to Signature, and cleanup left video/global motion state behind. | A policy bootstrap runs before loader; adapters consume the admitted v1 context, preserve reduced motion, fall back without IntersectionObserver, and restore videos, classes, GSAP, Lenis, and listeners. |
| P1 | A forged asset could occupy `pliego.build.json`; failed staging could leak directories or remove the last valid output before replacement succeeded. | The ledger name is reserved case-insensitively and SSG uses guarded staging plus backup/rollback replacement. |
| P1 | An existing output ledger could be read without a byte ceiling before ownership validation. | The SSG opens the final ledger without following links, validates the opened handle, and reads at most 8 MiB plus one detection byte before refusing replacement. |
| P1 | Adapter cleanup removed authored inline styles outside adapter ownership. | Cleanup no longer performs broad style deletion; it is limited to state, listeners, and animation resources owned by the adapter. |
| P1 | Baseline references and a content metadata/open race could read outside their declared roots. | Baseline paths are normalized and canonically confined; content opens through a root capability and no-follows the final component. |
| P1 | Adaptive/raster work and preview reload had aggregate resource-exhaustion paths. | Aggregate staged-media and legacy raster ceilings plus a separate bounded reload pool prevent unbounded I/O/memory and static-worker starvation. |
| P1 | The earlier Hyphae client accepted weak cursor/sequence relationships and exposed an unverified legacy ACK path. | Superseded by the R2 protocol v2 client: genesis and cursors are explicit, server sequences are absolute and contiguous, append/page attestations plus every receipt require one authority, and the M5 ACK API is isolated behind `experimental-legacy`. This does not claim a production gateway or service. |
| P2 | Content discovery and JSON/manifest inputs had no complete resource ceiling. | Configurable content depth/count/file/total-byte limits, bounded manifest reads, and adaptive recipe/job/source/artifact limits. |
| P2 | Asset inspection and preview could cross a declared root through filesystem links. | Every relevant walker rejects links; canonical verification keeps reads and publication inside the declared root. |
| P2 | Adapter policy and trigger attributes could be changed to unknown values, and a late async update could revive disposed state. | Unknown policy values fail closed; update completion rechecks the active lifecycle token. |
| P2 | Unicode props could pass the browser's JavaScript-length check while exceeding the Rust byte contract. | Runtime admission and updates now count serialized UTF-8 bytes exactly and reject payloads over 32,768 bytes before import or mutation. |
| P2 | Inspect and adaptive-asset inputs could change between path metadata validation and the actual read. | Bounded inputs are opened without following links and size, type, streaming read, digest, validation, and publication operate from the same handle. |
| P2 | Mixed-case `</script>` bypassed the inline-script closing-tag neutralizer. | The trusted inline bootstrap renderer now matches HTML end tags case-insensitively and has a mixed-case regression. |

The destructive output regression, link confinement, oversized inputs,
dangerous document URLs, adapter races, policy tampering, cleanup failures, and
adaptive staging forgery all have automated tests.

## Plugin API v1

[`pliego-adapters`](../crates/pliego-adapters/src/lib.rs) now defines the stable
browser extension boundary:

- `mount`, `update`, `unmount`, legacy remount compatibility, and automatic
  cleanup;
- `immediate`, `visible`, `idle`, and `interaction` loading;
- Universal, Lite, Balanced, and Signature admission tiers;
- DOM, motion, smooth-scroll, audio, video, WebGL, high-frequency RAF, WebGPU
  capability declarations;
- reduced-motion and Save-Data policies evaluated before import;
- same-origin local module validation, bounded props, error isolation, and
  lifecycle events;
- recipes for GSAP, Lenis, Three.js/WebGL, and other native ESM libraries.

The complete public contract is [External adapter contract](12-external-adapters.md).

## Adaptive media

[`pliego-assets`](08-pliego-assets.md) owns versioned recipes, plans, manifests,
budgets, content addresses, and runtime delivery directives for:

- AVIF, WebP, and JPEG responsive images;
- H.264/AV1 MP4 and VP9 WebM video;
- WOFF2 font subsets;
- GLB LODs with Meshopt or Draco geometry and KTX2 textures.

External codec and 3D tools remain pinned executors. They cannot inject shell
commands into recipes. Rust validates their staged output, observed codec or
geometry evidence, tier budgets, and final SHA-256 before publishing anything.
The manifest delivery policy maps directly onto plugin API tiers, Save-Data,
reduced motion, lazy loading, and interaction-only activation.

## Dependency review

The frozen lockfiles were checked on 2026-07-14:

- root and Email Worker `npm audit`: zero known vulnerabilities;
- RustSec database: zero vulnerable packages;
- informational warning: `proc-macro-error2 2.0.1` is unmaintained through
  `rstml -> syn_derive`. No patched `rstml` release or vulnerability advisory
  exists in the resolved graph. Track and replace it before the first public
  GitHub Release if an upstream release removes the dependency.

CI now runs both Node and Rust dependency audits. An audit result is evidence
for the checked lockfile, not a permanent guarantee.

## Release trust boundary

GitHub Releases in `celiumsai/pliegors` is the only official distribution
authority and download origin. The repository and candidate releases remain
private or draft, and this review authorizes no public release or promotion.
The separately approved `pliegors.dev` deployment is a private documentation
preview behind Cloudflare Access; it is not a framework distribution origin.
No secondary download origin exists in the distribution architecture.

Production support is limited to Linux x64 and Linux arm64. macOS x64, macOS
arm64, and Windows x64 remain development surfaces even when their candidate
archives pass smoke tests. The manual workflow now configures all five native
runners and can assemble only a private draft; it has not yet produced release
evidence, and it cannot publish a release.

Before publication, the exact GitHub Release assets must pass checksum and
installer lifecycle verification, an offline-signed manifest must bind every
asset to its tag and source commit, and promotion must require an explicit human
decision. Checksums from the same origin provide integrity, not independent
authenticity.

## Reproduce

```powershell
npm audit --audit-level=high
npm run test:adapters
node --test scripts/adaptive-asset-schemas.test.mjs
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo install cargo-audit --version 0.22.1 --locked
cargo audit
```

## Release boundary after hardening

The completed private hardening order is the R0-R7 sequence in the
[hardening roadmap](28-hardening-roadmap.md): reactive safety, artifact trust,
verified sync, snapshot identity, DOM lifecycle, golden developer experience,
candidate distribution, and an external auditable flagship.

Server functions, production Hyphae credentials, and product-specific backend
work belong inside those deployment/distribution fronts and retain their own
threat models.
