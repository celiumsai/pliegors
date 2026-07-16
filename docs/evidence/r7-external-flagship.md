# R7 external flagship acceptance evidence

**Gate:** R7 - External flagship

**Status:** Accepted

**Recorded:** 2026-07-16

**Framework candidate revision:** `db2ddb3b83d99f6d9ebb63218f1579a860935588`

**External application:** Cairn

**External application revision:** `75823a7da847f50f2577613f83868286ebc23da2`

**External application tree:** `cf9774bea76a36efbc8a86dcb84bee5f0b78ce3c`

## Acceptance matrix

| ID | Requirement | Evidence | Result |
| --- | --- | --- | --- |
| R7-A01 | Build outside the PliegoRS monorepo | Cairn is an independent private Git repository outside the PliegoRS checkout; no application source or machine-local path is copied into this repository | PASS |
| R7-A02 | Consume only the accepted candidate | The signed R6 bundle was verified before extraction; the candidate CLI scaffolded the project without `--framework-path`; all seven resolved `pliego-*` crates use the exact accepted Git revision | PASS |
| R7-A03 | Prove a real durable application | Cairn is an operational human-agent decision dossier with append-only journals, proposals, decisions, effect requests, receipts, branches, audit, and selective sync | PASS |
| R7-A04 | Make live and replayed state agree | Full replay, cursor replay, explicit fork, branch comparison, and post-write persisted replay are covered by tests and CLI verification | PASS |
| R7-A05 | Fail closed under tampering | Altered journal hashes, duplicate terminal receipts, and altered sync signatures are rejected | PASS |
| R7-A06 | Exercise authority and selective sync | Human, agent, and system actors are recorded; branch-and-actor selection emits two verified streams and excludes the unselected system event | PASS |
| R7-A07 | Produce a complete PliegoRS interface | The candidate CLI builds and verifies five routes, responsive navigation, audit views, sync evidence, a custom favicon, and a real 404 document | PASS |
| R7-A08 | Preserve private pre-release boundaries | Cairn has no Git remote; no release, deploy, public repository, or production Hyphae claim was created | PASS |

## Product exercised

Cairn records an investigation as typed, versioned facts. Its accepted fixture
contains nine events across two branches and three actor classes. An agent
submits a proposal, a human records a decision, an effect request settles only
through a terminal receipt, and a fork records divergent evidence without
rewriting the original branch.

The application provides:

- seven typed event schemas with UUIDv7 identities, causal parents, actor
  authority, source references, and SHA-256 provenance;
- an append-only bounded JSONL journal protected by the `pliego-log` hash chain,
  adjacent writer locking, `sync_data`, and read-after-write replay;
- deterministic folds, exact cursor replay, explicit branch forks, symmetric
  branch deltas, and audit filtering by user or agent;
- effect request/receipt invariants that reject receipts before requests and
  duplicate terminal receipts before changing journal bytes;
- branch-and-actor selective sync using `pliego-hyphae` append batches, verified
  receipts, page attestations, and independent persisted-evidence replay;
- a PliegoRS SSG interface for overview, branches, audit, sync, and not-found
  routes, with useful HTML independent of client JavaScript.

This is application behavior, not a framework marketing page.

## Candidate provenance

The private R6 bundle was independently replayed before Cairn was created:

```text
Release bundle PASS: v0.0.1 db2ddb3b83d9 15 signed assets
public key fingerprint: sha256:97df5a29b5d4be6f626634b6824eebea5f2e7fcfa9c93ed644a3a2913dad7250
candidate CLI: pliego 0.0.1
```

`cargo metadata --locked` resolved exactly these framework packages from the
accepted Git source and no path source:

```text
pliego-artifact, pliego-dom, pliego-fold, pliego-hyphae,
pliego-log, pliego-reactive, pliego-ssg

git+https://github.com/celiumsai/pliegors?rev=db2ddb3b83d99f6d9ebb63218f1579a860935588#db2ddb3b83d99f6d9ebb63218f1579a860935588
```

The application retains the licenses for the three font files copied from the
candidate's maintained cinematic starter.

## Deterministic evidence

The accepted application data is committed at the external application
revision above:

```text
data/demo.journal.jsonl
  bytes:  8413
  sha256: f090ce0c3804f1eda2383e0808f5873c09e57b466d5c039d36539adbea8d8bb3

data/demo.sync.json
  bytes:  43942
  sha256: 44cd25e3728c138e00d4bda1d231c8bb85009b9cb86de94ef7259bbe3dd9200d
```

Two consecutive selective-sync executions produced the same evidence hash.
The evidence contains eight selected events, one deliberately excluded event,
two branch streams, signed append responses, signed pull pages, public key,
cursors, and replayed event identities. No `.lock`, `.next`, or `.previous`
recovery file remained after acceptance.

## Automated gates

The final external tree passed:

```text
cargo fmt --all -- --check                                      PASS
cargo test --all-targets                                       PASS (7 tests)
cargo clippy --all-targets -- -D warnings                      PASS
pliego check                                                   PASS
pliego build                                                   Cairn -> target/site [c09dcf2a25ac]
pliego inspect                                                 VERIFIED c09dcf2a25ac / 5 HTML routes / 13 files / 125202 bytes
cairnctl verify data/demo.journal.jsonl data/demo.sync.json     journal PASS: 9 events / 2 branches
                                                               sync PASS: 8 selected / 1 excluded / 2 streams
```

The seven adversarial tests cover cursor replay and explicit divergence,
duplicate-receipt atomicity, journal hash tampering, receipt-signature
tampering, selective sync, agent-scoped audit, and exact candidate dependency
provenance. Markdown links, JSON parsing, secret patterns, generated-file
exclusions, and CSS product constraints also passed before the clean external
commit.

## Browser acceptance

The final candidate build was exercised in Chromium at `1440x1000` and
`390x844`:

- overview, branches, audit, sync, and 404 routes rendered without page-level
  horizontal overflow;
- all four causal nodes remained visible, and the compact mobile layout exposed
  the next metric within the first viewport;
- navigation current-state, two branch panels, nine audit rows, two sync
  streams, disclosure control, and 404 recovery link worked;
- console warning and error collections were empty;
- reduced-motion emulation matched and reduced animation duration to the
  effectively disabled value while restoring automatic scrolling.

## Findings closed during the flagship

The first branch comparison counted only left-side proposal identities. A
right-only proposal therefore disappeared from the delta. The implementation
now compares the symmetric proposal-ID union, and the accepted test proves one
proposal difference, zero effect drift, and one decision drift.

The first journal writer attempted to acquire its adjacent lock before creating
the parent directory. The writer now creates the bounded journal directory
first. Sync evidence publication was also hardened to stage `.next`, preserve
`.previous`, and restore the previous evidence if publication fails.

## Residual boundaries

- The sync authority is a deterministic local Ed25519 fixture used to exercise
  the complete verified client contract. It is not production Hyphae PKI,
  tenant authentication, key rotation, or revocation.
- Selective sync evidence is local application data. No network transport or
  deployed conforming Hyphae service is claimed.
- The candidate remains private. Its Git dependency required authorized
  read-only access during this test.
- Browser acceptance proves the recorded desktop and mobile viewports, not a
  universal device matrix or a Lighthouse performance claim.
- The external repository is intentionally local and has no remote. Its exact
  commit and tree identity make the accepted source state auditable without
  publishing it.

No unresolved R7 P0 or P1 remains inside this external flagship boundary. No
release, deploy, public repository, or production credential operation was
performed.
