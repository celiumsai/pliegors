# Native static generation

**Status:** implemented and maintained

`pliego-ssg` turns a typed `pliego-dom::View` tree into complete, escaped HTML
documents and deterministic static routes. It owns head metadata, canonical
links, stylesheets, assets, route validation, output hashes, guarded publication,
and the `pliego.build.json` ledger.

The smallest executable reference is `examples/minimal-pliego`. Build the CLI
once, then use the verified project entry point from inside the example:

```sh
cargo build -p pliego-cli --locked
cd examples/minimal-pliego
../../target/debug/pliego build
../../target/debug/pliego inspect
```

Running the site package directly is intentionally unsupported in production:
it would omit the CLI-resolved Cargo input graph required by artifact receipt
v2.

The resulting pages require no framework runtime. Rust/WASM is added only when a
route declares resumable behavior.

## Closed contract

- One typed view tree feeds the HTML renderer.
- Static pages ship zero framework runtime.
- Text, attributes, title, and metadata escape by default.
- Routes and assets reject absolute paths, traversal, and unsafe destinations.
- Every emitted file has a stable SHA-256 and byte count.
- Repeated builds from identical inputs produce identical page and asset hashes.
- Staged publication preserves the last valid output when a build fails.

## Open boundary

Streaming SSR and Cloudflare request-time rendering remain separate roadmap
items. Static generation, typed content, resumable islands, and the project CLI
already share the same public contracts.
