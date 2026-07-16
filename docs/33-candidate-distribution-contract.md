# Candidate distribution contract

**Status:** R6 accepted on 2026-07-16

R6 produces a signed, reproducible candidate before any GitHub Release is
created. Candidate construction and release promotion are different modes;
passing one never silently performs the other.

## Modes and authority

`.github/workflows/release.yml` is manual-only and accepts an exact Cargo tag,
mode, and matching confirmation:

```text
candidate + candidate:v<VERSION>
draft     + draft:v<VERSION>
```

Candidate mode may run on an authorized private branch. It has read-only
repository permissions and uploads only expiring private Actions artifacts.
Draft mode additionally requires `main`, a completely green candidate/golden
path, and exact `draft:v<VERSION>` confirmation. Only its final job receives
`contents: write`; it refuses to edit or replace an existing tag or release.

R6 acceptance executed candidate mode only. It did not execute draft mode,
create a tag, publish a release, deploy a site, or change repository visibility.

## Five targets, two builders

Every target is built twice in separate native GitHub-hosted jobs from one exact
commit and Rust 1.85.0:

| Target | Runner | Tier |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Ubuntu 24.04 x64 | production |
| `aarch64-unknown-linux-gnu` | Ubuntu 24.04 ARM64 | production |
| `x86_64-apple-darwin` | macOS 15 Intel | development |
| `aarch64-apple-darwin` | macOS 15 ARM64 | development |
| `x86_64-pc-windows-msvc` | Windows 2025 | development |

Release builds disable incremental compilation, strip symbols and debug data,
and remap the checkout path. MSVC additionally receives `/Brepro`; without it,
the PE timestamp and debug identity differ between builders and the candidate
must fail.

Each job uploads a ZIP, sidecar, and canonical build metadata. The seal job
downloads all ten uploaded replicas before it trusts or signs anything. The two
binary hashes for each target must be identical. ZIP hashes may differ because
the current archiver retains container timestamps; replica 1 is selected
deterministically and both archive hashes remain recorded. R6's reproducibility
claim is exact binary-byte equality, not deterministic ZIP bytes.

## Signed exact set

The assembler accepts exactly five targets times two replicas, rejects links,
unknown entries, malformed metadata, missing sidecars, changed bytes, target or
support drift, and binary disagreement. It emits one exact unsigned set:

- five selected CLI archives and five SHA-256 sidecars;
- Unix and PowerShell installers;
- `REPRODUCIBILITY.json`;
- the standalone Node.js verifier and its library; and
- the candidate public key.

Only after that exact set exists does `create-release-manifest.mjs` hash the 15
primary assets and write `RELEASE-MANIFEST.json`. The canonical bytes bind the
version, tag, source commit, source-date epoch, roles, byte sizes, SHA-256
digests, two-replica policy, Ed25519 key ID, and public-key fingerprint.

The manifest is signed with the GitHub secret
`PLIEGORS_CANDIDATE_SIGNING_KEY`. The private key is not present in source,
workflow logs, artifacts, or local evidence. The committed public key is:

```text
key ID:      pliegors-candidate-2026-01
algorithm:   Ed25519
fingerprint: sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250
```

`verify-release-bundle.mjs` requires the complete exact set, canonical schemas,
the trusted fingerprint and key ID, a valid detached signature, all asset sizes
and hashes, five ordered targets, two matching binary hashes per target, exact
sidecars, and selection of replica 1. Unknown files are failures, not ignored
extensions.

## Installer and golden path

Replica 1 for every target executes the native installer lifecycle:

```text
install -> execute -> install again -> rollback -> execute -> uninstall
```

The distribution-only golden job checks out no framework source. It downloads
only the sealed artifact, verifies the signed complete bundle against the fixed
fingerprint, installs the Linux x64 archive, and confirms `pliego 0.0.1`. The
installed CLI then scaffolds without `--framework-path`; all first-party Cargo
dependencies must use the exact public crates.io version, with no Git or path
dependency. The job completes:

```text
pliego check
cargo test --locked
pliego build
pliego inspect
pliego why artifact /
uninstall
```

No binary or framework checkout from the workflow workspace participates in
that application path.

## Candidate versus public release

The network installers validate archive SHA-256 sidecars; they do not
independently verify the Ed25519 bundle. The high-assurance path therefore
downloads and verifies the whole release before executing an installer.

For `0.0.1`, `pliegors.dev/security/` is the independent fingerprint bootstrap
surface and the final draft is reviewed over the exact sealed bytes. Linux is
the production target. macOS notarization and Windows Authenticode signing are
not claimed for the development artifacts in this release. A compromise of the
release key requires immediate secret removal, key rotation, a new key ID, and
rebuilding every candidate; an old manifest must never be re-signed in place.

The exact accepted results are recorded in
[`evidence/r6-candidate-distribution.md`](evidence/r6-candidate-distribution.md).
