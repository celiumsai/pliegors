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
`Page::new(...)` entry and declare each causal `.source(...)` edge.

## Production

```powershell
pliego check
pliego build
pliego inspect
pliego why artifact /
pliego why-rebuilt
pliego preview
```

The deployable site is written to `target/site`. `pliego.graph.json` explains
source to route to artifact causality and is covered by the build receipt.
Replace `https://example.com` in `src/main.rs` before launch so canonical and
social URLs are correct.

Documentation: https://pliegors.dev/docs/getting-started

## License

This starter is distributed under Apache-2.0; see `LICENSE`. Your original
application code and assets remain yours. Preserve notices for any third-party
dependencies or media you add.
