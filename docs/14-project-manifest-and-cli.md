# Project manifest and CLI

**Status:** implemented generic contract

## Discovery

`pliego` searches the current directory and its ancestors for `pliego.toml`.
The containing directory becomes the project root. This allows commands to run
from any nested project directory without coupling the CLI to the PliegoRS
source workspace.

## Schema

### `[project]`

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Human-readable project identity used in command output |
| `site_package` | yes | Cargo package that owns routes, documents, and output |
| `output` | yes | Relative build directory |

### `[client]`

| Field | Required | Meaning |
| --- | --- | --- |
| `package` | yes | Rust `cdylib` package compiled for WASM |
| `wasm_name` | yes | Rust artifact and `wasm-bindgen` output stem |
| `bindgen_output` | yes | Relative directory read by the site package |

The entire table is optional for projects without browser-side Rust.

The parser denies unknown fields, empty package identifiers, absolute paths,
parent-directory traversal, and invalid WASM artifact identifiers.

## Commands

`pliego templates` lists official starter IDs, revisions, capability tags, and
intended use without requiring an existing project.

`pliego new <path> --template <id>` creates a standalone Rust project with `Cargo.toml`,
`pliego.toml`, a maintained project tree, a real 404 route, and the output ledger contract.
The project directory must be empty. During framework development,
`--framework-path <checkout>` writes explicit local path dependencies; the
`PLIEGO_FRAMEWORK_PATH` environment variable provides the same override. A
released CLI otherwise writes Git dependencies for the canonical PliegoRS
source repository, with every framework crate pinned to the exact 40-character
commit revision embedded when that CLI was built. Local checkout discovery is
intentionally not implicit.

`pliego check` parses the project manifest and Cargo metadata, verifies that the
site package exposes a binary, and validates an optional client's `cdylib`,
`wasm32-unknown-unknown` target, and `wasm-bindgen` installation. It produces no
site output.

`pliego build` executes this pipeline:

1. Parse and validate `pliego.toml`.
2. Compile the optional client package for `wasm32-unknown-unknown` in release
   mode.
3. Run `wasm-bindgen --target web --no-typescript` when a client exists.
4. Run the declared site package with the configured output path.
5. Require the site to emit the deterministic `pliego.build.json` ledger.

The client build intentionally follows stable Rust's
`wasm32-unknown-unknown` panic policy. A panic traps and terminates that WASM
instance; `pliego build` does not currently produce an exception-handling build
and does not promise post-panic recovery in the browser. Runtime recovery
guarantees apply to unwind-capable targets. The R0 WASM EH suite is a separate
Node.js verification path and is not part of the production client pipeline.

`pliego dev [port]` performs a build, serves the output on `127.0.0.1`, watches
project files, and rebuilds after a debounced change. A failed rebuild leaves
the last valid site available and the watcher alive. Development HTML receives
a small EventSource hook that reloads after the next successful build; this
hook is never written to production output. Generated directories (`target`,
`node_modules`, `.git`, and the configured output) are excluded from watching. Files in the
reserved atomic-publication namespace (`.*.pliego.lock` and the adjacent
`.*.pliego-<process>-<sequence>.tmp|bak` transaction files) are also excluded: only a changed final
artifact or manifest destination may trigger a site rebuild. Snapshots include a streaming SHA-256
fingerprint per source file, so edits
remain observable even when byte length and filesystem timestamps coincide.
Use `--lan` for deliberate `0.0.0.0` exposure on a trusted network or
`--host <ip>` for one explicit interface. The reload endpoint has exact routing,
a 4,096-byte request-target ceiling, and a separate bounded 16-worker/32-request
pool, so long polls cannot consume the eight workers serving project files.

`pliego preview [port]` validates the existing build ledger and serves the
output without rebuilding or injecting development behavior. It is also
loopback-only unless `--lan` or `--host` is explicit.

Both servers resolve clean routes to `index.html`, apply explicit MIME types,
and return the project's `404.html` with HTTP 404 for missing paths.

`pliego inspect` reads the configured output ledger and reports HTML route,
file, and byte totals.

`pliego version` prints the CLI package version without requiring a project.

All commands accept `--diagnostic-format human|json`. Stable identifier families
and exit codes are documented in
[`docs/20-official-starters-and-diagnostics.md`](20-official-starters-and-diagnostics.md).

## Intentional dependencies

The CLI orchestrates Cargo, rustc, `wasm-bindgen`, and project site binaries. It
does not reimplement those compilers. Adapter modules may use esbuild internally
through `pliego-adapters`; this does not introduce a JavaScript application
runtime.
