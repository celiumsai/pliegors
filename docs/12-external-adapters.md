# External adapter contract

**Status:** API v1 preview (`pliego-adapters` 0.3.0-beta.1, runtime `1.1.0`)

PliegoRS renders useful static markup before external browser code is loaded.
An adapter owns enhancement below one `<pliego-adapter>` root. The framework
owns admission, lazy loading, cancellation, lifecycle events, error isolation,
and cleanup sequencing. JavaScript libraries keep their native APIs.

## Compatibility promise

`data-pliego-api="1"`, `context.apiVersion === 1`, and a plugin export with
`apiVersion: 1` form the public compatibility boundary. Additions to the v1
context are backward compatible. Removing fields, changing lifecycle order, or
changing an existing field's meaning requires API v2 and a new runtime asset.

The original mount-only module remains supported:

```javascript
export function mount(root, props, { signal }) {
  signal.addEventListener('abort', stopWork, { once: true });
  return () => releaseResources();
}
```

When a legacy plugin has no `update`, `PliegoAdapters.update()` safely unmounts
and remounts it. Existing `AdapterIsland::new`, `trigger`, `prop`, `child`, and
`into_view` call sites remain source compatible.

## V1 plugin module

The preferred module exports `pliegoAdapter` (a default object is also valid):

```javascript
export const pliegoAdapter = {
  apiVersion: 1,

  async mount(root, props, context) {
    const observer = new IntersectionObserver(() => {});
    context.onCleanup(() => observer.disconnect());

    // Returning a cleanup function remains supported.
    return () => releaseLibraryState();
  },

  async update(root, nextProps, context) {
    // Patch owned state without replacing the island root.
  },

  unmount(root, context) {
    // Optional explicit lifecycle hook.
  },
};
```

`context` is frozen and contains:

| Field | Meaning |
|---|---|
| `apiVersion` | Integer contract version (`1`). |
| `runtimeVersion` | Runtime implementation version for diagnostics. |
| `signal` | Aborted before every unmount, failed mount, or abandoned import. |
| `tier` | `universal`, `lite`, `balanced`, or `signature`. |
| `motion` | `full` or `reduced`, resolved before import. |
| `saveData` | Current Network Information Save-Data signal. |
| `capabilities` | Frozen declared capability list. |
| `onCleanup(fn)` | Register cleanup immediately; callbacks execute LIFO. |

Cleanup order is deterministic: abort signal, plugin `unmount`, cleanup returned
by `mount`, then `onCleanup` callbacks in reverse registration order. Each
callback has its own error boundary, so one failure never prevents later
cleanup. Promise-returning callbacks are awaited in that order. `unmount()`
keeps its synchronous boolean return for v1 compatibility, while any later
mount waits on the complete teardown barrier.

The runtime creates a provisional lifecycle before invoking plugin `mount`.
Unmount therefore aborts and executes cleanup already registered through
`onCleanup` without waiting for a pending `mount` or `update` promise. A cleanup
returned later by an obsolete mount is executed once; that generation cannot
become mounted again.

Updates are serialized in invocation order. An update received while a module
is still importing waits for mount and is then applied. Unmount invalidates the
queue, so a late resolve or rejection cannot revive a disposed island or change
its terminal status.

## Rust declaration

```rust
use pliego_adapters::{
    AdapterCapability, AdapterIsland, DataPolicy, LoadTrigger, MotionPolicy,
    PerformanceTier,
};

let scene = AdapterIsland::new(
    "product-scene",
    "/assets/product-scene.2047c812716c6789.js",
)?
.trigger(LoadTrigger::Interaction)
.min_tier(PerformanceTier::Balanced)
.capability(AdapterCapability::WebGl)
.capability(AdapterCapability::HighFrequencyRaf)
.motion_policy(MotionPolicy::Auto)
.data_policy(DataPolicy::SkipOnSaveData)
.prop("model", "/assets/product.4bc31a.glb")?
.child(static_fallback)
.into_view()?;
```

Props must be a JSON object and are capped at 32,768 serialized UTF-8 bytes in
both Rust emission and the browser runtime. Adapter
identifiers and prop keys are ASCII identifiers of at most 128 bytes. Module
paths must be same-origin `/assets/*.js` paths with no traversal, encoding,
query, fragment, control character, or non-ASCII ambiguity. The runtime repeats
this validation after DOM parsing to reject client-side attribute tampering.

## Lazy triggers

| Trigger | Import point | Automatic cancellation |
|---|---|---|
| `Immediate` | As soon as the runtime scans the island. | Yes. |
| `Visible` | First viewport intersection; immediate fallback without IO. | Observer is removed. |
| `Idle` | Idle callback with a 2 s ceiling; timer fallback. | Idle/timer handle is cancelled. |
| `Interaction` | First pointer, keyboard, focus, or touch intent. | All intent listeners are removed. |

The runtime scans initial markup and nodes added later by `MutationObserver`.
Removing a root aborts pending imports and disposes mounted state. A microtask
connectivity check prevents a DOM move from being mistaken for deletion.
`pagehide` disposes every known island, including a root already detached from
the current document query; a persisted `pageshow` scans again for bfcache.

## Admission policy

The effective tier comes from `data-pliego-tier` on the island, then the root
document, then `balanced`. `min_tier` and declared capabilities are both
enforced before dynamic import:

| Minimum tier | Capabilities |
|---|---|
| `universal` | DOM, motion |
| `lite` | smooth scroll, audio, video |
| `balanced` | WebGL, high-frequency RAF |
| `signature` | WebGPU |

`MotionPolicy::Auto` passes `reduced` when the media query requests it.
`Reduce` always passes `reduced`, `Full` is an authored override, and
`SkipWhenReduced` keeps the static fallback without importing the plugin.
`DataPolicy::SkipOnSaveData` likewise preserves the fallback and avoids import;
`Auto` and `Allow` pass the signal through the context.

`assets/pliego-policy.js` is the framework-owned pre-loader bootstrap for
first-party documents. It resolves authored tier, Save-Data, and motion before
`assets/pliego-adapters.js` can import an ecosystem bundle. Once installed, the
runtime listens for reduced-motion and Network Information changes and refreshes
known islands through the same teardown barrier. Projects must emit the policy
asset before the lifecycle loader; setting a tier from a later WASM bootstrap is
too late to prevent a disallowed download.

Capabilities are resource-admission declarations, not a JavaScript security
sandbox. External modules still execute with page privileges. Production sites
must use content-addressed local modules plus a restrictive CSP; untrusted
third-party code does not belong inside this boundary.

## Runtime control and events

The loader installs a non-writable `globalThis.PliegoAdapters` object:

```javascript
await PliegoAdapters.mount(root);
await PliegoAdapters.update(root, { color: 'carbon' });
PliegoAdapters.unmount(root, 'route-transition');
PliegoAdapters.refresh(root); // re-evaluate policy and lazy trigger
PliegoAdapters.scan(fragment); // discover dynamically inserted islands
```

An island can also dispatch `pliego:adapter-update` with
`detail: { props }`. Observable events bubble from the island:

- `pliego:mount`, `pliego:update`, `pliego:unmount`
- `pliego:skip` with the admission reason
- `pliego:error` with the failed phase and message
- `pliego:cleanup-error` for an isolated cleanup failure

The current state is inspectable as `data-pliego-status`: `scheduled`,
`loading`, `mounted`, `skipped`, `error`, or `disposed`.

Rust-owned DOM does not require application glue for teardown. `MountScope`
dispatches `pliego:scope-dispose` while its top-level elements are still
connected. The runtime listens in capture phase, finds adapter descendants, and
aborts them before PliegoRS removes the owned range.

## Library recipes

### GSAP

Create one `gsap.context` scoped to `root`. Revert it in `onCleanup`; kill any
ScrollTrigger or standalone timeline not owned by the context. Use
`context.motion === 'reduced'` to remove parallax, scrub, and large transforms
while keeping essential state changes.

### Lenis

Create at most one instance per document-level island. Own exactly one ticker
or RAF subscription, stop it when `context.signal` aborts, then remove the
subscription and call `destroy()` from cleanup. Do not intercept native scroll
in `universal` or reduced-motion mode.

### Three.js and WebGL

Use an interaction/visible trigger, declare `WebGl` and
`HighFrequencyRaf`, and retain the authored image or markup fallback. Cleanup
must cancel RAF, disconnect resize observers, remove controls/listeners, dispose
render targets, geometries, materials, textures, loaders, and renderer, then
release or intentionally lose the GL context. Use `context.saveData` and tier to
select texture/model variants rather than detecting device brands.

## Build pipeline

`EsbuildBundler` invokes the pinned workspace esbuild installation with ESM,
browser, ES2020, minification, and no extracted legal-comment side file. Output
bytes determine the content-addressed filename and SHA-256 used by the Pliego
build manifest. `policy_bootstrap_bytes()` and `loader_bytes()` emit the
framework-owned policy and v1 lifecycle runtimes.

## Verification

```text
cargo test -p pliego-adapters --locked
node --test crates/pliego-adapters/tests/runtime-v1.test.mjs
node scripts/test-adapters-browser.mjs --chromedriver <matching-driver>
npm run check:wasm-lifetimes
cargo clippy -p pliego-adapters --all-targets --locked -- -D warnings
```

The adversarial runtime suite covers lifecycle order, legacy remounts,
Save-Data/reduced-motion/tier denial before import, live policy changes, mutated
paths and versions, failure isolation, cleanup exceptions, interaction loading,
overlapping mount generations, ordered updates, updates during import,
rejections after unmount, asynchronous teardown barriers, and removal during an
unresolved dynamic import. The Chromium gate covers scope-event propagation,
never-settling updates, and real MutationObserver removal. First-party
source-contract tests require every
maintained adapter to declare API v1 and emit policy before the loader.
