# Stress dashboard Next fixture

This executable two-package Rust/WASM fixture uses the current Pliego build,
event, projection, reactive, keyed DOM, ownership, and SSG contracts.

It renders 1,536 keyed rows and 10,752 table cells, deterministic filters and
sorting, a 64-point chart, frequent typed tick events, nested ownership, replay
from genesis and snapshot-tail, and a 10,000-cycle representative lifecycle
plateau.

```sh
cargo test --manifest-path fixtures/next/stress-dashboard/Cargo.toml \
  -p stress-dashboard-next-client --locked
npm run check:next-stress-dashboard-browser
npm run measure:next-baseline -- --execute-stress-dashboard --output <new-directory>
```

The lifecycle result reports fixture-authored resources, detached-listener
behavior, DOM residue, and WebAssembly linear-memory plateau. It is not a census
of every private runtime arena node.
