# Framework API boundaries

**Status:** reviewed for the first native theme

## Framework-owned

- View and HTML escaping semantics.
- Reactive ownership, equality gates, and disposal.
- Event-chain hashing, replay, snapshots, and incremental folds.
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
- DOM keyed reconciliation and complete arena reclamation still require
  hardening before application-scale 1.0.
- The current production proof is static generation plus Rust/WASM; streaming
  SSR and server functions are not implemented.
- Package publication and Cloudflare deployment commands are not implemented.

These limitations are release boundaries, not hidden fallbacks to Astro, Vite,
Next.js, or another application framework.
