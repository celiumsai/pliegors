# R1 Artifact Trust Evidence

**Status:** complete. The implementation and adversarial acceptance matrix are
recorded below against implementation commit
`12ec7cead21003c6dee8d4a85b873adda3cf2779`.

## Security objective and threat model

R1 makes a PliegoRS static build fail closed when its namespace, declared
inputs, emitted files, or existing output ownership no longer match. It is
designed to detect:

- cross-platform aliases that would publish two logical outputs to one path;
- missing, extra, modified, symlinked, hard-linked, or non-regular output
  entries;
- receipt tampering whose bound fields, self-hash, or aggregate counts no
  longer agree;
- project, configuration, source, reachable local dependency, Cargo lock, or
  supported toolchain drift;
- replacement of an unverified output or an output owned by another project;
- concurrent publication by cooperating PliegoRS builders.

The receipt is deterministic integrity evidence, not an authority signature.
An attacker who can rewrite both the output and `pliego.build.json` can compute
new unkeyed hashes. R1 also does not sandbox build scripts, attest the host, or
make a hostile mutable filesystem race-free. Release signing and distribution
authenticity remain separate gates.

## One portable output namespace

Routes, assets, and the reserved `pliego.build.json` name are registered in one
`OutputNamespace` before staging begins. Route aliases such as `/foo` and
`/foo/` therefore collide after both map to `foo/index.html`; route-to-asset,
file-to-directory, and ambiguous directory-spelling collisions use the same
check.

A stored path is normalized to Unicode NFC. Collision keys use Unicode NFKC,
full case folding, and a second NFKC pass per component. The validator rejects:

- empty, absolute, trailing-slash, backslash, `.` and `..` paths;
- empty components, controls, NUL, and Windows-invalid characters;
- trailing dots or spaces and Windows device names such as `CON`, `AUX`,
  `COM1`, and `LPT1`, including names with extensions;
- paths over 4,096 UTF-8 bytes before or after NFC, components over 255 bytes,
  and paths over 128 components.

The original NFC spelling is emitted. A second spelling with the same portable
collision key is rejected rather than silently rewritten. Namespace work is
bounded to 262,144 path components and 16 MiB of stored prefix/path bytes so a
small ledger cannot amplify into an unbounded in-memory prefix tree.

## Build report and receipt v2

`pliego.build.json` has a strict, unknown-field-denying schema:

```text
BuildReport 2.0.0
  reportVersion
  receiptSha256
  receipt
    receiptVersion: 2.0.0
    namespaceVersion: 1.0.0
    context: BuildContext
    replacementPolicy
    previousOwnership?: projectId, sitePackage, receiptSha256
    outputs
      files[]: path, kind, producer, bytes, sha256
      fileCount
      totalBytes
      sha256
```

Collections are sorted and validated before serialization. `outputs.sha256`
commits to the ordered output records, while `receiptSha256` commits to the
entire receipt. Totals use checked arithmetic. The ledger is capped at 8 MiB,
each published payload file at 512 MiB, the payload set at 4 GiB, and artifact
traversal at 100,000 entries. SSG size and ledger preflights run before a
publication parent or stage is created.

The exact-set verifier reads the ledger separately, enumerates the output tree,
and then requires equality with every declared payload file. It rejects missing
files, undeclared files, undeclared empty directories, modified byte lengths or
SHA-256 values, symlinks, hard links, and other non-regular entries. It checks
entry, namespace, component, path-byte, per-file, and aggregate bounds before
hashing declared files. Declared length is compared with metadata before the
read, so an oversized sparse file fails without being streamed. File hashing
opens the final component without following a symlink and detects size,
link-count, or modification-time changes during the read.

The ledger is reserved framework metadata and is not listed as one of its own
payload files. Its schema, self-hash, size, regular-file type, and single-link
ownership are verified independently.

## BuildContext

The receipt binds these build inputs:

| Field | Evidence |
| --- | --- |
| `ownership` | Stable `projectId` and selected `sitePackage`. |
| `framework` | Version and source evidence for the one resolved, reachable `pliego-ssg` package: local material aggregate, full Git commit, or registry checksum. |
| `toolchain` | Full `rustc -vV`, `cargo -Vv`, and, for client builds, `wasm-bindgen --version` output, each executed from the canonical project root. |
| `configuration` | Relative path, byte count, and SHA-256 for selected project configuration files. |
| `sources` | Portable project-relative path, byte count, and SHA-256 for the recursively selected project tree. |
| `materials` | Public aggregate records for workspace inputs and reachable external local packages. |
| `sourceSetSha256` | One digest over project source records and material aggregate records. |
| `excludedPaths` | Canonical roots intentionally excluded from project source capture, including the configured output. |

Project capture excludes the exact root `.git`, `target`, and `node_modules`
directories plus explicitly declared output roots. A different root spelling
with the same portable collision key, such as `.GIT`, `Target`, or
`Node_Modules`, is rejected rather than silently omitted; the same names remain
valid when nested below a source directory. Inputs must be regular,
singly-linked files. Link count is checked before and after hashing, preventing
an innocent-looking project path from aliasing an external sensitive file. The
context and every nested evidence set must be canonically ordered, portable,
and internally consistent.

The CLI resolves exactly one `pliego-ssg` package reachable from the selected
site package. Its package version and source tuple must match exactly one
`Cargo.lock` entry. `framework.sourceRevision` then contains:

- `sha256:<material aggregate>` for a local path package;
- the full 40-character commit from a resolved Git source; or
- `sha256:<Cargo.lock checksum>` for a registry or sparse-registry source.

The local value is derived from the captured
`cargo-path/pliego-ssg@<version>` material, so uncommitted framework bytes are
bound without treating the CLI executable's own build revision as the SSG's
provenance. The build-time Git revision embedded in `pliego` remains relevant
to generated scaffold dependency pins, not to the runtime framework evidence
above. Missing, multiple, unsupported, or lock-disagreeing resolved
`pliego-ssg` packages fail closed.

## Cargo lock, workspace, and path dependency closure

`pliego build` runs Cargo metadata once to materialize the effective lockfile,
then repeats metadata with `--locked` before evidence capture. This prevents a
first build from creating an unrecorded `Cargo.lock` after the snapshot.

Evidence is captured before any optional client compilation. The CLI then
revalidates topology, selected files, material bytes, and resolved framework
provenance before and after client compilation, bindgen, site compilation, and
final artifact verification.

The CLI records project configuration or aggregate materials for:

- the effective workspace root `Cargo.lock`;
- the effective workspace root `Cargo.toml`;
- project `pliego.toml`, `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`,
  `.cargo/config`, `rust-toolchain.toml`, and `rust-toolchain` when present;
- `.cargo/config.toml`, `.cargo/config`, `rust-toolchain.toml`, and
  `rust-toolchain` found at effective ancestors from the canonical project root
  upward;
- `config.toml` and `config` from the resolved Cargo home when present; and
- every local Cargo package outside the project root that is reachable through
  the resolved dependency graph from the site package or optional client
  package, including transitive path dependencies and the local
  `pliego-ssg` material used by framework provenance.

Project-local files stay in the public project-relative evidence set. Ancestor
and Cargo-home files are represented as minimized exact-file materials whose
public records do not expose their roots or leaf names. Ancestor material IDs
number only selected configuration levels, so unrelated checkout depth does
not change the ID sequence. `CARGO_HOME` must resolve to an absolute,
non-linked directory that does not overlap the project. Unreachable workspace
members are not inputs. Recursive material roots may not overlap the project
or one another, and duplicate material identities fail closed.

The public receipt deliberately does not serialize external absolute roots or
individual material leaf names. Each `InputMaterial` publishes only `id`,
`kind`, selection contract, file count, total bytes, and a SHA-256 aggregate.
That aggregate still commits to the private ordered path/size/hash records; it
is evidence minimization, not a confidentiality or anti-enumeration guarantee.

Registry and Git dependencies are bound by the effective Cargo lock. Their
downloaded source bytes are not copied into material aggregates in R1.

Before `check`, `build`, `dev`, `preview`, or `inspect`, the CLI validates the
canonical, non-linked project root and current `pliego.toml`, checks that
`project.output`, `client.bindgen_output`, and `target/.pliego` are pairwise
disjoint, and rejects linked generated-path ancestors. It also rejects
`RUSTC`, compiler-wrapper variables, `RUSTC_BOOTSTRAP`, Rust flags,
`CARGO_INCREMENTAL`, and the `CARGO_BUILD_*`, `CARGO_PROFILE_*`, or
`CARGO_TARGET_*` environment families other than `CARGO_TARGET_DIR`.
Intentional file-based Cargo and toolchain configuration must live in one of
the captured project, ancestor, or Cargo-home locations above. After every
metadata resolution, the effective Cargo target directory must be canonical
and disjoint from the output, bindgen directory, and private state. The normal
project `target/` layout additionally reserves Cargo-owned directories,
built-in Rust targets, and the effective configured `build.target` so Cargo
cannot compile into a Pliego publication root.

## Ephemeral invocation sidecar

The CLI passes the site process a bounded `BuildInvocation` JSON sidecar through
`PLIEGO_BUILD_CONTEXT`. It lives under
`target/.pliego/build-context-<pid>-<sequence>-<time>.json` and contains the
canonical project root plus the private material roots and exact-file
selections required to recalculate aggregate evidence.
It also carries the canonical portable `outputPath` from `pliego.toml`.
`Site::build` resolves the requested path against the canonical project root
and requires exact equality before opening a parent, lock, or stage; the
sidecar cannot authorize a second publication location.

The sidecar is not published and is deleted by the CLI's cleanup guard after
the site process returns. It may remain after an uncatchable process or machine
termination, so `target/.pliego` should be treated as local build metadata with
normal workspace access controls. The root `target` directory is excluded from
source capture, so a stale sidecar cannot make the next receipt
self-referential.

Production publication requires this complete invocation. Without it,
`Site::build` fails before staging or replacing output. The internal
context-bearing SSG build entry point is private and compiled only for tests;
there is no supported production path that captures a partial Cargo graph.

## Publication and replacement policy

The SSG opens or creates each publication-parent component through directory
capabilities without following links. It allocates the sibling staging
directory atomically, retains its open handle and filesystem identity, and
creates intermediate directories relative to that capability. Every output
leaf, including the ledger, is opened with `create_new` and no-follow
semantics, must be a regular file with link count one before and after the
write, and is synced before verification. Reopened stage directories and the
stage name must retain their recorded device/inode identity.

The sibling publication lock is also opened relative to the parent capability
without following links, must be a singly linked regular file, and then takes
an exclusive OS lock. This serializes cooperating PliegoRS publishers without
trusting a path-based check-then-open sequence.

The SSG validates the complete namespace and serializes a bounded provisional
receipt before touching the publication filesystem. After any prior ownership
is verified, it serializes the final lineage-bearing receipt before creating a
stage or writing payloads. It then verifies the staged tree and recalculates
the input context before publication. Existing output is replaceable only when:

1. it is a real directory rather than a link;
2. its receipt and exact output set verify from disk;
3. its `projectId` matches the incoming project; and
4. the prior receipt and opened output-directory identity are unchanged when
   checked again immediately before swap.

Private lock, stage, and backup names use a fixed SHA-256 token derived from the
portable output-leaf collision key, so a valid 255-byte leaf cannot overflow
the filesystem component limit and aliases share one lock. The existing output
is renamed to a capability-relative sibling backup before the stage is renamed
into place. Fault-injection tests cover both failure to reopen that backup and
failure of the stage rename; each proves byte-for-byte rollback, a still-valid
prior receipt, and no stage or backup residue. Separate adversarial tests cover
a linked stage directory, a pre-existing hard-linked leaf or ledger, a
symlinked lock target, a linked output ancestor, and concurrent lock contention
without modifying outside sentinels.

`replacementPolicy.requiredPreviousProjectId` is the overwrite authorization
rule for the next build and equals the receipt's current `projectId`. It is
separate from `previousOwnership`, which a changed replacement fills from the
actual verified prior report: `projectId`, `sitePackage`, and
`receiptSha256`. The first publication has no `previousOwnership`.

If the staged receipt has the same artifact core as the reverified prior
receipt, publication becomes a no-op: PliegoRS returns the existing report and
does not replace the output. This preserves byte-identical ledger bytes for an
identical rebuild while changed builds carry real prior-receipt lineage. The
lineage remains deterministic, unkeyed integrity evidence rather than
cryptographic authenticity.

## `inspect` and `preview`

Both commands verify rather than trust the ledger:

1. parse and validate report/receipt versions, ordering, aggregates, and hashes;
2. recalculate the exact output set from disk;
3. run locked Cargo metadata;
4. recapture the current project, effective workspace, reachable local
   materials, framework identity, and supported toolchain versions; and
5. require exact `BuildContext` equality.

`pliego inspect` then reports the receipt prefix, HTML route count, payload file
count, and payload bytes. `pliego preview` serves only after the same validation.
Neither command rebuilds the project, creates a missing lockfile, repairs a
stale lockfile, or mutates receipt evidence.

## Secret handling

Build evidence publishes project-relative paths and hashes, so capture rejects
secret-looking inputs instead of silently omitting them. Current fail-closed
patterns include non-template `.env` files, `.npmrc`, `.netrc`, `.pypirc`,
`.git-credentials`, `credentials`, `credentials.json`, `credentials.toml`,
`credentials.yaml`, `credentials.yml`, `id_rsa`, `id_ed25519`, `id_ecdsa`,
`id_dsa`, and `.key`, `.pem`, `.p12`, or `.pfx` suffixes. `.env.example`,
`.env.sample`, and `.env.template` are allowed as public templates.

This is a narrow prevention guard, not a secret scanner. Runtime secrets and
credentials must stay outside project and material input roots. A secret with
an unrecognized name could otherwise have its relative path and hash committed
to the receipt.

## Migration from ledger v1

1. Add a stable `project.id` to `pliego.toml`. It must start with a lowercase
   ASCII letter, contain only lowercase ASCII letters, digits, or hyphens, and
   be at most 64 characters.
2. Update code that read `report.files` to read
   `report.receipt.outputs.files` and use the v2 `BuildReport` structure.
3. Move the legacy output directory aside. PliegoRS intentionally refuses to
   treat a v1 marker as ownership evidence and will not overwrite it.
4. Run `pliego build`, then `pliego inspect` against the newly produced v2
   output.

Changing `project.id` changes the ownership identity. A build with the new ID
will refuse to overwrite output owned by the old ID; move that output aside
deliberately before rebuilding. There is no automatic ledger migration.

## Verification record

The Rust matrix ran from a fresh isolated target directory on Debian GNU/Linux
13.5 (trixie) under WSL2 kernel `6.18.33.1-microsoft-standard-WSL2`. The starter
matrix used separate CLI and project target directories and built every
official starter twice before comparing the complete ledger bytes.

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS in 1.986 s. |
| `cargo clippy -p pliego-artifact -p pliego-ssg -p pliego-cli --all-targets --locked -- -D warnings` | PASS in 11.272 s. |
| `cargo test -p pliego-artifact -p pliego-ssg -p pliego-cli --all-targets --locked` | PASS in 32.825 s: 113 top-level tests, zero failed or ignored; the artifact child self-test also passed. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS in 20.132 s. |
| `cargo test --workspace --all-targets --locked` | PASS in 36.788 s: 267 top-level tests, zero failed or ignored; the artifact child self-test also passed. |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | PASS in 14.364 s; 20 crate documentation indexes generated. |
| `cargo clippy -p pliegors-site-client -p spike --target wasm32-unknown-unknown --locked -- -D warnings` | PASS in 25.347 s. |
| `node scripts/check-starter-builds.mjs target/ci-starter-minimal target/ci-starter-editorial target/ci-starter-cinematic` | PASS after two byte-identical builds each: minimal 2 routes/5 files/7,732 bytes; editorial 2/14/1,268,273; cinematic 2/12/406,039. |
| `npm run check:docs` | PASS: 40 documentation files checked. |
| `npm run check:distribution` | PASS: 15 source-only crates at `0.0.1`, 5 private release candidates, manual draft-release policy. |
| `git diff --check` | PASS; only informational Git line-ending notices were emitted on Windows. |

- Base commit: `0bdbc96f39ac5623c8576f02a9bbf7dd0e982c27`
- Resulting implementation commit: `12ec7cead21003c6dee8d4a85b873adda3cf2779`
- Tool versions: `rustc 1.85.0 (4d91de4e4)`, `cargo 1.85.0
  (d73d2caf9)`, Node `v20.19.2` in WSL and `v24.16.0` on Windows.
- Targets checked: `x86_64-unknown-linux-gnu` and
  `wasm32-unknown-unknown`; focal Windows suites also passed with 25 artifact,
  28 SSG, 43 CLI unit, and 14 CLI integration tests.
- Adversarial review: green, with no remaining reproducible P1 or P2 finding.
  The limits of unsigned receipts, unbounded project-input hashing, hostile
  mutable filesystems, and crash consistency remain explicitly listed below.

## Known residual risks

- SHA-256 receipts are not signed attestations. A writer that controls output
  and ledger bytes can create a different internally valid receipt.
- Toolchain evidence records version output, not executable bytes, compiler
  distribution signatures, or target specifications. Effective project,
  ancestor, and Cargo-home config files are bound, but allowed selectors such
  as `PATH`, `CARGO_HOME`, `CARGO_TARGET_DIR`, and `RUSTUP_TOOLCHAIN`, plus Cargo
  environment configuration outside the explicitly rejected families, are not
  themselves receipt fields. Their resulting selected versions and captured
  file contents are evidence, but R1 is not a hermetic build sandbox.
- Cargo registry and Git dependency source bytes are not recursively aggregated;
  R1 relies on the effective lockfile for those identities.
- Output verification is byte-bounded, but project and local-material input
  hashing does not yet impose a per-file or aggregate byte ceiling. A hostile
  local checkout containing an enormous sparse input can therefore consume
  substantial read time before capture fails for another reason.
- Production SSG publication depends on the CLI-generated `BuildInvocation`.
  Direct `Site::build` calls without it fail closed; the partial-context helper
  remains private and test-only.
- The secret-name guard is finite. It reduces accidental disclosure but does
  not provide content-aware DLP.
- Capability-relative no-follow opens, `create_new`, link-count and identity
  checks, repeated input checks, exact output verification, and publication
  locks narrow filesystem races. They do not fully defeat a hostile writer
  that can mutate an already-open directory, add a hard link after the staged
  writer's checks, or bypass the cooperative lock.
- Output leaves are synced and rename failure is rolled back in-process, but
  parent-directory fsync and crash-consistent recovery across every filesystem
  are not specified. Power loss between backup and publication renames can
  therefore require manual recovery.
- Material aggregates hide roots and leaf records from the JSON schema, but
  low-entropy material contents may still be guessable from an unkeyed digest.
