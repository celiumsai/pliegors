# PliegoRS Next transition program

**Status:** active planning and Phase 0 execution
**Codename:** PliegoRS Next
**Public release line:** `0.5.x`
**Owner:** Celiums Solutions LLC
**Started:** 2026-08-08

## Purpose

This program evolves PliegoRS into a causal, deterministic, inspectable browser
framework while preserving the public and verified contracts already present in
the repository. It replaces neither the signed release history nor mature
subsystems merely to match a proposed directory layout.

The central product goal is:

> One Rust-first UI runtime in which accepted events produce deterministic
> projections, owned DOM changes, explicit effects, and verifiable receipts,
> with a development experience hosted behind replaceable internal boundaries.

## Source authority

Two immutable references have different roles:

| Reference | Revision | Authority |
| --- | --- | --- |
| `v0.4.0-beta.1` | `9cdadea508dfbf78b2ec5061df6846e7fa727211` | Released packages, signed distribution, public compatibility, and legacy performance claims |
| pre-Next branch point | `bdc285fbe208f09c47ffdfbf1081e923884a9cf7` | Technical inventory and the starting source state for `0.5.x` work |

The release tag remains the legacy release authority. The branch-point commit
does not retroactively change released bytes or evidence. Historical benchmark
reports remain bound to their recorded revisions until Phase 0 remeasures the
frozen fixtures.

## Existing foundations

The transition starts with reusable implementation, not empty skeletons:

- `pliego-log` already owns typed, versioned, hash-chained event history.
- `pliego-fold` already owns transactional projection, replay, canonical state,
  and contract-bound snapshots.
- `pliego-reactive` already owns generational graph nodes, scheduling, equality
  gates, ownership, disposal, and unwind-target recovery.
- `pliego-dom` already owns surgical updates, keyed identity, listeners,
  lifecycle cleanup, SSR serialization, and strict adoption.
- `pliego-artifact` and `pliego-ssg` already own portable output namespaces,
  exact-set verification, causal graphs, and atomic publication.
- `pliego-cli` already owns the native watcher, verified builds, development
  server, SSE updates, diagnostics, and explanation commands.
- `pliego-router`, `pliego-runtime`, `pliego-data`, and `pliego-pboc` already
  provide the released native server and deployment path.

The implementation gap is coordination. No current authority executes the
complete browser transaction:

```text
COLLECT -> REDUCE -> PLAN -> COMMIT -> EFFECT -> RECEIPT
```

No current HMR path snapshots an authorized projection, loads changed Rust code,
replays accepted events, verifies a state root, and reports the resulting
strategy.

## Non-negotiable compatibility rules

1. Existing package names retain their released meaning unless a separate
   migration decision explicitly versions the change.
2. Existing event, snapshot, build-report, graph, SSR-marker, adapter, OpenSDK,
   and PBOC formats are never reinterpreted in place.
3. New persistent identities include an algorithm and format version. Existing
   SHA-256 fields remain SHA-256.
4. Accepted history remains state authority. A projection snapshot remains a
   verified cache unless a separate signature and authority policy promotes it.
5. Full replay is the safe default after a projection-contract change. State
   migration requires its own identity, canonical input and output, bounds, and
   receipt.
6. Vite, Cargo, wasm-bindgen, Node, and Hyphae types do not enter the public
   Pliego application API.
7. A reload, remount, broad invalidation, or state loss is never silent.

## Phase 0 - authority and baseline

The versioned fixture and report contracts are now defined by:

- `fixtures/next/manifest.json`;
- `schemas/pliego.next-fixture-manifest.schema.json`;
- `schemas/pliego.next-baseline-report.schema.json`.

The three fixture directories freeze their objectives, source donors, and
required capabilities. Minimal and stress dashboard are `executable`; Hyphae
Console is `specified` against an exact external release but is not executable.
An inventory run reports every unimplemented collector as `unavailable` rather
than inventing a zero measurement.

Fixture source identity is `sha256-tree-v1`: a domain-separated SHA-256 over
sorted portable relative paths and exact file bytes, excluding only the fixture
root's generated `target/` directory. Validation rejects links, non-regular
entries, case/Unicode path aliases, oversized trees, and any source byte or path
change not reflected in `fixtures/next/manifest.json`.

### Deliverables

- exact release and pre-Next references;
- current architecture and dependency inventory;
- preserve/adapt/add/retire matrix;
- reconciled product-truth documents and stronger drift checks;
- `fixtures/next/minimal`, `fixtures/next/stress-dashboard`, and
  `fixtures/next/hyphae-console`;
- a versioned baseline-result schema;
- one baseline command producing `baseline.json` and `baseline.md`;
- machine, toolchain, browser, cache, sample, and source metadata;
- p50, p95, and p99 summaries;
- three measured bottlenecks.

### Required measurements

- cold and verified warm build;
- Cargo/rustc, wasm-bindgen, site, transform, and verification phases;
- development cold and warm startup;
- filesystem change to server update;
- filesystem change to browser-visible update;
- DOM mutations per update;
- process-tree and long-running development RSS;
- active owners, listeners, effects, and mounted roots;
- emitted WASM and asset bytes;
- cache attempts, hits, misses, and reuse;
- remount and full-reload counts grouped by typed reason.

Legacy measurements may report a metric as `not-applicable` only with a stable
reason. Missing instrumentation is not recorded as zero.

### Exit gate

The same command reproduces all three fixtures and emits human and machine
reports from the same observations. The reports bind the exact source, fixture,
machine, tools, cache state, warmup, and sample count. The three largest measured
bottlenecks are named without extrapolating beyond the measured environment.

The Hyphae Console fixture is specified, not replaced by a mock product. Hyphae
`v1.0.1` and its closed bounded G6 functional profile are sufficient to begin a
PliegoRS-owned loopback HTTP `/v2` sidecar adapter without raising the framework
MSRV. [`ADR-011`](adr/ADR-011-hyphae-native-sidecar.md) freezes that boundary.
G7 still blocks performance and scale claims, while durable source-level G8
closure and the completed PliegoRS sidecar adapter block executable-fixture
acceptance and a complete Phase 0 baseline.

### Current commands

```sh
npm run test:next-baseline
npm run measure:next-baseline -- --output target/benchmarks/next-baseline-inventory
npm run measure:next-baseline -- --execute-minimal --output target/benchmarks/next-minimal-smoke
npm run measure:next-baseline -- --execute-stress-dashboard --output target/benchmarks/next-stress-smoke
npm run measure:next-baseline -- --execute-minimal --execute-dev-hmr --output target/benchmarks/next-minimal-dev
npm run check:next-baseline -- --baseline target/benchmarks/next-baseline-inventory
```

`measure:next-baseline` currently publishes the exact fixture/metric inventory
as an `incomplete` report. It writes `baseline.json` and `baseline.md` from the
same validated in-memory report through one staged directory rename. The command
refuses to overwrite an existing destination. Accepted performance evidence is
blocked until the fixture workloads and collectors produce the declared five
independent observations.

`--execute-minimal` runs one discarded warmup plus five independent cold/warm
build pairs for the executable minimal fixture. It measures wall-clock build,
native build phases, emitted WASM and asset bytes, verified artifact reuse, and
Linux process-tree RSS. Cache attempts/hits/misses, transforms, development
startup/HMR, browser-visible updates, DOM mutations, lifecycle counters, and
typed remount/reload reasons remain explicitly unavailable.

`--execute-stress-dashboard` uses the same source-bound native build collector
for the executable 1,536-row workload. Both execution flags may be combined;
the CLI is built once and each fixture keeps independent projects and cold Cargo
targets.

`--execute-dev-hmr` adds native dev startup and CSS HMR observations for each
selected executable fixture: cold/warm readiness, file-change-to-SSE, computed-
style browser settlement after stylesheet load, and MutationObserver record
count. It deliberately leaves remount/full-reload metrics unavailable because
the current host does not emit typed fallback receipts.

The `native-dev-v1` workload uses a fresh fixture copy per sample, one discarded
warmup and five measured runs. Each run starts a cold dev process from an empty
Cargo target, validates HTTP 200 fixture readiness, stops it, starts a warm
verified process, and performs exactly one direct `site/style.css` write. CSS
visibility requires the new computed outline color plus two animation frames.
DOM mutations count delivered `MutationRecord` objects from an observer attached
to `document.documentElement` before the write.

Every CSS update also captures the verified `pliego-build/1` record after SSE
publication. The report separates discovery, Rust/WASM, site execution, and
verification from `hmr-host-transport-overhead`, defined as observed
file-change-to-SSE latency minus the build record's non-overlapping total phase
duration. This remainder includes watcher debounce, host bookkeeping, SSE
polling, scheduling, and loopback delivery; it is not attributed to one of those
subsystems without narrower instrumentation.

## Phase 1 - contracts and conformance

Existing R0-R5 tests become the seed corpus. New invariant IDs cover only
behavior that is not already authoritative:

- effect-produced events execute in the next causal transaction;
- commit cannot mutate the active transaction inputs;
- equal history and executable contracts produce the same versioned state root;
- boundary-local SSR mismatch preserves unaffected siblings;
- compatible HMR preserves authorized state;
- incompatible HMR selects migration, boundary remount, or reload with a typed
  reason;
- client navigation changes route ownership without a document reload;
- native and Vite hosts produce equivalent Pliego-owned outputs.

`StateRoot` v1 is fixed by
[`ADR-009`](adr/ADR-009-state-root-v1.md). Its first implementation remains in
`pliego-fold`; extraction to a shared bottom-level crate waits for a second real
consumer and a separate dependency-direction decision.

The first scheduler proof is fixed by
[`ADR-010`](adr/ADR-010-causal-transaction-scheduler.md). `pliego-fold` now owns
an iterative, bounded `CausalScheduler` whose effect handle can only enqueue into
the next transaction generation. Receipts bind exact before/after StateRoots;
external effect receipts remain a later layer.

The direct renderer now exposes an opt-in `DomCommitObserver` through
`mount_with_observer` and `adopt_with_observer`. It emits an immutable `DomPlan`
after validation and immediately before each dynamic text, dynamic attribute,
subtree, or keyed mutation, then emits a terminal `DomCommitReceipt` for that
same renderer path only after success. A plan without a receipt therefore means
the operation did not complete; it does not claim rollback. Plans use
mount-local plan and target IDs, summarize structural work without exposing
`web_sys`, raw node moves, listeners, or value contents, and sample optional
coordinator-owned transaction and state-root metadata from the observer. This is
observation, not a public `commit(plan)` API; the browser mutation remains the
point of no rollback.

The conformance runner must execute without Vite and emit a stable JSON result.
Browser-only tests are allowed for DOM behavior but cannot become the sole proof
of state, scheduler, ownership, or host contracts.

## Phase 2 - BuildHost vertical

The first implementation adapts the existing native build and development flow.
The contract must support sessions, cancellation, structured diagnostics,
artifact references, invalidation, and HMR publication without exposing a
specific transport.

The Vite implementation starts as private repository tooling. It is not an npm
product and cannot become the default until it passes:

- fixture equivalence;
- conformance equivalence;
- manifest and state-root equivalence;
- clean shutdown and orphan-process tests;
- startup, update latency, and memory comparisons;
- explicit documentation of plugin and reproducibility limits.

`--host` retains its released network-bind meaning. A future selector uses
`--build-host`.

`pliegod` is not a prerequisite by name. A daemon is added only when persistent
sessions materially improve compile latency, graph authority, inspection, or
multi-client coordination. Its protocol must be versioned, bounded, and covered
by hostile-input tests.

## Phase 3 - causal browser runtime

The runtime coordinator composes current crates rather than copying them. It
adds typed transaction identity, explicit phases, event queueing, a versioned
state root, staged DOM plans, effect scheduling, cancellation, and transaction
receipts.

`pliego-runtime` keeps its released native HTTP meaning. The browser coordinator
receives a distinct package or module name only after its public boundary is
specified. `ReactiveNodeId`, persistent DOM identity, HMR boundary identity, and
server stream-boundary identity remain distinct types.

The browser panic contract must be decided before beta. Stable production WASM
currently uses `panic=abort`; the project will not claim recoverable browser
panics until exception handling or replaceable-instance isolation proves it.

## Phase 4 - SSR, adoption, routing, and causal HMR

Current `pliego:ssr:v1` structural markers remain valid. New versioned metadata
binds persistent boundary identity, route identity, component artifacts,
projection root, optional snapshot reference, and CSS/WASM references.

Adoption returns both the retained lifecycle handle and a typed strategy. HMR
separates source classification from execution strategy and fallback reason.
Receipts bind before and after artifacts, state roots, replay range, affected
boundaries, resource counts, and any fallback.

Client routing consumes a browser-safe projection of the sealed route graph. It
updates history only after the new route commits, disposes route-owned resources,
preserves application owners, and records a navigation receipt.

## Phase 5 - artifacts, CSS, and evidence

Internal content-addressed objects may use a new algorithm-tagged identity.
Release files, PBOC fields, Cargo-facing checksums, SBOM subjects, and Sigstore
materials retain their existing algorithms unless their own versioned contracts
change.

PliegoCSS remains a separate optional product. Integration may consume its
documented compiler, manifest, and watch contracts, but it does not make
PliegoCSS a runtime or default-starter dependency.

Evidence modes are explicit:

- `dev-fast`: bounded local journal and asynchronous evidence work;
- `ci-verified`: deterministic manifests, roots, and repeated-build checks;
- `release-strict`: signed build and deployment evidence under the release
  authority, with optional Hyphae integration behind an adapter.

## Phase 6 - `0.5.0-beta.1`

The public beta requires the official site, reduced Hyphae Console fixture, and
an unaffiliated application. It preserves existing release, security, and
distribution gates and adds causal-runtime conformance, host comparison,
long-running development tests, inspection tooling, and migration guidance from
`0.4.0-beta.1`.

NativeHost is not a future rewrite gate because a bounded native host already
exists. The post-beta decision is instead whether native, Vite, or both remain
supported development hosts based on measured value and maintenance cost.

## Integrated G4-G7 outcomes

- G4 external adoption becomes a `0.5` beta input and exit gate.
- G5 OpenSDK remains a bounded extension program and does not block the causal
  runtime unless an extension boundary is required by that runtime.
- G6 owner redundancy, support exercise, and incident drill remain release
  maturity gates.
- G7 competitive claims remain blocked until reproducible comparisons and
  external production evidence exist.

## Definition of done

A task is complete only when it compiles on the applicable stable Rust target,
has executable specification coverage, passes relevant conformance, emits typed
diagnostics, introduces no silent fallback, updates versioned contracts when
needed, and records performance or lifecycle evidence when it changes a hot
path or resource owner.
