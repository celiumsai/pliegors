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
id = "field-notes"
name = "Field Notes"
site_package = "field-notes-site"
output = "target/site"

[client]
package = "field-notes-client"
wasm_name = "field_notes_client"
bindgen_output = "target/field-notes-client/pkg"
```

The client table is optional. `pliego build` first resolves locked Cargo
metadata and captures the complete build context. When a client is present, it
then compiles the package for `wasm32-unknown-unknown`, runs `wasm-bindgen`, and
executes the site package, revalidating captured inputs around each step. The
site produces `pliego.build.json` through `pliego-ssg`.

Output must remain below `target/<name>`. The CLI and SSG reject traversal,
links, ancestors, current-directory output, and replacement of directories
without a valid ownership ledger.

`project.id` is the stable output ownership identity. It starts with a
lowercase ASCII letter, contains only lowercase ASCII letters, digits, or
hyphens, and is at most 64 characters. Changing it does not extend an ownership
history: it creates a different owner whose builds cannot overwrite the old
owner's output.

## Artifact contract

Routes, assets, and the reserved `pliego.build.json` name occupy one portable
namespace. NFC output spelling plus NFKC/full-case-fold collision keys reject
route aliases, case and Unicode aliases, ambiguous directory spelling, Windows
device names, and file/directory overlap before staging.

The strict v2 build report binds the exact payload file set to ownership,
framework source identity, supported toolchain versions, project configuration
and sources, the effective Cargo lock and workspace manifest, effective Cargo
and Rust toolchain files from the project and its ancestors, Cargo-home config,
and reachable external local path packages. External material roots and leaf
records remain out of the public receipt. The ephemeral local sidecar carries
only the roots and selection rules needed to recalculate them; the receipt
contains aggregate identity, counts, and bytes. The sidecar also binds the one
portable `outputPath` authorized by `pliego.toml`; `Site::build` rejects any
other requested destination before opening publication state.

Reproducible project commands reject `RUSTC`, compiler wrappers,
`RUSTC_BOOTSTRAP`, Rust flags, `CARGO_INCREMENTAL`, and Cargo build, profile, or
target overrides supplied through the environment (except
`CARGO_TARGET_DIR`). Intentional file-based settings belong in captured
`.cargo/config.toml`, `.cargo/config`, `rust-toolchain.toml`, or
`rust-toolchain` files at the project or ancestor levels, or in Cargo home's
`config.toml` or `config`. `CARGO_TARGET_DIR` remains an allowed selector;
artifact trust is deterministic evidence, not a hermetic build claim.

Production publication has one entry path: `pliego build` supplies the complete,
ephemeral `BuildInvocation` consumed by `Site::build`. A production call without
that invocation fails before publication; lower-level context injection remains
private and test-only.

The SSG preflights its 8 MiB ledger, 512 MiB per-file ceiling, 4 GiB aggregate
payload ceiling, and bounded namespace before creating a stage. It verifies the
private stage and rechecks inputs before publication. It
replaces an existing output only after that tree verifies from disk and has the
same `project.id`. A changed build records the actual validated prior
`projectId`, `sitePackage`, and `receiptSha256` in `previousOwnership`. An
identical rebuild returns the already-verified report without replacing the
tree, so its ledger bytes remain unchanged. `replacementPolicy` separately
records the stable overwrite authorization rule. `pliego inspect` and `pliego
preview` recalculate both disk outputs and the current build context; they do
not trust or repair stale evidence.

Receipt hashes provide deterministic integrity, not authenticity. The complete
threat model, migration notes, and remaining limits are in
[`docs/evidence/r1-artifact-trust.md`](docs/evidence/r1-artifact-trust.md).

Generated paths are compared with portable case/Unicode keys. The effective
Cargo target directory, built-in Rust targets, configured `build.target`,
output, bindgen directory, and private state must remain disjoint from
Cargo-owned layout before compilation begins.

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
