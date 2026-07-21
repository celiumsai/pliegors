# PliegoRS product constitution

**Status:** accepted for the public preview line on 2026-07-18  
**Applies to:** framework, CLI, OpenSDK contracts, official starters, and release artifacts

PliegoRS is an accountable web platform. It is developed in the open and must
remain useful without a Celiums-operated deployment product. This constitution
defines the questions every new public surface must answer before it enters the
roadmap.

## Five admission pillars

| Pillar | Admission question | Minimum evidence |
| --- | --- | --- |
| Reliability | Can the behavior be reproduced, tested, and recovered? | Adversarial fixture, explicit failure mode, and a receipt or equivalent durable record |
| Adoption | Can a team introduce it without rewriting the whole application? | A measured golden path, migration report, or working bridge |
| Trust | Can a developer identify what executes, what it may access, and what was published? | Structured diagnostics, declared capabilities, and verifiable provenance |
| Openness | Can an independent implementation use the contract without private Rust internals? | Versioned public specification and conformance fixture |
| Portability | Can the output run without a Celiums host and leave one cleanly? | A second target, deterministic export, or provider-neutral output contract |

A feature that only broadens the API does not enter the critical path. Every
proposal must name its owner, compatibility tier, resource budget, cleanup
behavior, diagnostics, and acceptance evidence.

## Product boundary

| Product | Visibility | Boundary |
| --- | --- | --- |
| PliegoRS | Apache-2.0 open source in `celiumsai/pliegors` | Framework, CLI, OpenSDK specifications, conformance suites, portable artifacts, and provider-neutral deployment contracts |
| Pliego.run | Closed source in a separate private repository | Hosted control plane, build orchestration, UI, infrastructure, billing, and operations |
| PliegoCSS | Separate Apache-2.0 public-preview product | Optional build-time companion; never a required PliegoRS dependency |

The PliegoRS repository must not contain the Pliego.run dashboard, control
plane, billing logic, infrastructure implementation, or private provider
adapter. A public build-output contract does not open the hosted product; it
prevents the hosted product from becoming the only implementation.

PliegoRS accepts standard CSS and external CSS pipelines. The delegated
`pliego css check` command is active experimental interoperability with a
separately installed, exact PliegoCSS release candidate. PliegoCSS is never
installed implicitly, linked into the server runtime, or required by the
default starter.

## Stability vocabulary

Stability describes a specific surface, not the whole repository.

| Tier | Compatibility promise | Required gate |
| --- | --- | --- |
| `experimental` | May change or be removed in any release. It must be explicitly labeled and cannot be required by the default starter. | Owner, threat boundary, tests for the claimed behavior, and documented removal path |
| `preview` | Deliberate public contract. Breaking changes require changelog entry, migration guidance, and a version change allowed by pre-1.0 SemVer. | Cross-platform tests for supported targets, structured diagnostics, documentation, and acceptance evidence |
| `stable` | Backward compatibility follows SemVer and the published deprecation policy. Removal requires a major version. | At least one full support window in preview, external conformance use, security review, and migration tooling |

Current classification:

| Surface | Tier |
| --- | --- |
| R0-R7 contracts and their accepted evidence | `preview` |
| Published `pliego-*` Rust APIs and CLI commands in `0.0.x` | `preview` |
| Hyphae protocol client boundary | `preview`; production gateway operation is outside this repository |
| Delegated PliegoCSS command | `experimental`; active optional companion |
| Product topology registry | `experimental` until OpenSDK conformance fixes its external schema |
| OpenSDK, server runtime, PBOC, and Pliego.run provider adapter | Not released |
| Any `stable` public API | None before its explicit promotion record |

## Canonical capability authority

[`product.capabilities.json`](../product.capabilities.json) is the
machine-readable authority for current availability, stability, support
targets, and evidence. Its vocabulary and change rules are defined in the
[product capability manifest](47-product-capability-manifest.md).

README prose, the framework contract, website, release metadata, crates.io
state, and roadmap documents may explain that manifest but may not contradict
it. `npm run check:product-truth` validates the schema and public surfaces in
CI. A Draft RFC, private deployment implementation, or source file cannot
promote a capability by itself.

## Release channels

The channel identifies promotion and support; it does not silently upgrade the
stability tier of an API.

| Channel | Form | Intended use | Support |
| --- | --- | --- | --- |
| `canary` | Expiring GitHub Actions artifact bound to an exact commit | Maintainer and contributor validation | No compatibility or backport promise |
| `beta` | Signed GitHub pre-release and prerelease SemVer tag | External evaluation of a release candidate | Latest beta only; replaced betas receive no feature fixes |
| `stable` | Signed, non-prerelease GitHub Release plus matching crates.io packages | Normal development and production within the target support matrix | Latest published pre-1.0 stable channel release and `main` for security fixes |

Mutable channel names are convenience selectors only. Receipts, lockfiles, and
reproduction instructions always record the resolved exact version and digest.
Promotion reuses already sealed bytes; it never rebuilds a candidate in place.

## Compatibility and support matrix

- **SemVer:** all public packages in one release use the same exact version.
  Before `1.0`, an incompatible public change increments the minor version and
  includes migration guidance. Patch releases do not deliberately break a
  documented contract.
- **MSRV:** Rust `1.86` is the current minimum and the exact release toolchain is
  `1.86.0`. Raising the MSRV requires a versioned decision, changelog entry, and
  clean-machine evidence.
- **Operating systems:** Linux x64 and ARM64 are production release targets.
  macOS x64/ARM64 and Windows x64 are development targets. The exact artifact
  requirements remain in the [distribution guide](27-distribution-and-release.md).
- **Browsers:** current release-blocking browser/WASM lifecycle evidence is real
  Chromium. Firefox and Safari remain compatibility candidates until they run
  the same lifecycle and golden-path corpus in CI or committed physical-device
  evidence. Documentation must not imply a broader browser guarantee.
- **Support window:** before `1.0`, security fixes target the latest stable
  channel release and `main`. A superseded preview may receive a migration note,
  but no backport is promised. The policy will be replaced by an explicit
  multi-line window before the first stable API promotion.

## Telemetry and privacy

The framework and CLI currently send no usage telemetry. Future funnel
reporting must be explicit opt-in, disabled by default, inspectable before
transmission, and independently removable. It may report only documented event
names and coarse environment classes; project paths, source, content,
credentials, environment values, arguments, and artifact payloads are excluded.

A diagnostic or reproduction bundle is local output, not telemetry. Its schema,
redaction rules, size limits, and included files must be documented and tested.
Creating a bundle never uploads it.

## Change control

Every roadmap gate records evidence at an exact source revision. A claim is not
complete because code exists or a local test passed. Promotion requires the
documented supported matrix, adversarial cases, clean-machine path, and release
artifact verification to agree.

