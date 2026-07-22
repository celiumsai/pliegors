# Release-only golden environment matrix

**Status:** implemented; hosted candidate and WSL2 evidence pending  
**Target line:** `0.2.0-beta.1`

P8-A07 turns the first-use experience into a release gate. Every hosted row
starts from the signed distribution bundle, installs the native CLI, creates a
new application, exercises the complete developer path, and uninstalls the CLI.
It does not execute a binary from a framework checkout.

## Exact workflow

The signed `run-golden-path.mjs` asset executes this ordered contract:

```text
verify release -> extract signed source when required -> install -> version
-> prove telemetry disabled -> global doctor -> new -> check -> cargo test
-> dev HTTP smoke -> build
-> inspect -> why artifact -> report --bundle -> upgrade --check
-> project doctor -> prove telemetry still disabled -> uninstall
```

Every step records pass/fail and duration in a canonical
`dev.pliegors.p8-golden-path/v1` report. The report also binds:

- release version, 40-character source revision, target, and environment ID;
- SHA-256 of the exact signed release manifest, installed CLI, and reproduction
  bundle;
- host OS, architecture, CPU, memory, Node, Rust, and Cargo identities;
- dependency source and workspace scenario; and
- explicit Unicode and legacy Windows long-path observations.

The runner deletes its temporary workspace after writing the bounded report.
Failure still produces a raw report and fails the workflow.
On Windows, child-process working directories use the extended path namespace;
process and stdio failures are captured into the same bounded report instead of
escaping as unhandled Node events.
The long-path row also relocates Cargo's disposable compilation target to a
short, isolated directory. The PliegoRS CLI performs the same relocation when
`CARGO_TARGET_DIR` is not already explicit, while project sources, `target/site`,
the artifact ledger, and report evidence remain rooted in the original path.

## Required environments

Eight clean GitHub-hosted rows run in parallel:

| Environment ID | Host | Scenario |
| --- | --- | --- |
| `linux-x64` | Ubuntu 24.04 x64 | standard |
| `linux-arm64` | Ubuntu 24.04 ARM64 | standard |
| `macos-x64` | macOS 15 Intel | standard |
| `macos-arm64` | macOS 15 ARM64 | standard |
| `windows-x64` | Windows 2025 x64 | standard |
| `windows-unicode` | Windows 2025 x64 | non-ASCII path |
| `windows-long-path` | Windows 2025 x64 | path longer than 260 characters |
| `container-linux-x64` | pinned Rust 1.86 Debian container | standard |

The container image is digest-pinned to
`rust:1.86.0-slim-bookworm@sha256:57d415bbd61ce11e2d5f73de068103c7bd9f3188dc132c97cef4a8f62989e944`.
The Linux x64 release binary is built twice on Ubuntu 22.04 and then executed
inside that Debian bookworm container. This prevents a newer hosted glibc from
becoming an undeclared runtime requirement. Linux ARM64 is built on the
available Ubuntu 24.04 ARM runner and remains a separately recorded boundary.
WSL2 x64 is physical/local evidence because GitHub-hosted runners do not
represent that Windows subsystem boundary. It uses the same signed runner and
bundle, then enters promotion as a base64-encoded canonical report.

`check-golden-matrix.mjs` validates the exact report schemas and required host
tuples. It rejects a failed, missing, duplicated, stale, wrong-source, wrong-OS,
wrong-scenario, or hash-incomplete row. It also requires every row to name one
identical release-manifest SHA-256. The resulting
`dev.pliegors.p8-golden-matrix/v1` document and its raw reports receive a
separate keyless Sigstore identity.

## Candidate and draft sources

A canary can exist before its exact crates are published. Candidate rows
therefore scaffold against `pliegors-source.tar.gz`, an Ed25519-covered archive
created from the exact Git revision. The archive is produced twice with
`git archive` and `gzip -n`, compared byte for byte, rejects tracked symlinks,
and is extracted only after bounded path validation. This mode is named
`candidate-source` and is intentionally recorded as such.

A beta or stable draft must scaffold from crates.io with four exact
`=VERSION` first-party dependencies and no path or Git dependency. This mode is
named `registry`. It proves the public package graph rather than the embedded
candidate source.

The five platform ZIPs are created by the repository's deterministic ZIP
writer with sorted paths, fixed metadata, and explicit file modes. Both native
replicas must now match at the binary and archive byte levels. The source
archive and golden runner expand the Ed25519 manifest from R6's 15 historical
primary assets to 17 primary assets for future releases.

## Promotion sequence

The non-circular promotion order is:

1. Run a canary candidate for the intended version and source revision. The
   eight hosted rows use the signed candidate source archive.
2. Review the sealed bundle and publish all 19 exact-version crates from the
   same clean revision through the guarded publisher.
3. Run the signed golden runner on WSL2 using the sealed canary bundle and
   `--dependency-source registry`.
4. Dispatch beta or stable draft mode for the same revision and version, with
   the WSL2 report supplied through `wsl_report_base64`.
5. Rebuild deterministically. The eight hosted draft rows now use crates.io.
   The matrix validator requires their release-manifest hash to equal the WSL2
   report captured from the canary bytes.
6. Only after the complete nine-row matrix passes may the workflow create a
   reviewable GitHub Release draft.

Deterministic ZIP and source construction are what make the same-revision
canary and draft manifest comparable. A canary without WSL2 evidence may pass
as an explicitly incomplete matrix. Draft mode rejects an absent WSL2 report
and cannot produce a release draft from an incomplete matrix.

## Local WSL2 capture

After downloading the complete sealed canary bundle into a WSL-visible
directory and publishing the exact crates version, run:

```sh
node ./run-golden-path.mjs \
  --release . \
  --output ./wsl2-x64.json \
  --environment-id wsl2-x64 \
  --target x86_64-unknown-linux-gnu \
  --scenario standard \
  --dependency-source registry

base64 -w 0 ./wsl2-x64.json
```

The encoded output is a workflow input, not a repository file or credential.
The aggregator revalidates all report fields and exact hashes; it does not
trust the filename or the operator's description.

## Claim boundary

Local schema, tamper, and reproducibility tests prove the validator behavior.
They do not prove that the hosted platforms, Sigstore identity, crates.io, or a
real WSL2 environment executed successfully. P8-A07 remains open until one
same-revision hosted candidate and the required WSL2 promotion evidence are
retained in the final P8 evidence record.
