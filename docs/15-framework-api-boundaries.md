# Framework API boundaries

**Status:** public `0.4.0-beta.1` preview boundary; no stable 1.0 API

## Framework-owned

- View and HTML escaping semantics.
- Reactive ownership, equality gates, and disposal.
- Typed event-chain hashing, exact local cursors, sealed schema catalogs,
  transactional projections, canonical state codecs, and contract-bound
  projection snapshots.
- Full-document generation and head metadata.
- Route-to-file mapping, redirects, atomic output replacement, and build ledger.
- Resumable standard action descriptions.
- Adapter mount/disposal contract and immutable content-addressed bundles.
- WASM bootstrap generation.
- Project discovery, validation, compilation, serving, and inspection.

## Project-owned

- Route catalog and content.
- Components, styles, fonts, media, and visual identity.
- Application state models and domain-specific reducers.
- Which external libraries are installed and the adapter code that integrates
  them.
- Canonical production origin and deployment configuration.

## External-tool boundary

PliegoRS deliberately composes mature tools:

- Cargo/rustc compile Rust.
- `wasm-bindgen` produces the standards-facing WASM loader.
- FFmpeg and image codecs perform media encoding.
- esbuild bundles external ecosystem adapters such as GSAP and Lenis.
- Browsers provide DOM, CSS, WebAssembly, WebGL, and storage primitives.

No external tool owns routes, state, component rendering, or the project build
contract.

## Current limitations

- `pliego-hyphae` implements the protocol v2 client trust boundary: append and
  page attestations, receipt verification, event-version admission, and
  stream-bound replay typestate. The authenticated transport, production
  gateway/service, key distribution, durable outbox, and replay persistence are
  not implemented by that client crate.
- DOM ownership, retained keyed reconciliation, strict complete-seed SSR
  adoption, and adapter cancellation are complete under the R4 lifecycle
  contract. Boundary-local causal-state adoption is not implemented.
- G1 native HTTP and dynamic/streamed SSR, G2 data contracts, and G3 PBOC native,
  OCI, and Cloudflare hosts are released previews. Production durable providers,
  TLS/proxy policy, and broad provider parity remain outside those contracts.
- Hosted extension registry and discovery are not implemented. OpenSDK
  build/browser/tooling planes are released previews; its server plane remains
  unavailable.

These limitations are release boundaries, not hidden fallbacks to Astro, Vite,
Next.js, or another application framework.
