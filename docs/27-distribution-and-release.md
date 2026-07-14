# Distribution and release

**Status:** GitHub Releases is the approved canonical distribution channel. The
repository remains private, releases remain private or draft, and nothing has
been deployed or published.

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

The first `pliegors.dev` deployment is not a public launch. When deployment is
explicitly approved, it must sit behind Cloudflare Access with deny-by-default
policy and admit only Mario's enrolled phone through a device-aware Zero Trust
rule. Email identity alone is not the access boundary, and no origin route may
bypass Access.

No deployment is authorized by this document. During the preview:

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
manifest: https://github.com/celiumsai/pliegors/releases/download/v<VERSION>/SHA256SUMS
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

`.github/workflows/release.yml` is manual-only and restricted to `main`. Given a
Cargo-matching tag and exact confirmation, it builds all five targets, runs the
installer lifecycle on each native runner, verifies every sidecar, retains the
candidate bytes privately, and assembles one GitHub Release as a draft. It never
publishes, edits, replaces, or marks a release as latest. Every candidate run is
therefore reviewable without changing the repository or release visibility.

The native reference products under `examples/` are validation fixtures, not
framework distribution assets. Before the repository can become public, their
source, copy, and media must be separated from the public framework package or
covered by an explicit redistribution license. This is a release gate, not an
assumption granted by repository visibility.

## Integrity and authenticity gate

SHA-256 sidecars detect corruption and accidental mismatch. They do not by
themselves establish authenticity. Private candidates must describe them only
as integrity checks.

Public promotion remains blocked until the GitHub Release contract adds an
offline-signed manifest, publishes the verification-key fingerprint through an
independent source surface, verifies the final uploaded assets, and applies
platform signing where the target supports it. GitHub release identity and an
independent signature are complementary controls.

Bootstrap documentation downloads an installer or archive to disk, verifies it,
and executes it as a separate step. It never pipes a network response directly
into a shell.

## Install, upgrade, and rollback

No unauthenticated network installation is available while releases remain
private or draft. Authorized candidate testing uses an explicit local archive
downloaded from its GitHub Release and then exercises install, second install,
rollback, execution, and uninstall.

Before public launch, `install.sh` and `install.ps1` must themselves ship as
GitHub Release assets. Installation never selects latest implicitly:

- `--version <semver>` / `-Version <semver>` resolves the matching
  `releases/download/v<VERSION>/...` assets;
- `--channel latest` / `-Channel latest` is the explicit opt-in to
  `releases/latest/download/...`; and
- omitting both selectors fails with an actionable usage message.

The installers validate semantic-version input, target selection, manifests,
and checksums before installing into `$PLIEGO_HOME/bin`, defaulting to
`~/.pliego/bin`. Every network download resolves directly to the canonical
GitHub Release.

There is no alternate release registry or authoritative download-base override.
Offline validation continues to use an explicit local archive.
