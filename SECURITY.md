<!-- SPDX-License-Identifier: Apache-2.0 -->

# Security policy

The canonical online trust center is <https://pliegors.dev/security/>. The
machine-readable disclosure contact is published at
<https://pliegors.dev/.well-known/security.txt>.

## Supported versions

Before the first stable release, security fixes are made on the latest
published pre-release and the default branch.

| Version | Supported |
| --- | --- |
| `0.0.2` | Yes |
| Earlier versions | No |

After 1.0, this table will identify every supported release line.

## Report a vulnerability

Do not open a public issue for a suspected vulnerability. Use GitHub's private
vulnerability reporting flow or email `hello@pliegors.dev` with the subject
`SECURITY: short description` and include:

- the affected PliegoRS version or commit;
- the affected crate, CLI command, generated artifact, or browser runtime;
- reproduction steps or a minimal repository;
- expected and observed behavior;
- impact, prerequisites, and any known mitigations;
- whether you intend to publish the report and your preferred attribution.

Never include credentials, private source, or personal data that is not needed
to reproduce the issue.

We aim to acknowledge complete reports within three business days and provide
an initial assessment within seven business days. These are response goals, not
a service-level agreement. We will coordinate a disclosure date after a fix is
available. Please allow reasonable time for supported users to update.

## Scope

Security-sensitive PliegoRS surfaces include the CLI, project scaffold,
development and preview servers, build ledger, content parser, generated HTML,
Rust/WASM runtime, JavaScript adapter boundary, asset pipeline, and Hyphae
protocol client. Vulnerabilities in third-party services or dependencies should
also be reported upstream when appropriate.

Good-faith research that avoids privacy violations, data destruction, service
degradation, and unauthorized access will be handled constructively. This
policy does not authorize testing systems you do not own or have permission to
test.

## Published advisories

There are no published PliegoRS security advisories as of 2026-07-16. This is a
disclosure status, not a claim that undiscovered vulnerabilities do not exist.
Published advisories appear in the GitHub Security Advisories section and are
linked from the online trust center.

## Dependency maintenance

GitHub dependency alerts, CodeQL, secret scanning, `cargo audit`, and `npm
audit` remain active. Automated dependency-update branches are disabled so the
public repository keeps `main` as its sole persistent branch. Maintainers batch
reviewed updates in short-lived branches, pass the protected-branch gates, and
delete those branches immediately after a linear merge.

The current lockfile has no known vulnerability advisories. `cargo audit`
reports one allowed maintenance warning, `RUSTSEC-2026-0173`, for the
build-time transitive `proc-macro-error2` dependency introduced by `rstml`.
It is not a reported vulnerability and is excluded only as a documented
maintenance exception. PliegoRS tracks removal in a maintenance release.
