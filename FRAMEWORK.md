# PliegoRS framework contract

PliegoRS currently supports deterministic static sites and focused Rust/WASM
browser experiences. The framework owns routes, document heads, rendering,
typed content, asset policy, build evidence, reactive ownership, and adapter
lifecycles. It does not claim to replace Cargo, rustc, wasm-bindgen, JavaScript
libraries, codecs, or browser APIs.

## Project manifest

Every project owns a `pliego.toml`:

```toml
[project]
name = "Field Notes"
site_package = "field-notes-site"
output = "target/site"

[client]
package = "field-notes-client"
wasm_name = "field_notes_client"
bindgen_output = "target/field-notes-client/pkg"
```

The client table is optional. When present, `pliego build` compiles the package
for `wasm32-unknown-unknown`, runs `wasm-bindgen`, and then executes the site
package. The site produces `pliego.build.json` through `pliego-ssg`.

Output must remain below `target/<name>`. The CLI and SSG reject traversal,
links, ancestors, current-directory output, and replacement of directories
without a valid ownership ledger.

## Language boundary

Rust owns product routing, rendering, content schemas, state, folds, persistence
coordination, and application behavior. JavaScript is admitted at explicit
browser ecosystem boundaries:

- generated `wasm-bindgen` glue;
- the PliegoRS lifecycle runtime;
- adapter modules around native JavaScript libraries.

Adapter API v1 provides mount, update, unmount, abort signals, LIFO cleanup,
lazy triggers, capability policy, Save-Data, reduced motion, and dynamic
discovery. External modules execute with page privileges; this is a lifecycle
and resource contract, not a sandbox.

## Progressive modes

Projects should expose only the mode they need:

1. deterministic static output;
2. local event history and replay;
3. a durable outbox;
4. verified synchronization with Hyphae.

Static projects do not require Hyphae. Projects that enable sync must define
authority, cursor, receipt, event-version, conflict, and unknown-version policy.

## Official starters

`pliego templates` lists `minimal`, `editorial`, and `cinematic`. A generated
project includes explicit customization guidance and uses framework crates
either from a local checkout or one exact canonical Git revision.

## Current limits

Streaming SSR, server functions, authenticated Hyphae infrastructure,
selective build invalidation, deployment automation, and public package
distribution are not stable surfaces. The R0-R7 gates in
[`docs/28-hardening-roadmap.md`](docs/28-hardening-roadmap.md) take precedence
over expanding those areas.

## Release gate

A release candidate must pass the native migration gate, workspace tests,
adversarial lifecycle tests, artifact trust checks, full target matrix,
installer lifecycle, reproducibility checks, and final-manifest signature.
Checksums provide integrity; they do not independently establish authenticity.
