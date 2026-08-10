# PliegoRS Next migration inventory

**Status:** Phase 0 baseline inventory
**Source revision:** `bdc285fbe208f09c47ffdfbf1081e923884a9cf7`
**Released reference:** `v0.4.0-beta.1`

This inventory decides whether each current package is preserved, adapted,
added to, or removed from the `0.5.x` critical path. `Retire from critical path`
does not mean deleting released code or regression tests.

## Current workspace crates

| Crate | Current authority | Disposition | `0.5.x` action |
| --- | --- | --- | --- |
| `pliego-log` | Typed/versioned event admission, canonical payloads, exact cursors, hash chain, sealed catalogs | Preserve | Reuse as accepted-history authority; add no competing event envelope |
| `pliego-fold` | Transactional projections, replay, reducer/codec identities, contract-bound snapshots | Preserve and extend | Define the state root from existing identities; keep snapshots as verified caches |
| `pliego-reactive` | Dependency graph, scheduling, generational nodes, ownership, effects, disposal | Adapt | Keep graph internals; place a causal event coordinator above the current flush model |
| `pliego-dom` | Views, surgical DOM bindings, keyed reconciliation, ownership, SSR serialization/adoption | Adapt | Add inspectable commit receipts and boundary-local orchestration without replacing the renderer |
| `pliego-artifact` | Portable namespace, exact outputs, build contexts, receipts, causal graph | Adapt | Add algorithm-tagged internal artifact identity and host-neutral manifests |
| `pliego-ssg` | Deterministic documents, routes, assets, graph emission, staged publication | Adapt | Preserve publication; expose host-neutral build inputs and SSR metadata |
| `pliego-macros` | Typed `view!` and component macros | Preserve and extend | Add compiler metadata only after the binding/boundary contract is executable |
| `pliego-resume` | Delegated resumable standard actions | Adapt | Keep as a progressive mode; align state ownership with the causal runtime |
| `pliego-adapters` | ESM lifecycle, bundling, cancellation, cleanup, adapter HMR | Adapt | Retain as an explicit JS boundary and one HMR strategy, not causal-state authority |
| `pliego-router` | Sealed deterministic server route graph | Adapt | Emit a browser-safe route projection and add client route ownership |
| `pliego-runtime` | Native HTTP lifecycle, request ownership, SSR, streaming, receipts | Preserve | Keep the published server meaning; do not reuse the name for the browser coordinator |
| `pliego-data` | Provider-neutral resources, loaders, actions, sessions, cache | Preserve outside early critical path | Maintain regression coverage; integrate only where route/state contracts require it |
| `pliego-pboc` | Provider-neutral deployment manifest and host admission | Preserve and adapt later | Bind new artifact/SSR roots through a versioned PBOC change, never reinterpret v1alpha1 |
| `pliego-cloudflare` | Cloudflare PBOC host adapter | Preserve outside early critical path | Keep provider conformance while browser-runtime work proceeds |
| `pliego-hyphae` | Verified durable-sync client boundary | Preserve | Keep optional; add build evidence only through a separate adapter |
| `pliego-sdk` | OpenSDK admission, Wasm Component execution, capabilities, receipts | Preserve outside early critical path | Reuse capability patterns; do not repurpose its tooling protocol as the build-daemon protocol |
| `pliego-assets` | Deterministic adaptive asset plans and work status | Preserve | Feed its outputs into the host-neutral artifact graph |
| `pliego-content` | Typed bounded content collections | Preserve | Use as fixture and build-graph input; no runtime rewrite |
| `pliego-inspect` | Asset manifests and budget inspection | Adapt | Expand through stable graph, owner, state, HMR, and artifact inspection contracts |
| `pliego-starters` | Embedded exact-version project templates | Adapt | Add a causal starter only after conformance and preserve existing templates during migration |
| `pliego-cli` | Project creation, build, native dev server, HMR, inspection, release commands | Adapt | Extract build sessions and host boundary; retain command and network-bind compatibility |

## New bounded components

| Component | Reason to add | Admission gate |
| --- | --- | --- |
| Core contract module or `pliego-core` | Shared transaction, boundary, root, diagnostic, and receipt types that do not duplicate existing event/fold types | Type ownership and dependency-direction ADR |
| Causal browser coordinator | Join log, projection, reactive graph, DOM commit, effects, and receipts | Host-independent scheduler and rollback conformance |
| Conformance runner | Stable invariant IDs and machine results across implementations | Existing R0-R5 cases imported without weakening them |
| Build-host boundary | Remove direct host semantics from public framework behavior | Native adapter preserves current behavior |
| Build protocol | Versioned messages for persistent sessions and large artifact references | Bounded framing, shutdown, and hostile-input tests |
| `pliegod` | Optional persistent session and inspection authority | Measured benefit over in-process `BuildSession` |
| Private Vite adapter | Compatibility and development-host comparison | Equivalent fixtures, manifests, roots, diagnostics, and cleanup |
| Next fixture suite | Minimal, stress dashboard, and reduced Hyphae Console | One baseline command and frozen source identities |

The fixture suite now has strict manifests and algorithm-tagged source
identities under `fixtures/next`. Source changes require the explicit
`node scripts/update-next-fixture-identities.mjs` maintenance command and then
review of the resulting digest change. The current workload descriptors are
specifications, not accepted measurements.

`hyphae-console` is specified against exact Hyphae `v1.0.1` through the
PliegoRS-owned loopback HTTP `/v2` sidecar boundary in
[`ADR-011`](adr/ADR-011-hyphae-native-sidecar.md). It must not embed the Hyphae
workspace, link Rust `1.89` crates into PliegoRS, expose the sidecar to browsers,
or present protocol-v2 test doubles as the released Native product. G7 blocks
performance claims; durable source-level G8 closure and the completed PliegoRS
sidecar adapter block final fixture acceptance.

No new package is published merely because a directory exists. New crates stay
unreleased in `product.capabilities.json` until their public contract and release
gate are complete.

## Existing behavior retired from the critical path

| Behavior | Treatment |
| --- | --- |
| Whole-body `content` HMR presented as state-preserving HMR | Keep as a legacy strategy with an explicit remount reason until boundary HMR replaces it |
| Unexplained `location.reload()` fallback | Replace with typed reason, diagnostic, and receipt before causal HMR is accepted |
| Direct Cargo and `tiny_http` orchestration as public host semantics | Preserve implementation behind the native host adapter |
| File extension as the only change classifier | Separate source change class, HMR strategy, and fallback reason |
| Snapshot as state authority | Reject; accepted event history and executable contracts remain authoritative |
| Automatic projection-state migration | Reject; use identified, bounded, receipted migrations or full replay |
| `--host` as a build-host selector | Reject; it remains the released network interface option |
| Mandatory PliegoCSS | Reject; it remains a separate optional build-time companion |
| Greenfield replacement of current packages | Reject; evolve current contracts or create additive versioned boundaries |

## Contract collisions to resolve before code

1. `pliego-runtime` already means the native HTTP runtime.
2. Reactive `NodeId`, persistent DOM identity, HMR boundary identity, and server
   stream boundary identity are different concepts.
3. Current reducer and codec identities are stronger than the proposed minimal
   fold descriptor and must not be weakened.
4. `MountedRoot` retention is required for adopted listeners and effects; an
   adoption result cannot hide or discard the lifecycle handle.
5. Current scheduler flush semantics do not create a separately identified
   transaction for effect-produced updates.
6. Stable browser WASM uses `panic=abort`; native unwind recovery cannot be
   generalized to browser recovery.
7. Current SHA-256 formats and any new internal digest algorithm need explicit,
   versioned mapping.
8. Current HMR kind describes a browser action, while the new model also needs
   source classification and fallback reason.

## Dependency direction

The target direction is:

```text
versioned core contracts
        ^
event log / projection / reactive graph / DOM
        ^
causal browser coordinator
        ^
SSR adoption / HMR / client routing
        ^
CLI and build-host adapters
```

Vite, Node, Cargo process APIs, HTTP servers, and provider SDKs stay in adapters.
The core contracts do not import host implementations.
