# Distribution and release

**Status:** `0.0.1` is the first PliegoRS public preview release. The repository,
crates.io packages, signed GitHub Release, and `pliegors.dev` documentation are
the public surfaces for the same version.

## Canonical ownership boundary

PliegoRS uses two official distribution systems:

- `https://github.com/celiumsai/pliegors` owns source history, tags, release
  records, checksums, manifests, installers, and prebuilt CLI archives.
- `https://crates.io` distributes the 15 `pliego-*` Rust packages. Every
  first-party dependency in one release uses an exact `=VERSION` requirement.
- `https://pliegors.dev` publishes documentation and the independently visible
  release-key fingerprint. It does not mirror binaries.

No secondary download domain, package registry, redirect, or cache has release
authority. A GitHub release exists only when its tag and non-draft release
record agree. A Rust package exists only when crates.io reports that exact
package version with this repository as its source.

Canary, beta, and stable promotion semantics, API stability tiers, MSRV, browser
scope, and pre-1.0 support are normative in the
[product constitution](34-product-constitution.md). A stable distribution
channel does not imply that a preview API has reached the `stable` API tier.

## Install from crates.io

The normal developer installation is:

```sh
cargo install pliego-cli --version 0.0.1 --locked
pliego version
pliego new my-site
cd my-site
pliego check
pliego dev
```

Generated projects use exact registry dependencies such as
`pliego-ssg = { version = "=0.0.1" }`. Every first-party crate in a project must
remain on one version. Local framework development is explicit:
`pliego new my-site --framework-path <checkout>` or `PLIEGO_FRAMEWORK_PATH`
replaces registry requirements with local path dependencies.

The crates.io publication order follows the dependency graph. Independent
crates publish first; `pliego-cli` publishes last. The guarded
`scripts/publish-crates.mjs` command checks package contents, the 10 MB registry
limit, exact internal requirements, repository state, registry convergence, and
the server-provided backoff deadline when crates.io rate-limits new packages.
Authentication comes from the ephemeral `CARGO_REGISTRY_TOKEN` environment
variable or Cargo's local credential store after an explicit `cargo login`. The
token is never passed on the command line or stored in the repository; a local
login used for a release is removed with `cargo logout` after publication.

## Support matrix

| Tier | Target | Status |
| --- | --- | --- |
| Production | `x86_64-unknown-linux-gnu` | Reproduced on two native runners and release-blocking. |
| Production | `aarch64-unknown-linux-gnu` | Reproduced on two native ARM64 runners and release-blocking. |
| Development | `x86_64-apple-darwin` | Built and smoke-tested; not a production deployment commitment. |
| Development | `aarch64-apple-darwin` | Built and smoke-tested; not a production deployment commitment. |
| Development | `x86_64-pc-windows-msvc` | Built and smoke-tested; not a production deployment commitment. |

macOS artifacts are not notarized and Windows artifacts are not Authenticode
signed in `0.0.1`. Those platforms are development surfaces; their archive
hashes and release-manifest entries remain verified. Linux is the production
server target for this release.

## GitHub Release contract

Every release is bound to one immutable version and source commit:

```text
tag: v<VERSION>
release: https://github.com/celiumsai/pliegors/releases/tag/v<VERSION>
asset: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/pliego-<TARGET>.zip
checksum: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/pliego-<TARGET>.zip.sha256
manifest: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/RELEASE-MANIFEST.json
signature: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/RELEASE-MANIFEST.json.sig
reproducibility: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/REPRODUCIBILITY.json
verifier: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/verify-release-bundle.mjs
shell installer: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/install.sh
PowerShell installer: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/install.ps1
```

Asset names are stable; the tag selects the version. A release title, tag,
manifest, source commit, archive metadata, and checksums must agree. Validated
bytes are never rebuilt in place. Any byte change requires the complete matrix
to run again.

`.github/workflows/release.yml` is manual-only. It builds two native replicas
for each target, runs installer lifecycle tests, compares binary hashes,
assembles the exact release set, signs the Ed25519 manifest, and proves a first
application using only public distribution surfaces. Candidate mode writes only
expiring Actions artifacts. Draft mode is restricted to `main`; it is the only
job with `contents: write`, and it can create but never publish or mutate a
release. Candidate mode is the `canary` channel. A draft selects `beta` or
`stable`; beta requires a prerelease tag and stable rejects one.

After sealing, a separate least-privilege job creates the CycloneDX SBOM,
SLSA-compatible provenance, exact attestation manifest, and keyless Sigstore
bundle. The distribution-only golden path verifies both the original Ed25519
bundle and the [supply-chain attestation package](37-supply-chain-attestations.md)
without a source checkout. A draft uploads both exact sets.

## Verify the complete bundle

SHA-256 sidecars detect corruption but do not independently establish who
published a file. The detached Ed25519 manifest binds installers, verifier,
reproducibility record, sidecars, archives, roles, sizes, version, and source
commit. The `0.0.1` accepted key fingerprint is published here and at
`https://pliegors.dev/security/`:

```text
sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250
```

With GitHub CLI and Node.js installed:

```sh
mkdir pliegors-v0.0.1
cd pliegors-v0.0.1
gh release download v0.0.1 --repo celiumsai/pliegors
node verify-release-bundle.mjs \
  --dir . \
  --expected-key-fingerprint sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250
```

The verifier rejects an unknown key, invalid signature, missing asset, extra
asset, role mismatch, size mismatch, hash mismatch, version mismatch, or source
commit mismatch.

## Install, upgrade, rollback

After full-bundle verification, install the matching local archive:

```sh
sh ./install.sh \
  --archive ./pliego-x86_64-unknown-linux-gnu.zip \
  --version 0.0.1
```

```powershell
.\install.ps1 `
  -ArchivePath .\pliego-x86_64-pc-windows-msvc.zip `
  -Version 0.0.1
```

Network selection is always explicit: `--version <semver>` / `-Version`, or
the deliberate mutable opt-in `--channel latest` / `-Channel latest`. Omitting
both fails. Installers validate the selected target and archive checksum before
writing to `$PLIEGO_HOME/bin`, defaulting to `~/.pliego/bin`. They retain one
rollback binary and support `--rollback` / `-Rollback` and `--uninstall` /
`-Uninstall`.

Installers do not yet verify the detached Ed25519 signature internally. The
full-bundle procedure above is the high-assurance path. Documentation never
pipes a network response directly into a shell.

The normative build behavior remains in the
[candidate distribution contract](33-candidate-distribution-contract.md), with
accepted pre-release evidence in
[R6 evidence](evidence/r6-candidate-distribution.md).
