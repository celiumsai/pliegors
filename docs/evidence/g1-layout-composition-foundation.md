# G1 layout composition foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** In progress; route-owned complete-document composition

**Base revision:** `13b451e2b9f419774f151910fea355e6e03b6b4e`

**Toolchain:** Rust and Cargo 1.86.0 on Windows x86-64

## Contract under test

The unreleased router now preserves two related but distinct identities on a
matched route:

- `scope_ids()` contains pathless groups and layouts in root-to-leaf ownership
  order; and
- `layout_ids()` contains only the layout scopes that may own document
  composition.

`LayoutDocument` binds its required layers to that sealed layout sequence. Each
`LayoutLayer` transforms one private child frame through typed `before`,
`after`, and `wrap` operations. The child frame cannot be extracted, cloned,
omitted, or inserted twice by application code. Composition operates on typed
view data; it does not parse authored HTML or perform string substitution.

The implementation enforces before response commitment:

- every route-owned layout is supplied exactly once;
- groups and foreign layout IDs cannot claim the document;
- every admitted layer preserves exactly one child frame by construction;
- root-to-leaf head contributions have deterministic inner/page precedence;
- stylesheet and module-script order remains stable while exact duplicates are
  emitted once;
- the existing document metadata, DOM depth, node, and output-byte ceilings;
  and
- `renderMode: "layout"`, the full scope chain, and the layout-only chain in the
  exactly-once runtime receipt.

The layout capability lives entirely in the unreleased runtime crate. It does
not add a public variant to the released `pliego-dom::View` enum, preserving the
existing exhaustive-match API of the published 0.0.2 crate.

## Reproduction

```powershell
cargo test -p pliego-dom --lib --locked
cargo test -p pliego-router --lib --locked
cargo test -p pliego-runtime --all-targets --locked
cargo test -p native-pliego --locked
cargo clippy -p pliego-dom -p pliego-router -p pliego-runtime -p native-pliego --all-targets --locked -- -D warnings
cargo clippy -p pliego-dom --target wasm32-unknown-unknown --locked -- -D warnings
```

Observed targeted result before the full workspace regression:

- 27 `pliego-dom` unit tests passed;
- 23 `pliego-router` unit tests passed;
- 19 `pliego-runtime` unit tests passed;
- 27 native runtime integration tests passed;
- 2 raw socket tests passed;
- 6 native reference application tests passed;
- native and WASM Clippy passed with warnings denied.

Router tests prove a group plus layout chain yields two scope identities but
only one layout identity. Runtime tests compose two nested layouts, verify the
child appears exactly once in exact body order, page-over-layout metadata
precedence, stable asset deduplication, pre-commit failures, and the `layout`
render receipt. The full Axum reference path renders a matched page through its
sealed layout and records both ownership chains. The source diff leaves the
published DOM API unchanged, and its 27 existing unit tests remain green.

The Windows workspace run reached the pre-existing `dev_hmr` integration case
after all preceding packages passed, but Windows Application Control denied a
newly compiled `serde` build script inside that test's temporary project. The
same `dev_hmr` test passed unchanged under Debian/WSL2 with Rust 1.86.0, as did
the raw native socket corpus. This is recorded as a host-policy limitation, not
converted into a framework exception. The protected GitHub Linux workflow
remains the merge authority for the complete workspace and crates.io dry-run.

## Evidence boundary

This slice establishes layout-owned composition for complete responses only.
It does not add layout frames to ordered or asynchronous-boundary rendering,
layout loaders, layout cleanup resources, partial prerendering, HTTP/2,
OpenTelemetry, or fixed-load evidence. It does not close G1 or promote the
native runtime and dynamic SSR capabilities from `not-released`.
