# Distribution and release

**Status:** R6 private candidate distribution is accepted. GitHub Releases is
the approved future canonical release channel, but the accepted R6 workflow did
not create or mutate a release. The repository remains private, releases remain
private or draft, and the `pliegors.dev` documentation site remains a protected
preview.

## Canonical ownership boundary

PliegoRS has one canonical GitHub origin:

- `https://github.com/celiumsai/pliegors` owns the source history, tags, release
  records, checksums, manifests, and downloadable CLI archives.
- GitHub Releases in that repository is the only official distribution channel.
  A version does not exist as a PliegoRS release unless it has a matching tag and
  release record there.
- No secondary download domain redirects, mirrors, caches, or originates
  releases.

PliegoRS crates keep `publish = false`; crates.io is not a distribution path.
There is no third-party mirror with authority to create or rename a release.

## Private preview gate

The approved `pliegors.dev` deployment is active as a private preview, not a
public launch. It sits behind Cloudflare Access with a deny-by-default policy.
Admission requires Mario's approved identity together with enrolled iOS WARP
and Gateway device posture; email identity alone is not the access boundary.
The Worker exposes only the `pliegors.dev` Custom Domain: its `workers.dev`
route and version preview URLs are disabled, and no `www` origin route exists.

During the preview:

- the GitHub repository remains private;
- candidate GitHub Releases remain private or draft;
- no release asset is promoted to public visibility; and
- no secondary download origin exists.

Opening the repository or publishing a non-draft release requires a separate,
explicit decision after quality, security, documentation, and installation
gates pass.

## Source dependencies

The CLI embeds official starters, but generated projects obtain framework crates
directly from the canonical PliegoRS Git repository. `pliego-dom`, `pliego-ssg`,
and every other framework crate receive the same full 40-character `rev` pin.
Release candidate builds inject `${GITHUB_SHA}` as `PLIEGORS_SOURCE_REV`, so a
CLI generates projects against the exact source revision that produced it.

Development remains explicit: `--framework-path <checkout>` or
`PLIEGO_FRAMEWORK_PATH` replaces the Git pin with local path dependencies.

## Support matrix

The release policy distinguishes production support from development
availability:

| Tier | Target | Status |
| --- | --- | --- |
| Production | `x86_64-unknown-linux-gnu` | Required and configured in the candidate matrix. |
| Production | `aarch64-unknown-linux-gnu` | Required and configured on a native Linux ARM64 runner. |
| Development | `x86_64-apple-darwin` | Configured contributor build and test surface; no production support commitment yet. |
| Development | `aarch64-apple-darwin` | Configured contributor build and test surface; no production support commitment yet. |
| Development | `x86_64-pc-windows-msvc` | Configured contributor build and test surface; no production support commitment yet. |

Development artifacts may be attached to a private or draft GitHub Release for
testing, but they must be labeled as development builds. Passing their smoke
tests does not expand the production support matrix.

## GitHub Release contract

Every candidate is bound to one immutable version and commit:

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

Asset names are stable across releases: `pliego-<TARGET>.zip`, its `.sha256`
sidecar, `SHA256SUMS`, `install.sh`, and `install.ps1`. The release tag and URL,
not a version embedded in the filename, select the version. The release title,
tag, embedded source revision, manifest entry, and checksum must agree.
Candidate assets stay in a private or draft release until the release gate is
explicitly approved. Rebuilding replacement bytes after validation is not
allowed; promotion must use the exact validated bytes or rerun the complete
matrix against the final assets.

`.github/workflows/release.yml` is manual-only. Candidate mode builds two native
replicas for each of five targets, runs the installer lifecycle on replica 1,
downloads the ten private artifacts, rejects binary disagreement, selects exact
archives, signs the final 15-asset manifest, and runs a distribution-only golden
path. Candidate mode is read-only and creates only expiring private Actions
artifacts. Draft mode is separate, restricted to `main`, requires its own exact
confirmation and the complete green path, and is the only job with
`contents: write`. It never publishes, edits, replaces, or marks a release as
latest.

The native reference products under `examples/` are validation fixtures, not
framework distribution assets. Before the repository can become public, their
source, copy, and media must be separated from the public framework package or
covered by an explicit redistribution license. This is a release gate, not an
assumption granted by repository visibility.

## Integrity and authenticity gate

SHA-256 sidecars detect corruption and accidental mismatch. They do not by
themselves establish authenticity. R6 adds a canonical Ed25519 manifest only
after all ten uploaded builder artifacts have been downloaded and compared. The
manifest binds the final installers, verifiers, reproducibility record,
sidecars, archives, roles, sizes, version, and source commit. Its accepted key
fingerprint is:

```text
sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250
```

Public promotion remains blocked until this fingerprint has an independent
trusted publication/bootstrap surface, the exact sealed bytes receive final
review, and platform signing or notarization is applied where required. The R6
candidate key is an online private-CI key, not an offline public-release root.
GitHub release identity and an independent signature remain complementary.

Bootstrap documentation downloads an installer or archive to disk, verifies it,
and executes it as a separate step. It never pipes a network response directly
into a shell.

## Install, upgrade, and rollback

No unauthenticated network installation is available while releases remain
private or draft. Authorized R6 testing downloads the sealed private Actions
artifact, verifies the complete signature and exact set, then uses an explicit
local archive to exercise install, second install, rollback, execution, and
uninstall.

Before public launch, `install.sh` and `install.ps1` must themselves ship as
GitHub Release assets. Installation never selects latest implicitly:

- `--version <semver>` / `-Version <semver>` resolves the matching
  `releases/download/v<VERSION>/...` assets;
- `--channel latest` / `-Channel latest` is the explicit opt-in to
  `releases/latest/download/...`; and
- omitting both selectors fails with an actionable usage message.

The installers validate semantic-version input, target selection, and checksums
before installing into `$PLIEGO_HOME/bin`, defaulting to `~/.pliego/bin`. They
do not yet verify the detached bundle signature internally; R6 verifies the
whole bundle before invoking them. Every future network download resolves
directly to the canonical GitHub Release.

There is no alternate release registry or authoritative download-base override.
Offline validation continues to use an explicit local archive.

The complete normative R6 behavior is in the
[candidate distribution contract](33-candidate-distribution-contract.md), with
the exact accepted hashes in
[R6 evidence](evidence/r6-candidate-distribution.md).
