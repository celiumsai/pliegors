# Native PliegoRS migration gate

Use this gate before describing a site as fully migrated.

## Ownership

- [ ] The project has its own `pliego.toml`.
- [ ] `pliego-cli` contains no project or theme name, route, or asset path.
- [ ] Routes and complete documents originate from Rust project code.
- [ ] Build output is created by `pliego-ssg` and includes
  `pliego.build.json`.
- [ ] The build does not read the former framework project.

## Runtime

- [ ] Application behavior is Rust/WASM or a PliegoRS resumable action.
- [ ] JavaScript exists only as generated glue, framework lifecycle code, or a
  declared external-library adapter.
- [ ] Every adapter implements abort/disposal and has an immutable asset path.
- [ ] Final HTML contains no former framework islands, hydration markers,
  development clients, or overlays.

## Identity and metadata

- [ ] Favicon, web manifest, touch icon, generator metadata, canonical URL, and
  social metadata are explicit.
- [ ] No former framework's default identity assets remain.
- [ ] Every local `src` and `href` resolves.

## Behavior

- [ ] Canonical routes return 200 and unknown routes return the project's 404.
- [ ] Keyboard navigation, focus, reduced motion, responsive layouts, and
  interactive workflows are tested.
- [ ] Desktop and mobile layouts have no incoherent overlap or horizontal
  overflow.

## Engineering

- [ ] `cargo fmt --check` passes.
- [ ] Workspace tests and Clippy with warnings denied pass.
- [ ] The client crate passes Clippy for `wasm32-unknown-unknown`.
- [ ] Two consecutive builds produce the same ledger hash.
- [ ] The generic minimal project and the migrated project both build through
  the same release CLI.

Every maintained application must pass this complete gate before release.
