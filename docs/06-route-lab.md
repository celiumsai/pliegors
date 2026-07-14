# PLIEGO Route Lab 0.2

**Status:** Phase 1 run 1.1; current collector patch `pliego-route-lab/0.2.2`; acceptance review remains manual
**Raw run schema:** `schemas/pliego.measurement-run.schema.json`

## Purpose

Route Lab turns the physical measurement protocol into an executable browser
session without embedding its collector or control panel in the measured site.
Each target may keep a small permanent tier adapter because its
delivery policies are product behavior, not test-only telemetry. The lab is a
strict local reverse proxy: the target list comes from the measurement plan, so
it cannot become an arbitrary network proxy.

The dashboard binds one accepted device fingerprint to one canonical target,
route, tier, cache mode, motion mode, orientation, power state, thermal state,
and the device row's plan-locked network profile. The upstream HTML receives an
early same-origin probe. All other target resources stream through without
source-file changes.

Route Lab 0.2 does not perform network shaping, throttling, or mobile-network
emulation. A profile such as `lan-wifi` describes the real connection under
test; it is evidence metadata, not a transport configuration. A shaped profile
requires an external tool and separately reviewed evidence.

## Start

The three production previews must already be listening on the endpoints in
`fixtures/phase-1/measurement-plan.json`.

```powershell
node tools/route-lab/server.mjs --host 0.0.0.0 --port 5330
```

Open:

```text
http://<workstation-lan-ip>:5330/_pliego/
```

The process reserves one origin per measured target so cache, storage, cookies,
and Service Worker registrations cannot cross-contaminate:

| Port | Surface |
| --- | --- |
| 5330 | Route Lab dashboard |
| 5331 | PliegoRS site proxy |

Additional configured targets receive consecutive ports after 5331.

The dashboard creates an HTTP-only session cookie and redirects to the selected
canonical route. The fixed measurement panel survives full document navigation
through local session state and server-side raw segments.

## Recorded Evidence

- The first-viewport snapshot is frozen at window load plus two animation frames,
  before the measurement panel allows the interaction protocol to begin.
- First-viewport and complete-session transfer bytes are separate values.
- The first snapshot stores one navigation plus current resource entries, in
  chronological order, with a hard limit of 512 entries.
- Ledger entries preserve only pathname, entry type, target or external scope,
  initiator, transfer/encoded/decoded bytes, cache state, and duration. Query,
  fragment, host, and credentials are never persisted.
- Cache state is explicit: `network`, `local-cache`, `validated-cache`,
  `opaque`, or `unknown`. Opaque, unknown, and overflowed ledgers are violations.
- Cached response counts are stored for the first viewport and complete session;
  a requested cold run with any cached initial response is flagged.
- First-viewport transfer, encoded, decoded, resource, and cache totals are
  recalculated from the ledger and must reconcile exactly, including navigation.
- Complete-session totals still cover all resources in every document segment.
- `requestAnimationFrame` deltas cover the complete interaction session.
- LCP and CLS come from buffered browser observers when available.
- CLS uses the standard one-second-gap, five-second session-window algorithm.
  Unsupported Layout Shift observation remains `null`, never zero.
- INP candidates come from Event Timing interaction IDs when available.
- Acceptance decode p95 remains `null` unless the target exposes authored
  request-to-decode marks. The post-load `HTMLImageElement.decode()` diagnostic
  is stored separately and never substitutes for that metric.
- Acceptance main-thread p95 remains `null` unless the target exposes task
  slices or a reviewed trace supplies them. Long Task p95 is stored separately
  as a lower-resolution diagnostic.
- Estimated VRAM sums decoded image, video-frame, and canvas dimensions.
- Draw calls and triangles are zero only for a page with no canvas scene.
  A canvas without a scene hook remains `null`, never an invented zero.
- Vite HMR, `/@vite/client`, Astro's development toolbar, and equivalent
  development-only markers add `development-server-artifact`. Such a run cannot
  be accepted.
- Fixed viewports retain exact width and height tolerances. Native mobile
  viewports keep width, DPR, and orientation strict while allowing height to
  move only inside the accepted fingerprint's inner/visual-to-available range;
  this accounts for dynamic browser controls without accepting another device.

An integrated scene may expose exact renderer data:

```js
window.__PLIEGO_SCENE_METRICS__ = () => ({
  renderer: actualContextRenderer,
  drawCallSamples: perFrameDrawCalls,
  triangleSamples: perFrameTriangles,
  estimatedVramBytes: currentTextureBytes,
});
```

Draw calls and triangles are calculated as p95 over the hook's frame samples.
A single end-of-run renderer snapshot is not accepted as a substitute.

A target may expose acceptance timing arrays when it owns exact marks. Every
canonical fixture exposes its actually selected tier:

```js
window.__PLIEGO_ACTIVE_TIER__ = "lite";
window.__PLIEGO_PERFORMANCE_METRICS__ = () => ({
  activeTier: "lite",
  decodeDurations: requestToDecodedFrameDurations,
  mainThreadTaskDurations: reviewedTaskSlices,
});
```

An absent or mismatched active tier is a run violation. Merely selecting a tier
on the dashboard never proves that an unintegrated target honored it.

Before authored scripts execute, the proxy exposes the validated request as
`window.__PLIEGO_REQUESTED_TIER__` and `window.__PLIEGO_REQUESTED_MOTION__`.
These are inputs, not evidence. The target must apply them and independently
publish `window.__PLIEGO_ACTIVE_TIER__` after selecting its real behavior.

## Interaction Contract

The panel records the seven protocol states: ready, one-viewport scroll,
primary navigation, visual response, disclosure, ten-second scene hold, and
return to the initial route. A state can be complete, not applicable, or missed.
Missing applicable work and foreground loss become explicit violations.
Authored disclosure controls (`aria-expanded`, `summary`, or menu controls) and
other authored buttons automatically complete their matching interaction step;
clicks inside the Route Lab overlay never count as target behavior. After ten
seconds, scene hold becomes complete only when a canvas or scene hook exists,
and otherwise becomes not applicable. Scene hold is read-only because its state
is derived from observable target capabilities; other states retain a manual
not-applicable escape hatch for controls that genuinely do not exist.
After the ten-second minimum, `Finish run` remains locked until every state is
complete or not applicable. The panel lists unresolved states and explicitly
asks the operator to return to the canonical route when required. `Reject`
retains a deliberate bypass so failed evidence can still be sealed and audited.

A hidden document is confirmed with a short timeout. `pagehide` cancels that
pending check before a normal navigation segment is sealed, so navigating the
fixture does not impersonate a real background-tab interruption.

Each navigation seals a raw segment with `sendBeacon`. Finishing the run first
flushes every locally queued segment, then writes a schema-shaped record under
`measurements/runs/inbox/`,
and returns a SHA-256 receipt. Raw segments live under
`measurements/runs/raw/<session-id>/`.
Receipt creation closes that raw segment set. An exact retransmission of the
same final segment returns the existing receipt, while any new or changed
segment receives `409 session-finalized` and is not persisted.

## Acceptance Boundary

Route Lab produces a candidate, not an accepted report. Review still must:

1. confirm the selected physical device, browser, and production preview/build;
2. verify cold or warm cache behavior from the recorded transfers;
3. confirm the target actually honored the requested performance tier;
4. attach the required screenshot and browser trace;
5. reject interrupted, hidden, throttled, or power-state-changing runs;
6. move reviewed runs into `measurements/runs/accepted/`;
7. aggregate five accepted runs into a `1.1.0` measurement report.

Each report points to a directory containing exactly its five accepted raw-run
JSON files. Screenshot and trace evidence are immutable files or archives whose
SHA-256 values are stored in the report. The closure audit reloads the five raw
runs, verifies their receipts and shared case identity, independently validates
every first-route ledger, and recalculates every aggregate observation.

The proxy probe is excluded from Resource Timing totals by its reserved
`/_pliego/` path. Its CPU overhead is not zero. Final framework performance work
will move the same observer contract into the native PliegoRS test build so the
proxy can be removed from release measurements.

The current proxy is designed for reviewed PLIEGO-owned builds, not hostile
pages. Probe and target share an origin, so authored JavaScript could call the
collector endpoints or alter browser APIs. Server validation, an HTTP-only
session, source fingerprints, raw ledgers, and the signed receipt make accidental
mixups and post-capture edits visible; they do not create an isolated execution
world. Adversarial benchmark integrity requires a browser extension or DevTools
isolated world and remains outside Route Lab 0.2.

Route Lab 0.2 measures public fixture routes. It does not certify authenticated
flows: cookies are namespaced per target, but `Secure`, `__Secure-`, and
`__Host-` cookies cannot preserve their production semantics on the HTTP LAN
origins used for physical-device collection.

Cold mode sends `Clear-Site-Data: "cache"` and marks the request in the run.
Browser support differs, especially on non-secure LAN origins. Route Lab counts
both zero-transfer cache hits and header-only revalidations backed by a decoded
cached body. A cold run with no observed initial transfer or any cached initial
response is automatically flagged and cannot be accepted.

Every cold capture therefore starts in a new private browsing session, or after
an explicit site-data reset performed before the dashboard opens. Cold mode
also forces proxied responses to `no-store`; it must not be used to warm the
cache for a later run. Warm measurements use their own fresh private session:
one excluded warm-mode warm-up followed by the five measured warm runs without
resetting storage or interleaving cold captures.

Route Lab hashes the exact upstream HTML response that is actually proxied, with
the run's UA and cookies, and combines that digest with the target's
asset-manifest hash. The accepted device
fingerprint is also bound by file hash, rather than by filename alone.

The proxy preserves production content encoding for resources and re-encodes
only the instrumented HTML with its original gzip, Brotli, deflate, or Zstandard
encoding. Instrumented HTML is always `no-store`. In warm mode target assets
retain their upstream cache behavior; in cold mode every proxied response is
forced to `no-store` after the pre-run reset. For a strict header CSP it adds a
per-session nonce solely to the injected probe. Target `Origin`, `Referer`, and
cookies are forwarded against the real upstream. Routes controlled by a Service
Worker are flagged and cannot be accepted.

After `Finish run`, the measured page must remain foregrounded until the overlay
shows `Candidate saved / <file>`. The final segment and receipt are part of the
evidence; leaving the page or switching applications before that message makes
the capture incomplete.
