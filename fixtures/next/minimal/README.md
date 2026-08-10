# Minimal Next fixture

This executable two-package workspace combines the supported Pliego build path
with a causal Rust/WASM browser UI.

It covers a counter, owner-bound local state, a keyed list, a conditional view,
an owned effect, and repeated mount/unmount behavior. Live state, genesis replay,
and snapshot-tail replay must agree after every accepted browser action.

## Commands

```sh
cargo test --manifest-path fixtures/next/minimal/Cargo.toml -p minimal-next-client --locked
pliego check
pliego build
pliego inspect
```

Run the Pliego commands from this fixture directory. Rust `1.86.0`, the
`wasm32-unknown-unknown` target, and `wasm-bindgen-cli 0.2.126` are required.

The browser workload exposes stable selectors for the counter, local draft,
list, conditional state, replay status, mount controls, and dedicated app host.

After building, run the Chromium lifecycle gate from the repository root:

```sh
npm run check:next-minimal-browser
```

The Phase 0 build collector runs this fixture with:

```sh
npm run measure:next-baseline -- --execute-minimal --output target/benchmarks/next-minimal
```

The report remains incomplete until the stress dashboard, reduced Hyphae
Console, browser, development, and lifecycle collectors are available.
