<!-- SPDX-License-Identifier: Apache-2.0 -->

# Project governance

PliegoRS is stewarded by Celiums Solutions LLC. Maintainers are responsible for
technical direction, release integrity, security response, and the long-term
compatibility of public contracts.

## Decision process

Small, reversible changes may be decided in a pull request. Changes to public
APIs, event or artifact schemas, security boundaries, supported platforms, or
project governance require a public issue before implementation once the
repository is open. The decision record must state the compatibility impact,
alternatives considered, and verification evidence.

Maintainers seek technical consensus. When consensus is not possible, the
project steward makes the final decision and records the rationale. A decision
may be revisited when new evidence changes its assumptions.

## Releases

Only maintainers may create official releases. A release must originate from a
reviewed commit on the default branch, pass the documented quality gates, and
publish checksums for every distributed artifact. Drafts and prereleases do not
create a support commitment.

## Conduct and security

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
Vulnerabilities follow [SECURITY.md](SECURITY.md), never the public issue
tracker. Use of the project name and symbol follows
[TRADEMARKS.md](TRADEMARKS.md).

Governance questions may be sent to `hello@pliegors.dev`.
