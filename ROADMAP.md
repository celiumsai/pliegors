# PliegoRS 0.5 roadmap

PliegoRS Next is the development codename for the coordinated PliegoRS `0.5.x`
line. It is an evolution of the existing public beta, not a greenfield rewrite
and not a second product.

The program preserves the verified event, projection, lifecycle, artifact,
server, and release contracts already shipped in `0.4.0-beta.1`. New work joins
those contracts into a causal browser runtime, makes development-host boundaries
explicit, and measures every replacement before it becomes the default.

## Fixed decisions

- PliegoRS Next remains in `celiumsai/pliegors` under Celiums Solutions LLC.
- `main` remains the sole persistent branch. Work uses short-lived branches and
  reviewed changes.
- The next public package line is `0.5.0-*`; existing packages do not restart at
  `0.1.0`.
- The released legacy reference is signed tag `v0.4.0-beta.1` at
  `9cdadea508dfbf78b2ec5061df6846e7fa727211`.
- The technical pre-Next branch point is
  `bdc285fbe208f09c47ffdfbf1081e923884a9cf7`.
- The current native build and development path is the reference host. Vite is
  an experimental host until equivalence and benchmark gates pass.
- The existing `pliego-runtime` package remains the native HTTP runtime. A
  causal browser coordinator will not silently redefine that published crate.
- G4 through G7 are integrated into this program rather than maintained as a
  competing roadmap.

## Ordered phases

| Phase | Outcome | Exit gate |
| --- | --- | --- |
| 0 - Authority and baseline | Frozen references, migration inventory, three fixtures, and one reproducible baseline | Human and machine reports identify three measured bottlenecks |
| 1 - Contracts and conformance | Versioned state root, causal phases, ownership, HMR, host, and evidence contracts | Every P0 invariant has an executable host-independent specification |
| 2 - Build host vertical | Current native flow behind a host boundary and a private Vite spike | Both hosts produce equivalent Pliego manifests, diagnostics, and roots |
| 3 - Causal browser runtime | Events, projections, DOM commits, effects, cleanup, and receipts coordinated transactionally | Determinism, reentrancy, rollback, and lifecycle stress gates pass |
| 4 - SSR, adoption, routing, and HMR | Boundary-local adoption and state-preserving compatible replacement | Every remount or reload has a typed, visible reason |
| 5 - Artifacts, CSS, and evidence | Content-addressed internal artifacts, optional PliegoCSS integration, and evidence modes | Equivalent builds and deployments are reproducible and auditable |
| 6 - Public beta | Cross-platform hardening, external adoption, migration guidance, and inspection tooling | Coordinated `0.5.0-beta.1` release gates pass |

Native and Vite host work is governed by a comparison gate. A host becomes the
default because it improves measured product behavior or compatibility, not
because of implementation-language preference.

## Immediate execution

1. Establish Phase 0 authority and reconcile public product truth.
2. Preserve existing R0-R7, P8, and G1-G4 evidence as regression gates.
3. Build the fixture and baseline contract before adding a daemon or Vite
   integration.
4. Promote existing tests into a stable conformance index.
5. Define the host-neutral boundary, then adapt the native flow before building
   the Vite spike.

Phase 0 fixture and report schemas now exist. The current runner emits an honest
incomplete inventory until executable fixture workloads and collectors land;
unavailable data is never represented as a zero measurement.

The detailed transition plan is in
[`docs/52-pliegors-next-transition.md`](docs/52-pliegors-next-transition.md).
The current package disposition is in
[`docs/53-next-migration-inventory.md`](docs/53-next-migration-inventory.md).
