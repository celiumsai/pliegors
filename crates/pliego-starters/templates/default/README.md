# __NAME__

The official replayable PliegoRS starter: typed actions and events, a
transactional projection, replay tests, three routes, local assets, a causal
build graph, and a deterministic build ledger.

## First run

```powershell
pliego check
cargo test --locked
pliego dev
```

Open `http://127.0.0.1:4400`. PliegoRS watches native filesystem events and
applies CSS, content, or adapter HMR after a successful causal rebuild. Build
failures are rendered as diagnostic pages while the watcher remains alive.

## Project map

- `src/main.rs`: routes, document metadata, and views written in Rust.
- `src/domain.rs`: action, versioned event, reducer, projection, and replay tests.
- `assets/site.css`: design tokens, layout, and responsive behavior.
- `assets/favicon.svg`: the PliegoRS starter identity. Replace it before launch.
- `assets/site.webmanifest`: install metadata and theme colors.
- `assets/robots.txt`: crawler policy.
- `pliego.toml`: project identity, Cargo package, and output directory.

## Make the first change

Add an `Action` and its typed event in `src/domain.rs`, extend the reducer, and
keep live state equal to replay in the included tests. For a new route, add a
`Page::lazy(...)` entry and declare every file read by its renderer with a
causal `.source(...)` edge.

## Production

```powershell
pliego check
pliego build
pliego inspect
pliego why artifact /
pliego why-rebuilt
pliego cache status
pliego preview
```

The deployable site is written to `target/site`. `pliego.graph.json` explains
source to route to artifact causality and is covered by the build receipt.
An unchanged build is a verified no-op; changed builds can reuse only verified
lazy-route artifacts whose declared sources remain unchanged.
Replace `https://example.com` in `src/main.rs` before launch so canonical and
social URLs are correct.

Documentation: https://pliegors.dev/docs/getting-started

## License

The copied starter code and PliegoRS engine are AGPL-3.0-only. New original
application code, content, brand, and assets that you author remain yours and
are not automatically licensed by PliegoRS. Choose an SPDX identifier in
`Cargo.toml` and replace `LICENSE.md` before distribution, while preserving AGPL
obligations for covered or combined work. Preserve third-party notices.
