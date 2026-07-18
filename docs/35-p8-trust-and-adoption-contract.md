# P8 trust and adoption contract

**Status:** in progress  
**Target line:** `0.0.2`  
**Audit baseline:** local `main` at `a13cd4a` on 2026-07-18

P8 makes the existing public preview diagnosable, reproducible, measurable, and
safe to evaluate before PliegoRS expands into OpenSDK or a server runtime. R0-R7
remain mandatory regression gates.

## Audited baseline

| ID | Requirement | Baseline result | Closure evidence |
| --- | --- | --- | --- |
| P8-A01 | Constitution, stability tiers, and open/closed product boundary | Contract added in [product constitution](34-product-constitution.md) | Documentation-link gate and review against public surfaces |
| P8-A02 | Canary, beta, and stable release channels; SemVer, MSRV, browser, OS, and support policy | Defined by the constitution; workflow has candidate/draft modes but no beta/stable promotion proof yet | Workflow tests plus one sealed canary and beta dry run |
| P8-A03 | `pliego doctor`, `pliego report --bundle`, and `pliego upgrade --check` | Implemented after audit; final release-only and degraded-machine evidence remains open | CLI contract tests on clean, degraded, redacted, and incompatible fixtures |
| P8-A04 | Installer signature verification, SBOM, SLSA provenance, and Sigstore identity | SBOM, SLSA-compatible provenance, exact attestation manifest, Sigstore workflow, and local tamper tests are implemented; installer verification and hosted-candidate proof remain open | Tamper tests and a sealed release candidate where every artifact is covered |
| P8-A05 | Fuzz, property, and adversarial suites for trust boundaries | Six maintained targets, reviewed seed corpora, bounded CI smoke, and reproducible long-run instructions are implemented; hosted CI evidence remains open | [Fuzzing contract](38-fuzzing-and-adversarial-testing.md), local WSL2 smoke, and hosted candidate run |
| P8-A06 | Reproducible cold/warm/content/CSS/Rust-view/browser-apply/memory benchmarks | R5 measures first-app p50/p95 and lifecycle plateau; the complete P8 benchmark set is absent | Versioned harness, raw samples, hardware fingerprint, p50/p95, and honest scope statement |
| P8-A07 | Release-only golden paths on Windows, macOS, Linux, WSL, container, Unicode, and long paths | Five release targets and several Unicode/WSL fixtures exist; the complete environment matrix is not one gate | Matrix report bound to the candidate commit and exact release bytes |
| P8-A08 | Telemetry disabled by default and voluntary funnel report | No framework/CLI telemetry exists; no opt-in funnel reporter exists | Network-denial test, documented schema, local preview, explicit consent, and deletion path |

No row becomes complete from inspection alone. The final evidence file must link
commands, fixtures, raw outputs, runner identities, and the exact candidate.

## CLI contracts

### `pliego doctor`

`doctor` is read-only and works both inside and outside a project. It reports:

- CLI version, host OS/architecture, and executable identity;
- Rust and Cargo availability and versions;
- `rustup`, the `wasm32-unknown-unknown` target, and `wasm-bindgen` when a client
  package requires them;
- nearest project root, manifest validity, Cargo lock state, output path safety,
  and first-party package version alignment when a project exists;
- stable check IDs, status, cause, and an actionable next step.

Human output is the default. `--format json` emits a versioned schema and no
ANSI control sequences. Exit `0` means no required check failed, exit `1` means
one or more required checks failed, and usage remains exit `2`. Optional tools
may be warnings but must not make a project appear healthy when its declared
workflow needs them.

### `pliego report --bundle`

`report --bundle` creates a bounded local reproduction archive and never
uploads it. The exact-set archive includes a canonical manifest, redacted doctor
report, Pliego manifest, dependency metadata, lockfile digest, build/inspection
reports when present, and an explicit omission ledger.

The collector rejects links, device files, path escapes, unbounded files, and
unknown implicit inputs. It excludes source, content, `.env*`, credentials,
environment values, VCS objects, generated binaries, and absolute home/project
paths. Tests seed recognizable secrets in every candidate input and prove none
survive in filenames or bytes. Repeated collection from unchanged normalized
inputs produces the same manifest and payload digests.

### `pliego upgrade --check`

`upgrade --check` is read-only. It compares the CLI, manifest, Cargo metadata,
lockfile, and all first-party packages against an explicit or resolved target
version. It reports compatible, migration-required, or blocked with stable
reasons. It never edits `Cargo.toml`, `Cargo.lock`, source, or generated output.
Network resolution, when added, is explicit and cached; an offline exact target
must remain supported.

## Release and supply-chain contract

Each promoted target archive must have:

1. the existing SHA-256 sidecar and Ed25519 exact-set manifest entry;
2. an installer path that verifies trusted identity and exact manifest coverage
   before replacing a binary;
3. a machine-readable SBOM bound to the source revision and archive digest;
4. SLSA-compatible provenance identifying builder, invocation, materials, and
   subject digests;
5. keyless Sigstore identity evidence in addition to the project-controlled
   Ed25519 continuity key;
6. offline verification instructions and adversarial tests for missing, extra,
   substituted, replayed, and mismatched assets.

The implementation must not claim a SLSA level or Sigstore transparency entry
until an actual candidate is independently verified. Platform notarization and
Authenticode remain separate claims.

## Verification suites

The maintained corpus covers routes and portable paths, project and plugin
manifests, event schemas, snapshots, DOM construction/adoption, and capability
declarations. Every fuzz target has deterministic seed cases and bounded input
sizes. CI executes a short deterministic smoke; maintainers can reproduce the
long run from one documented command and retain every minimizing regression.

Benchmarks publish raw observations and summary statistics for:

- clean cold build;
- no-change warm build;
- content-only and CSS-only change;
- Rust view change;
- browser update application;
- reactive and DOM lifecycle memory plateau.

Each report records OS, CPU, memory, storage class, Rust/Node/browser versions,
power mode, sample count, warmup, cache state, and source revision. PliegoRS does
not publish competitor comparisons unless their harnesses, equivalent work, and
raw data are reviewable.

## Golden environment matrix

At least one path in each environment installs from sealed distribution bytes,
not a framework checkout, then runs:

```text
install -> version -> doctor -> new -> check -> dev smoke -> build -> inspect
        -> report --bundle -> upgrade --check -> uninstall
```

The matrix includes Linux x64, Linux ARM64, macOS x64/ARM64, Windows x64, WSL2,
a pinned Linux container, a Unicode workspace, and a platform-appropriate long
path workspace. Unsupported host limitations are explicit results, not skipped
successes.

## Telemetry acceptance

The zero-telemetry path is release-blocking. With reporting disabled, the CLI
must perform no framework-owned network request during the local golden path.
Opt-in reporting, once implemented, requires a deliberate command or config
change, a local event preview, a versioned event allowlist, bounded storage,
retry limits, redaction tests, and `pliego telemetry disable --delete-local`.

The initial funnel is limited to `install`, `new`, `check`, `dev`, and `build`.
It is unsuitable for billing, identity, or project analytics and must not be
implemented as a hidden prerequisite for Pliego.run.

## Exit gate

P8 closes only when:

- a clean machine completes the release-only golden path;
- every emitted error in that path has a stable code, cause, and concrete next
  action, with a span when a user-controlled file location exists;
- every distributed artifact is covered by verifiable identity, SBOM, and
  provenance evidence;
- the full environment, adversarial, privacy, and benchmark reports are bound
  to one candidate revision; and
- the committed `docs/evidence/p8-trust-and-adoption.md` records all limitations
  without promoting experimental surfaces.

That evidence file is created only after every row passes. P9 OpenSDK work may
be prepared in RFC form, but implementation does not become the critical path
before this gate closes.
