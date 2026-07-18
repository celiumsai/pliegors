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

P8 makes the promotion channel explicit. Candidate mode is always `canary`.
Draft mode requires `beta` or `stable`: beta requires a prerelease SemVer tag
and produces a GitHub prerelease draft, while stable rejects prerelease tags.
The channel never changes the sealed bytes or an API's stability tier.

R6 acceptance executed candidate mode only. It did not execute draft mode,
create a tag, publish a release, deploy a site, or change repository visibility.

## Five targets, two builders

Every target is built twice in separate native GitHub-hosted jobs from one exact
commit and Rust 1.85.0:

| Target | Runner | Tier |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 x64 | production |
| `aarch64-unknown-linux-gnu` | Ubuntu 24.04 ARM64 | production |
| `x86_64-apple-darwin` | macOS 15 Intel | development |
| `aarch64-apple-darwin` | macOS 15 ARM64 | development |
| `x86_64-pc-windows-msvc` | Windows 2025 | development |

Release builds disable incremental compilation, strip symbols and debug data,
and remap the checkout path. MSVC additionally receives `/Brepro`; without it,
the PE timestamp and debug identity differ between builders and the candidate
must fail.

The accepted `0.0.1` R6 candidate used Ubuntu 24.04 x64. P8 lowers the future
Linux x64 build host to Ubuntu 22.04 so its glibc requirement is exercised by
the pinned Debian bookworm container instead of silently inheriting glibc 2.39
from the newest hosted image. ARM64 remains on the available Ubuntu 24.04 ARM
runner and therefore has its own recorded host boundary.

Each job uploads a ZIP, sidecar, and canonical build metadata. The seal job
downloads all ten uploaded replicas before it trusts or signs anything. For the
accepted `0.0.1` R6 evidence, binary hashes had to match while ZIP hashes could
differ because the archiver retained container timestamps; replica 1 was
selected deterministically. That paragraph is a historical claim about R6.

P8 supersedes the packaging rule for future candidates. A repository-owned ZIP
writer now sorts paths, fixes metadata, preserves only declared modes, and must
produce identical archive bytes across both native replicas. The seal rejects
either binary or archive hash disagreement.

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
historical R6 primary assets and write `RELEASE-MANIFEST.json`. The canonical
bytes bind the
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

For P8 and later, the primary exact set has 17 assets: the historical 15 plus
the deterministic `pliegors-source.tar.gz` and signed
`run-golden-path.mjs`. The source archive is built twice from the exact Git
revision with timestamp-neutral gzip metadata, rejects tracked symlinks, and
must compare byte for byte before assembly. The complete bundle additionally
contains the public key, manifest, and detached signature. The current
verifier enforces this expanded set; the committed R6 evidence remains an
immutable record validated by its historical checks.

## Installer and golden path

Replica 1 for every target executes the native installer lifecycle:

```text
install -> execute -> install again -> rollback -> execute -> uninstall
```

The accepted R6 distribution-only golden job checked out no framework source.
It downloaded only the sealed artifact, verified the signed complete bundle
against the fixed fingerprint, installed the Linux x64 archive, and confirmed
`pliego 0.0.1`. The installed CLI then scaffolded without
`--framework-path`; all first-party Cargo dependencies used the exact public
crates.io version, with no Git or path dependency. The job completed:

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

P8 replaces this single row with the
[release-only golden environment matrix](40-release-only-golden-matrix.md).
Eight hosted environments plus required physical WSL2 evidence exercise the
signed runner, deterministic release bytes, candidate-source bootstrap, and
registry-only draft promotion.

## Candidate versus public release

The network installers validate archive SHA-256 sidecars and independently
verify the selected archive against the canonical Ed25519 manifest and fixed
public-key fingerprint before extraction. They require Node.js for this
verification. Because an installer cannot establish its own authenticity, the
high-assurance bootstrap path still downloads and verifies the complete bundle
before executing that installer.

For `0.0.1`, `pliegors.dev/security/` is the independent fingerprint bootstrap
surface and the final draft is reviewed over the exact sealed bytes. Linux is
the production target. macOS notarization and Windows Authenticode signing are
not claimed for the development artifacts in this release. A compromise of the
release key requires immediate secret removal, key rotation, a new key ID, and
rebuilding every candidate; an old manifest must never be re-signed in place.

The exact accepted results are recorded in
[`evidence/r6-candidate-distribution.md`](evidence/r6-candidate-distribution.md).
