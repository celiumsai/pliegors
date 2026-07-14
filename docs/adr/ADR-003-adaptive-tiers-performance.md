# ADR-003: Adaptive visual tiers and performance contract

**Status:** Accepted
**Decision date:** 2026-07-10
**Scope:** Assets, motion, 3D, PliegoRS islands, and physical-device gates

## Context

PLIEGO promises authored digital works, not one oversized scene forced onto every
device. Shipping identical WebGL, geometry, textures, video, and motion to modest
phones, iPads, integrated-GPU laptops, and capable desktops would either flatten
the strongest experience or fail users with less capable hardware.

The visual system therefore needs adaptive fidelity without treating a fallback
as a broken or lesser page. Performance is part of the work's authorship.

## Decision

Every Pliego ships four art-directed tiers:

<!-- markdownlint-disable MD013 -->

| Tier | Contract |
| --- | --- |
| **Universal** | Complete semantic HTML, typography, authored imagery or poster media, navigation, accessibility, and the core narrative. It downloads no 3D runtime or scene. |
| **Lite** | A deferred, reduced Three.js scene with bounded geometry, textures, draw calls, motion, and GPU memory. |
| **Balanced** | The full composition with moderated resolution, effects, and simulation for devices that sustain it. |
| **Signature** | The highest authored fidelity, selected only after capability evidence and still bound by a work-specific budget. |

<!-- markdownlint-enable MD013 -->

Universal is the product baseline, not an error state. Lite, Balanced, and
Signature add expression without replacing the document, content, controls, or
legal information.

## Tier selection

The server emits Universal first. A capability bootstrap of at most 5 KiB Brotli
chooses a candidate tier before importing a heavy runtime or scene package.
Selection considers feature support, user motion preference, Save-Data, coarse
device and memory signals when available, visibility, and prior measured health.

Rules:

- Save-Data or missing required graphics support caps the page at Universal.
- Reduced-motion users receive a complete non-animated expression by default and
  may opt into additional motion deliberately.
- A heavy tier is never downloaded merely to discover that it cannot run.
- The runtime may downgrade after sustained frame-time pressure, context loss, or
  memory pressure. It does not oscillate tiers during normal interaction.
- A tier decision is observable in diagnostics and reproducible in tests.
- Every tier has an authored poster or static recovery state.

## Rendering and asset stack

PliegoRS owns composition, island lifecycle, tier selection, diagnostics, and
budgets. It does not create a new 3D engine or media codec without measured need.

- Three.js is the mature client renderer for 3D islands.
- Blender headless export supplies deterministic source-to-scene builds.
- glTF/GLB is the scene interchange format.
- Meshopt, quantization, authored LODs, and KTX2/Basis texture variants provide
  measured scene compression and GPU suitability.
- Responsive AVIF, WebP, and JPEG variants serve raster imagery according to
  browser support and measured quality.
- Browser-native, production-proven media codecs and Cloudflare delivery are
  preferred over custom decoders in the critical path.

Three.js and its scene package are dynamically imported only for a selected 3D
tier. PliegoRS may wrap them in `pliego-scene`; it does not fork their renderer.

## Runtime contract

A mounted 3D island uses:

- one active renderer and graphics context;
- render-on-demand when continuous animation is unnecessary;
- visibility pause and background-work cancellation;
- bounded dynamic device-pixel ratio;
- precomputed LOD and texture selection;
- explicit geometry, material, texture, listener, and renderer disposal;
- context-loss recovery to an authored poster;
- no Hyphae query, network request, asset decode, or unbounded allocation in the
  frame loop.

Hyphae can change scene or navigation state through prefetched snapshots and
background event deltas. The frame loop consumes resolved local state only, as
required by ADR-002.

## Performance contract v0

These are initial hard gates:

| Gate | Budget |
| --- | ---: |
| Universal first viewport | <= 350 KiB transferred |
| Capability bootstrap | <= 5 KiB Brotli |
| Universal 3D runtime transfer | 0 bytes |
| PliegoRS interactive island | <= 180 KiB Brotli; ratchet toward 120 KiB |
| Lite scene, deferred transfer | <= 900 KiB |
| Lite scene GPU estimate | <= 32 MiB |
| Lite scene frame p95 | <= 33 ms |
| PLIEGO target LCP | <= 1.8 s |
| PLIEGO target INP | <= 150 ms |
| PLIEGO target CLS | <= 0.05 |

Balanced and Signature require explicit per-Pliego transfer, GPU, draw-call,
triangle, decode, and frame-time budgets in the asset manifest before release.
They cannot ship with an unbounded "desktop only" exemption.

## Measurement and enforcement

The contract is measured on the permanent device matrix: modest Android,
iPad/Safari, integrated-GPU laptop, and capable desktop reference.

`pliego inspect` and CI will report:

- transfer and decoded sizes by asset and tier;
- startup, decode, compile, and main-thread work;
- estimated and observed GPU resources;
- draw calls, triangles, active contexts, and frame-time percentiles;
- LCP, INP, CLS, long tasks, and island bootstrap cost;
- resource growth across repeated mount, swap, and unmount cycles.

Browser tests and canvas-pixel checks must prove that each tier is nonblank,
correctly framed, and visually intentional on desktop and mobile viewports.
Physical-device evidence has priority over a desktop emulator.

Budgets ratchet downward as measurement improves. Raising a budget requires a
new ADR with physical-device evidence, alternatives considered, and explicit
approval. A visual difference alone is not a waiver.

## Current state and north star

<!-- markdownlint-disable MD013 -->

| Concern | Current fact on 2026-07-10 | Accepted north star |
| --- | --- | --- |
| Production sites | The official PliegoRS site is fully native. | Each maintained product passes the native migration gate. |
| 3D runtime | No production `pliego-scene`, adaptive selector, or certified scene package exists. | Three.js islands load only for selected tiers and dispose deterministically. |
| Asset pipeline | Blender and mature codecs are available, but deterministic manifests and budget enforcement are not implemented. | `pliego-assets` emits hashed variants, LODs, provenance, rights, and executable budgets. |
| Performance | The v0 numbers are accepted policy, not yet complete CI gates. | CI, RUM, browser tests, and physical-device reports enforce every released tier. |
| Hyphae | It is not connected to a production visual runtime. | It supplies bounded state outside the frame loop through ADR-002. |

<!-- markdownlint-enable MD013 -->

## Consequences

- Design begins with four expressions instead of producing a fallback after the
  Signature scene is complete.
- Asset and renderer work cannot land without manifests, disposal tests, and
  device evidence.
- Universal preserves SEO, accessibility, navigation, and core actions even when
  all optional runtimes fail.
- PliegoRS gains a distinctive adaptive contract while relying on proven visual
  infrastructure.

## Rejected alternatives

- **One identical 3D bundle for every device:** rejected for transfer, memory,
  accessibility, battery, and stability reasons.
- **Static-only works:** rejected because motion and 3D are valid parts of a
  Pliego when they have a complete Universal expression.
- **Build a custom Rust/WebGPU renderer now:** rejected because Three.js and
  mature codecs solve the immediate product need with lower execution risk.
- **Detect capability after downloading everything:** rejected because discarded
  bytes and decode work already violate the contract.

## References

- [PliegoRS founding specification](../00-pliegors-spec.md)
- [PliegoRS / Hyphae protocol](../01-hyphae-protocol.md)
- [Execution backlog](../19-product-execution-backlog.md)
- [Hardening roadmap](../28-hardening-roadmap.md)
- [ADR-002: Hyphae active plane](ADR-002-hyphae-active-plane.md)
