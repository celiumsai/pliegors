# R6 candidate distribution acceptance evidence

**Gate:** R6 - Candidate distribution

**Status:** Accepted

**Recorded:** 2026-07-16

**Implementation revision:** `db2ddb3b83d99f6d9ebb63218f1579a860935588`

**Private Actions run:** `29504976368`, attempt 1, successful

## Acceptance matrix

| ID | Requirement | Evidence | Result |
| --- | --- | --- | --- |
| R6-A01 | Produce exactly five target archives | Native matrix built all five target triples | PASS |
| R6-A02 | Prove reproducibility with independent hashes | Two separate native jobs per target produced identical binary SHA-256 | PASS |
| R6-A03 | Sign only the final exact asset set | Seal job downloaded ten artifacts, assembled 15 assets, then signed canonical manifest | PASS |
| R6-A04 | Verify manifest authenticity and all asset bytes | Ed25519 verifier passed in seal, golden path, and independent local replay | PASS |
| R6-A05 | Verify install, upgrade, rollback, and uninstall | Replica 1 passed the native lifecycle on all five targets | PASS |
| R6-A06 | Start the golden path from distribution | No checkout; sealed bundle to installed CLI to replayable app | PASS |
| R6-A07 | Preserve private/no-publish boundary | Candidate artifact private; draft job skipped; no release or deploy performed | PASS |
| R6-A08 | Fail closed under real defects | MSVC nondeterminism and leaked builder env each stopped prior attempts | PASS |

## Accepted run

The accepted workflow ran from `2026-07-16T14:04:46Z` through
`2026-07-16T14:08:23Z` on the exact implementation revision. All ten build jobs,
the seal job, and the distribution-only golden path passed. The draft job was
skipped by its mode and `main` guards.

The sealed private artifact was:

```text
name:   pliegors-sealed-v0.0.1-db2ddb3b83d99f6d9ebb63218f1579a860935588
bytes:  11955033
digest: sha256:b6836557fa83ec87e04cedd03b9a1ccc4d82cd690cb94738f76437ae2478a078
```

Actions artifacts expire; the exact signed records are committed here:

- [`r6/RELEASE-MANIFEST.json`](r6/RELEASE-MANIFEST.json), SHA-256
  `41062a8774abd528459bae21dd8e4150c475d99cc56bbde611b64ec0573abcff`;
- [`r6/RELEASE-MANIFEST.json.sig`](r6/RELEASE-MANIFEST.json.sig), SHA-256
  `71b4a9a041b35d73f200eb0a9a4eeb6838a974cd4f20e99db8aed2287f76757f`;
- [`r6/REPRODUCIBILITY.json`](r6/REPRODUCIBILITY.json), SHA-256
  `70284457d2b9ef775de8d300f19951aa3b94af11ccfe86c36e7be65dcc2120fd`.

## Reproducibility results

Each binary hash below was observed independently in replicas 1 and 2. The
archive hash is the exact selected replica-1 asset bound by the manifest.

| Target | Binary SHA-256, both replicas | Selected archive SHA-256 |
| --- | --- | --- |
| `aarch64-apple-darwin` | `e3153c39c54cdeeee69508f9597b151bcb8e124cbf7bfd0d64c10ed299ea2879` | `e644d38d48533f9fb1b3f323008be7baf096fca15d871b44e38177c86bbc459d` |
| `aarch64-unknown-linux-gnu` | `8abcaf69f1a4dde00364b060e2bd7ee5cc1517e4e3942c9788922578f4e0d482` | `13a6a2aea3588300ba79319e116550fb9080af9e64880ccc3d535cb5eeafe2fe` |
| `x86_64-apple-darwin` | `16282ca714e10034887603f8f770d9e7fb8c5597a58393f9f0e5d8f18d09e3c2` | `cd53134109c5d6c353eb36172306da63de01d973a0d087e9b257204bbd919ea1` |
| `x86_64-pc-windows-msvc` | `abc4582de05c1ea6f80d134ec46f8d8b9a43a989b58946badc4c39b108840b77` | `e69696082594929930870c29e2db11881532ce449bbe5eff1f05d0d142183b35` |
| `x86_64-unknown-linux-gnu` | `b9f9a0e2735e22633bbde61e3c144e5385d2a41c591cbf45806dd4d05c9ab3e2` | `e726f693649083cb15c50fa99e19c0e24726407a9bff5ae92bac5b26b6aaf8de` |

The selected ZIP and its second replica differ because `Compress-Archive`
retains container timestamps. This does not weaken the recorded claim: the two
extracted executable hashes match byte for byte, both ZIP hashes are preserved,
and the manifest selects and signs one immutable archive.

## Signature and exact-set replay

The manifest covers 15 primary assets: five archives, five sidecars, two
installers, reproducibility evidence, verifier, and verifier library. It binds
version `0.0.1`, tag `v0.0.1`, the candidate commit, byte sizes, roles, hashes,
two replicas, and the source-date epoch. The detached Ed25519 signature verifies
under key ID `pliegors-candidate-2026-01` and fingerprint:

```text
sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250
```

After downloading the sealed artifact on Windows, the repository verifier used
the separately committed public key and reproduced:

```text
Release bundle PASS: v0.0.1 db2ddb3b83d9 15 signed assets
```

Unit mutation gates additionally reject changed asset bytes, extra files,
noncanonical JSON, wrong public keys or fingerprints, bad signatures, target
set drift, sidecar drift, replica identity drift, and binary disagreement.

## Distribution-only golden path

The golden job had no checkout and downloaded only the sealed candidate. It
verified the signature against the fixed fingerprint, installed the Linux x64
archive, and observed `pliego 0.0.1`. The installed CLI generated dependencies
using the canonical Git URL and exact candidate commit, with no path dependency.
Read-only authentication allowed Cargo to fetch that private revision.

The generated default project then passed `pliego check`, its three replay
tests, `pliego build`, `pliego inspect`, `pliego why artifact /`, and uninstall.
The job took 46 seconds including setup; the verify/install/application step
took 29 seconds. Neither a local CLI nor a framework checkout participated.

## Adversarial findings and disposition

The first pipeline attempt rejected Windows because its two PE binaries differed
in 24 bytes: the COFF timestamp and debug identity. Adding MSVC `/Brepro` made
two clean local builds identical and the accepted hosted replicas identical.

The second attempt built and signed successfully, then the golden path rejected
the builder-only `CARGO_INCREMENTAL=0` inherited from global workflow scope.
The variable moved to the matrix build job; the accepted consumer environment
contains no uncommitted Cargo build override.

No gate was removed, converted to a warning, or bypassed. Both failures occurred
before candidate acceptance.

## Residual boundaries

- Binary reproducibility is proven across two hosted jobs using the same pinned
  toolchain and target runner class. It is not yet a diverse-toolchain rebuild.
- ZIP containers are individually hashed and signed but are not byte-reproducible.
- The candidate signing key is an online GitHub secret, not an air-gapped public
  release root. Public launch needs an independently published fingerprint and
  platform signing/notarization decisions.
- Network installers verify SHA-256 sidecars. R6 executes them only after the
  complete bundle signature is independently verified.
- The private Actions artifact expires after 14 days. Committed evidence keeps
  the manifest, signature, hashes, and reproduction record, not the binaries.
- A historical private draft named `v0.0.1` predates this R6 run. Candidate mode
  neither inspected nor mutated it, and it is not accepted R6 distribution.

No unresolved R6 P0 or P1 remains inside the private candidate boundary. No
release, tag, deploy, or public repository operation was performed.
