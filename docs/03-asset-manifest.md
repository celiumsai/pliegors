# PLIEGO asset manifest 1.0

**Status:** Phase 1 contract
**Schema:** `schemas/pliego.asset-manifest.schema.json`

## Purpose

`pliego.asset-manifest.json` is the deterministic inventory for a PliegoRS
project. It answers four different questions without
collapsing them into one field:

1. What is the asset and where did it come from?
2. Who owns it and under what rights can it be distributed?
3. Which encoded variants exist for each adaptive tier?
4. Which variants participate in a route and delivery-phase budget?

The manifest contains no build time, machine path, random identifier, or other
volatile field. Identical inputs serialize to identical bytes.

## Identity and coverage

`work.kind` identifies the project class and `work.id` remains stable across
builds. Optional project metadata cannot weaken the identity or hash contract.

`coverage.assetRoot` is a logical root, normally `public`. Paths inside variants
are POSIX-style and relative to that root. `trackedExtensions` defines the files
that the coverage gate considers assets. Every matching file must be represented
by exactly one variant or explicitly listed in `coverage.excluded` with a reason.

## Provenance and rights

Every asset declares:

- `owner`: the entity responsible for the asset and its role;
- `source`: how the asset entered the work and a durable locator;
- `rights`: ownership, license state, and distribution treatment;
- `visualImportance`: whether the asset is critical, supporting, decorative, or
  utility material.

`rights.transfer` records whether redistribution is included, excluded, or
limited; it is not a substitute for the applicable license text.
It is the machine-readable input used to generate and audit that schedule.
Qualified counsel still reviews the signed instrument before a real closing.

## Variants and tiers

Each variant has its own byte length and lowercase SHA-256. A variant can serve
more than one tier. The four tier names are fixed:

| Tier | Contract |
| --- | --- |
| `universal` | Complete authored experience with no 3D runtime dependency |
| `lite` | Restrained interactive media for modest devices |
| `balanced` | Higher fidelity within measured device headroom |
| `signature` | Full authored expression for capable devices |

`delivery` describes when bytes are eligible to move: `initial`, `deferred`,
`on-demand`, or `download`. It does not claim that the browser actually loaded
the file; runtime measurement proves that separately.

Raster and texture variants should declare `dimensions` and
`estimatedVramBytes`. Geometry variants should also declare triangles and draw
calls. These estimates let `pliego inspect` reject obviously impossible scene
packages before a browser run.

## Budget scopes

A global sum of every asset on a multi-route site is not a useful performance
budget. `budgetScopes` therefore name an exact route, tier, delivery phase, and
set of variant IDs. The inspector computes transfer, VRAM, triangles, and draw
calls only from that set.

Runtime metrics such as LCP, INP, CLS, decode time, main-thread work, and frame
time live in the measurement report. They are observed facts, not asset metadata.

## Deterministic rules

`pliego inspect` applies constraints that JSON Schema cannot express cleanly:

- asset IDs, variant IDs, budget IDs, and file paths are unique;
- every budget reference resolves to a declared variant;
- every fallback reference resolves and cannot point to itself;
- a budget can only reference a variant available in its tier;
- a budget phase must match the referenced variant delivery phase;
- byte counts, hashes, and coverage match disk when an asset root is supplied;
- transfer, VRAM, geometry, and draw-call limits are calculated exactly.

The canonical JSON output uses sorted maps and arrays in manifest order. It has
no timestamp, absolute path, or host-specific metadata.

## Commands

Inspect one manifest and verify its files:

```powershell
cargo run -p pliego-inspect -- inspect `
  path/to/pliego.asset-manifest.json `
  --asset-root path/to/public
```

Produce the committed cross-fixture baseline from manifest snapshots:

```powershell
cargo run -p pliego-inspect -- baseline fixtures/targets.json `
  --format json --output fixtures/phase-1/baseline.current.json
```

Use `--enforce-budgets` when a phase gate requires every declared budget to
pass. Phase 1 records current failures; Phase 2 must remove the asset failures
before its gate can close.

Refresh the three fixture snapshots and synchronize a project-local manifest:

```powershell
node scripts/snapshot-fixture-assets.mjs `
  --site path/to/pliegors/examples/pliegors-site `
  --sync-projects true
```
