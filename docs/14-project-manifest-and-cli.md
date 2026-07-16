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
| `id` | yes | Stable portable ownership identity recorded in artifact receipts |
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

The parser denies unknown fields, invalid project identities, empty package
identifiers, absolute paths, parent-directory traversal, and invalid WASM
artifact identifiers. `project.id` must start with a lowercase ASCII letter,
contain only lowercase ASCII letters, digits, or hyphens, and be at most 64
characters. Keep it stable across machines and builds. Changing it creates a
different ownership identity, so the new project cannot overwrite output owned
by the old ID.

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

1. Validate the canonical project root, current `pliego.toml`, environment, and
   disjoint generated paths.
2. Materialize the effective lockfile, repeat Cargo metadata with `--locked`,
   and resolve the site/client graph and exactly one reachable `pliego-ssg`.
3. Capture project sources, effective configuration, supported toolchains,
   framework provenance, and reachable local materials into a build context;
   write its private roots and selections to the ephemeral invocation sidecar.
4. Compile the optional client package for `wasm32-unknown-unknown` in release
   mode, with the captured inputs revalidated immediately before and after.
5. Run `wasm-bindgen --target web --no-typescript` when a client exists, again
   bracketed by input revalidation.
6. Run the declared site package with the configured output path and the
   ephemeral build-invocation sidecar.
7. Require the site to emit a deterministic `pliego.build.json` v2 report, then
   recalculate both build inputs and the exact output set before accepting the
   build.

The first metadata pass may materialize an absent effective workspace
`Cargo.lock`; capture happens only after that pass. The build itself, subsequent
metadata checks, `inspect`, and `preview` use locked resolution. The public
receipt aggregates reachable external path-package evidence without publishing
absolute roots or individual leaf names. Private roots exist only in the
bounded sidecar under `target/.pliego`, which is removed after the site process
returns.

`check`, `build`, `dev`, `preview`, and `inspect` reject build-affecting
environment overrides: `RUSTC`, compiler wrappers, `RUSTC_BOOTSTRAP`, Rust
flags, `CARGO_INCREMENTAL`, and the `CARGO_BUILD_*`, `CARGO_PROFILE_*`, or
`CARGO_TARGET_*` families other than `CARGO_TARGET_DIR`. Effective
`.cargo/config.toml`, `.cargo/config`, `rust-toolchain.toml`, and
`rust-toolchain` files are captured from the project and its ancestors;
`config.toml` and `config` are also captured from the resolved `CARGO_HOME`.
Intentional file-based settings must live in one of those locations so the
artifact receipt binds their contents. `CARGO_TARGET_DIR` remains an allowed
selector rather than a receipt field; R1 does not claim a hermetic environment.
After every Cargo metadata pass, its resolved target directory is checked for
portable/canonical spelling and overlap with the configured output, bindgen
directory, and `target/.pliego`. The normal project `target/` layout also
reserves Cargo's standard directories, built-in Rust targets, and the effective
configured `build.target`; validation happens before compilation can write into
a publication root.

This is the only production publication path. `Site::build` requires the
complete `BuildInvocation` delivered through `PLIEGO_BUILD_CONTEXT`; it fails
before publication when that invocation is absent. The lower-level
context-bearing SSG entry point exists only for framework tests, so application
code cannot opt into a partial Cargo input graph. The invocation binds the
portable `project.output` value as `outputPath`; another relative or absolute
destination is rejected before the SSG opens its parent.

The client build intentionally follows stable Rust's
`wasm32-unknown-unknown` panic policy. A panic traps and terminates that WASM
instance; `pliego build` does not currently produce an exception-handling build
and does not promise post-panic recovery in the browser. Runtime recovery
guarantees apply to unwind-capable targets. The R0 WASM EH suite is a separate
Node.js verification path and is not part of the production client pipeline.

`pliego dev [port]` performs a build, serves the output on `127.0.0.1`, and
subscribes to the operating system's recursive filesystem event backend through
`notify`. Events are debounced for 120 ms without rescanning and hashing the
project tree. A failed rebuild leaves the last valid site available and the
watcher alive. Generated directories (`target`, `node_modules`, `.git`, and the
configured output) are excluded from watching.

Every successful build emits `pliego.graph.json`, a deterministic causal graph
covered by the artifact receipt. `Page::source(...)` and `Asset::source(...)`
create precise source edges; an undeclared legacy producer visibly depends on
`allSources` instead of receiving invented precision. Route outputs form
`source -> route -> artifact` edges, while standalone assets form direct
`source -> artifact` edges. `pliego why artifact <path|route>` verifies the
current receipt and explains that chain. `pliego why-rebuilt` reads the last
bounded private rebuild record and reports changed sources, invalidated routes,
affected artifacts, byte-changing artifacts, HMR mode, and receipt transition.

Development HTML receives an EventSource client that consumes typed generation
payloads. CSS changes replace matching stylesheet URLs; content changes fetch
and replace the current document body after a scope-disposal event; adapter
changes dispatch `pliego:adapter-hmr`, whose runtime v1.2 remounts through a
cache-busted module specifier. Mixed or unhandled changes reload the document.
The hook is never written to production output, and development responses use
`Cache-Control: no-store`.

Current SSG coordination entries are sibling
`.pliego-<output-sha256>-stage-<process>-<sequence>` and
`.pliego-<output-sha256>-backup-<process>-<sequence>` directories plus the
`.pliego-<output-sha256>.lock` file. The digest comes from the portable
collision key of the output leaf, keeping names below filesystem limits and
making case/Unicode aliases contend on the same lock. Because generated outputs
and their siblings live below `target`, the watcher ignores that root tree
wholesale rather than observing publication churn. Nested source directories
named `target`, `node_modules`, or `.git` remain observable. Same-length and
timestamp-coincident edits remain observable because the OS reports the write
and the verified build context rehashes source bytes before publication.
Use `--lan` for deliberate `0.0.0.0` exposure on a trusted network or
`--host <ip>` for one explicit interface. The reload endpoint has exact routing,
a 4,096-byte request-target ceiling, and a separate bounded 16-worker/32-request
pool, so long polls cannot consume the eight workers serving project files.

`pliego preview [port]` validates the existing v2 report, recalculates the exact
output tree and current build context, and serves the output without rebuilding
or injecting development behavior. It is also loopback-only unless `--lan` or
`--host` is explicit.

Both servers resolve clean routes to `index.html`, apply explicit MIME types,
and return the project's `404.html` with HTTP 404 for missing paths.

`pliego inspect` recalculates the configured output and current project evidence
from disk, then reports the verified receipt prefix and HTML route, payload file,
and byte totals. It never creates or repairs `Cargo.lock`.

`pliego why artifact <path|route>` requires a current verified build and fails
closed when the graph, output, source set, or receipt disagree. `pliego
why-rebuilt` becomes available after the first successful development rebuild.

## Artifact trust and ownership

Routes, assets, and the reserved ledger name share one portable namespace.
PliegoRS rejects aliases caused by route normalization, Unicode normalization,
case folding, ambiguous directory spelling, and file/directory overlap before
it creates the publication stage.

Receipt v2 binds project ownership, framework source identity, supported
toolchain versions, configuration, project sources, effective workspace Cargo
inputs, reachable external local packages, and every emitted payload byte. The
verifier rejects missing, extra, modified, symlinked, hard-linked, or
non-regular entries and also rejects undeclared empty directories. Receipt v2
limits each payload file to 512 MiB, the payload set to 4 GiB, and bounds both
namespace components and stored path-prefix bytes before hashing.

`replacementPolicy.requiredPreviousProjectId` is a reproducible rule for who
may replace the output next. For a changed replacement,
`previousOwnership` records the actual verified prior report's `projectId`,
`sitePackage`, and `receiptSha256`. Before publication, the SSG independently
verifies the existing tree, checks its `projectId`, and confirms that its
receipt did not change during the staged build. When inputs and payload bytes
match the verified prior artifact, the build is a no-op: it returns that report
without replacing the output, preserving identical ledger bytes.

These unkeyed hashes provide deterministic integrity and lineage, not a
cryptographic identity or authority signature. A writer who can replace both
payload and ledger can calculate a different internally consistent receipt.

Build evidence exposes project-relative paths and hashes. Secret-looking files
therefore fail capture instead of being silently omitted. Keep runtime secrets
outside build input roots; the filename guard is not a content-aware secret
scanner.

The complete schema, threat model, migration procedure, and residual risks are
recorded in
[`docs/evidence/r1-artifact-trust.md`](evidence/r1-artifact-trust.md).

### Migrating a v1 output

Add `project.id`, update `BuildReport` consumers from `report.files` to
`report.receipt.outputs.files`, and move the existing output directory aside.
The current SSG deliberately refuses to infer ownership from a legacy marker.
Run `pliego build` and `pliego inspect` to create and verify a v2 output; there
is no in-place or automatic migration.

`pliego version` prints the CLI package version without requiring a project.

All commands accept `--diagnostic-format human|json`. Stable identifier families
and exit codes are documented in
[`docs/20-official-starters-and-diagnostics.md`](20-official-starters-and-diagnostics.md).

## Intentional dependencies

The CLI orchestrates Cargo, rustc, `wasm-bindgen`, and project site binaries. It
does not reimplement those compilers. Adapter modules may use esbuild internally
through `pliego-adapters`; this does not introduce a JavaScript application
runtime.
