# PLIEGO physical measurement protocol 1.0

**Status:** Phase 1 protocol; physical acceptance runs pending
**Plan:** `fixtures/phase-1/measurement-plan.json`
**Raw run schema:** `schemas/pliego.measurement-run.schema.json`
**Aggregate report schema:** `schemas/pliego.measurement-report.schema.json`

## Evidence rule

Emulation is useful during development but cannot close the device gate. Every
required row needs a physical device fingerprint, browser build, viewport, DPR,
GPU renderer, raw runs, trace, and screenshot. A result without those artifacts
is a note, not acceptance evidence.

Local artifact staging, hashing, and report metadata follow
`docs/07-evidence-bundles.md`. Bundle creation is deliberately separate from
publishing reviewed runs into `measurements/runs/accepted/`.

The public repository carries no personal device fingerprints. Every reference
row remains pending until an anonymized evidence bundle is reviewed and
published independently. No performance number is inferred from a model name or
from browser emulation.

## Matrix

| Required role | Primary tier evidence | Current state |
| --- | --- | --- |
| Modest physical Android | Universal and Lite | Fingerprint pending |
| Physical iPad with Safari | Through Balanced | Fingerprint pending |
| Integrated-GPU laptop | Through Balanced | Fingerprint pending |
| Capable physical desktop | Through Signature | Fingerprint pending |

## Device Lab

Start the local physical fingerprint collector on the LAN:

```powershell
node tools/device-lab/server.mjs --host 0.0.0.0 --port 5310
```

Open `http://<workstation-lan-ip>:5310` on the physical device, choose its
acceptance row, and run the five-second probe without leaving the tab. The lab
stores unreviewed captures under `measurements/inbox/`. Review a capture before
moving it to `measurements/accepted/` or updating a matrix fingerprint.
Captures made with the wrong browser move to `measurements/rejected/` with a
written reason; they never satisfy the named acceptance row.

The Device Lab's `frameProbe` measures the interval between consecutive
`requestAnimationFrame` callbacks with no authored scene workload. About 17 ms,
9 ms, and 6 ms correspond roughly to 60 Hz, 120 Hz, and 165 Hz scheduling. A
smaller interval does not prove greater browser or GPU efficiency. Efficiency
requires the same rendered workload plus CPU/GPU time, energy, and dropped-frame
evidence normalized for refresh rate.

## Route Lab

Start the instrumented route proxy on the LAN:

```powershell
node tools/route-lab/server.mjs --host 0.0.0.0 --port 5330
```

Open `http://<workstation-lan-ip>:5330/_pliego/`, bind an accepted fingerprint,
target, route, tier, cache mode, motion mode, orientation, power state, thermal
state, and network profile, then open the route. Route Lab injects its probe into
the proxied HTML before authored scripts, persists the interaction state across
full document navigations, and stores raw segments plus a hashed candidate run.

Starting at port 5331, Route Lab assigns one dedicated origin to every target in
the measurement plan. Separate origins prevent cache, storage, and Service
Worker state from one target entering another target's run.

Route Lab 0.2 does not shape, throttle, or emulate the network. The
`networkProfile` value is locked to the selected device row and records the real
connection used for that run. Current Android and iPad LAN measurements use
`lan-wifi`; claiming a shaped mobile profile requires an external shaper and
separate evidence outside Route Lab 0.2.

Acceptance runs must use the production build or preview command for every
fixture. A dev server is not equivalent evidence: Vite HMR, `/@vite/client`, an
Astro development toolbar, or another unambiguous development marker adds the
`development-server-artifact` violation and disqualifies the run.

A candidate remains unaccepted until its raw ledger is reviewed and the required
trace and screenshot are attached. Each fixture now exposes its active delivery
tier; Route Lab rejects an absent or mismatched confirmation. Cold mode requests
browser cache clearing; review must still confirm that initial-route transfer
was observed and that no initial response came from cache. See
`docs/06-route-lab.md` for exact boundaries.

## Run preparation

1. Build the exact source revision and record its commit or source-archive hash.
2. Start the three fixture servers from their production builds.
3. Record device, OS, browser, viewport, DPR, renderer, power, and network
   state.
4. Begin at nominal temperature. Discard a throttled or interrupted run.
5. Before every cold run, close all private/incognito windows and start a new
   private session, or explicitly clear the target origin's site data before
   opening Route Lab. `Clear-Site-Data` on an HTTP LAN origin is not sufficient
   evidence by itself.
6. Capture five cold-cache runs for each route/tier case, resetting storage
   before each one. A cold run with any cached first-viewport response is
   rejected by the ledger.
7. Measure warm cache in a separate fresh private session. Launch one warm-mode
   warm-up, reject it as `protocol warm-up excluded`, then capture the five
   measured warm runs without clearing storage or interleaving cold runs.
8. Repeat the route's canonical interaction script for INP and frame timing.
9. Run reduced motion separately; never mix those samples into default motion.
10. Keep the measured page foregrounded after `Finish run` until Route Lab
    displays `Candidate saved / <file>`. Switching applications before that
    receipt disqualifies the run.

## Canonical interaction window

Each route receives the same bounded sequence where the control exists:

1. load and wait for the declared ready mark;
2. scroll one viewport;
3. activate the primary navigation transition;
4. exercise one pointer or touch-driven visual response;
5. open and close one menu or disclosure;
6. hold the animated scene for ten seconds;
7. return to the initial route state.

Missing interactions are recorded as not applicable. Automation must not invent
hidden controls merely to fill the sequence.

## Metrics

| Metric | Source | Aggregate |
| --- | --- | --- |
| First-viewport transfer bytes | Immutable load + two-rAF resource ledger | Median |
| Complete-session transfer bytes | Navigation and Resource Timing | Ledger only |
| Decode latency | Image decode and request-to-first-frame marks | p95 |
| Main-thread work | Performance trace task slices | p95 |
| Estimated VRAM | Renderer data and decoded dimensions | Exact peak |
| Draw calls / triangles | `renderer.info.render` | p95 |
| Frame time | `requestAnimationFrame` deltas | p95 |
| LCP / INP / CLS | `web-vitals` observer during scripted session | p75 |

Asset estimates are checked before the run. Browser evidence replaces the
estimate whenever the two disagree and the discrepancy is filed for correction.

Measurement run 1.1 preserves the first-viewport ledger at window load plus two
animation frames, before Route Lab exposes its interaction panel. The ledger
contains the navigation and current resources, excludes `/_pliego/`, stores only
pathnames, and reconciles transfer, encoded, decoded, resource, and cache totals
exactly. Complete-session totals remain separate and include every measured
document segment. Overflow, opaque cross-origin timing, or an unknown cache
state disqualifies the run instead of being interpreted as zero cost.

## Noise and rejection

A run is rejected when the browser loses foreground visibility, the OS changes
power mode, the network profile changes, a service worker from another revision
controls the page, a renderer context is lost, or thermal throttling begins.
Rejected runs remain in the raw ledger with their reason and are not included in
the five accepted samples. A normal same-origin navigation does not count as
leaving the foreground; Route Lab confirms a hidden document with a short delay
and cancels that pending flag when `pagehide` seals a navigation segment.

## Ratchet

The committed contract begins with the Phase 0 budgets. A budget may become
stricter after two consecutive accepted baselines show headroom. Raising a
budget requires measurements, a short ADR, and explicit approval. A faster
reference device can never justify weakening the modest-device limit.

## Current closure condition

Phase 1 remains open until all four required physical rows have validated
reports for the canonical routes. The deterministic asset baseline and
inspector are complete inputs to that work; they are not a substitute for it.
