# DOM lifecycle contract

**Status:** R4 accepted (`pliego-dom` 0.3.0-beta.1, adapter API v1 preview)

PliegoRS renders one validated `View` tree through two concrete paths: bounded
HTML serialization and surgical browser DOM mounting. Both paths consume the
same tag, attribute, parser-context, keyed, and resource-limit contracts. The
browser path adds explicit ownership: a `MountedRoot` contains one non-cloneable
`MountScope` that owns its nodes, reactive effects, listeners, cleanup callbacks,
dynamic children, keyed rows, and external adapter cancellation boundary.

This document is the normative R4 contract. The implementation is in
[`pliego-dom`](../crates/pliego-dom/src/lib.rs), with the browser lifecycle in
[`mount.rs`](../crates/pliego-dom/src/mount.rs) and strict SSR adoption in
[`adopt.rs`](../crates/pliego-dom/src/mount/adopt.rs).

## Core invariants

1. A mounted resource has exactly one lifecycle owner.
2. `MountScope::dispose` is idempotent and makes the scope unusable immediately.
3. Effects, listeners, registered cleanup, and adapter cancellation begin before
   owned DOM is removed.
4. Cleanup uses frozen node identities. Foreign DOM inserted inside PliegoRS
   boundaries is never inferred to be owned.
5. A topology mismatch is diagnostic. Dynamic or keyed slots become terminal
   when continuing could remove or overwrite foreign DOM.
6. User callbacks never run while the reactive runtime has a global mutable
   borrow.
7. No maintained WASM surface uses `Closure::forget()` or `mem::forget()` to
   simulate ownership.

## Safe view construction

`try_el`, `Element::try_attr`, `try_attr_dyn`, `try_child`, and `try_on` are the
fallible construction boundary. Their infallible counterparts panic only for
programmer-authored invalid constants. Runtime-controlled names and values must
use the `try_` APIs and propagate `DomError`.

The shared validators enforce:

- inert HTML, SVG, custom-element, `data-*`, and `aria-*` names;
- exact qualified SVG attribute namespaces;
- no inline event-handler attributes;
- no active document elements, unsafe URL schemes, or `srcdoc`;
- valid void-element and parser-sensitive parent/child topology;
- no parser-normalized text that would make server and browser trees differ.

The serializer and browser mount use the same parser context, including HTML
integration points inside SVG.

## Mount ownership

```rust
use pliego_dom::{el, mount};

let view = el("button")
    .attr("type", "button")
    .on("click", |_| {})
    .child("Run")
    .into_view();

let root = mount(&view, host.as_ref())?;
root.scope().on_cleanup(|| release_project_resource())?;

// Also runs automatically when `root` is dropped.
root.dispose();
root.dispose(); // no-op
# Ok::<(), pliego_dom::MountError>(())
```

The disposal sequence is:

1. mark the scope disposed;
2. dispatch `pliego:scope-dispose` from each owned top-level element while it is
   still connected;
3. dispose reactive descendants, listeners, and `on_cleanup` callbacks in LIFO
   order;
4. remove the exact owned range and separately registered node identities;
5. run a bounded drain for custom-element reactions that reinsert owned nodes;
6. retain and report any survivor that cannot be removed safely.

`MountError::CleanupDidNotConverge` is terminal evidence, not a silent leak.
`MountedRoot::last_error` and `take_error` expose asynchronous patch and cleanup
diagnostics without allowing a poisoned slot to resume.

## Dynamic views

`dyn_text` patches one retained text node. Dynamic attributes patch the exact
validated attribute. `dyn_view` owns a marker-bounded child `MountScope` and
stages a replacement before retiring the stable child.

A failed stage leaves the prior view stable. Once old-DOM retirement starts, a
later topology failure poisons the slot instead of claiming rollback. Self-
disposal from an effect or listener is deferred to the reactive safe boundary,
so no callback writes after its owner is disposed.

## Keyed reconciliation

```rust
use pliego_dom::{el, keyed};
use pliego_reactive::Signal;

let rows = Signal::new(vec![(1_u64, "Alpha"), (2, "Beta")]);
let list = keyed(
    move || rows.get(),
    |(id, _)| *id,
    |(id, label)| el("li").attr("data-id", id.to_string()).child(label).into_view(),
);
```

Keys are typed: signed, unsigned, and textual domains are distinct. Text keys
are non-empty and bounded. One update accepts at most `65,536` rows, `256` UTF-8
bytes per text key, and `8 MiB` of aggregate key material. Duplicate and
oversized keys fail before any new row builder executes.

Retained keys keep the same row `MountScope`, DOM node identities, listeners,
and focused control. New builders run once per key lifetime. Reordering uses an
`O(n log n)` longest-increasing-subsequence plan to minimize moves. Foreign DOM
inside a keyed gap blocks the commit and survives cleanup.

The row builder receives the value captured when a key is first created. Put
mutable row state in signals captured by the returned view; changing non-reactive
data while retaining the same key does not rebuild that row.

## Versioned SSR adoption

Plain HTML output remains unchanged:

```rust
let html = pliego_dom::try_render_html(&view, limits)?;
```

To reuse server-authored nodes, render the versioned seed and adopt that exact
tree in the browser:

```rust
// Native/server.
let html = pliego_dom::try_render_adoptable_html(&view, limits)?;

// wasm32/browser, after placing `html` in a dedicated host.
let root = pliego_dom::adopt_with_limits(&view, host.as_ref(), limits)?;
```

Adoptable output is delimited by `pliego:ssr:v1` comments and internal inert
boundaries. Those markers are a versioned framework protocol, not styling or
application hooks.

Adoption has two phases:

1. **Preflight:** no mutation. It verifies root version, complete DOM shape,
   element tag and namespace, exact attribute names/namespaces/values, text,
   keyed identities, dynamic seeds, and traversal budgets.
2. **Commit:** it attaches owners, listeners, and effects to the verified node
   identities. Any commit or final range failure disposes installed resources
   and removes the complete failed seed with a bounded rollback.

`MountError::AdoptionMismatch` contains bounded `path`, `expected`, and `actual`
diagnostics. A preflight mismatch leaves the host byte-for-byte and
identity-for-identity untouched.

SSR and browser callbacks must be deterministic for the first read. If a
dynamic value, keyed sequence, or produced subtree differs between serialization
and adoption, adoption fails closed. Static text is supported under `textarea`
and `title`; dynamic text, dynamic views, and keyed markers are rejected in
those RCDATA contexts because comments would become authored text.

The host is dedicated: the versioned start and end markers must cover all its
children. Arbitrary foreign siblings are not silently absorbed.

## External adapters

`MountScope` and adapter API v1 are connected by a synchronous capture-phase
event. When a Rust-owned range is disposed, `pliego:scope-dispose` reaches
`runtime-v1.js` before DOM removal. The runtime then:

- aborts the adapter `AbortSignal` immediately;
- executes already registered cleanup without waiting for a pending `mount` or
  `update` promise;
- invokes the plugin unmount hook and returned cleanup at most once;
- rejects late completion from an obsolete generation;
- re-evaluates Save-Data and reduced-motion policy on live changes;
- cancels scheduled visibility, idle, and interaction triggers;
- includes detached known roots in `pagehide` cleanup.

Plugins must still observe `context.signal`. JavaScript cannot forcibly cancel
an arbitrary promise or synchronous library call, but a non-cooperative call can
no longer postpone cleanup that was already registered with the lifecycle.

See [External adapter contract](12-external-adapters.md) for GSAP, Lenis,
Three.js, WebGL, tier, and lazy-loading recipes.

## Verification gates

```text
cargo test -p pliego-dom --lib --locked
cargo clippy -p pliego-dom --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
powershell -File scripts/test-browser-wasm.ps1 -ChromeDriver <matching-driver>
npm run test:adapters
npm run test:adapters:browser
npm run check:wasm-lifetimes
```

The Chromium matrix includes 10,000 mount/dispose cycles, exact listener
removal, custom-element reinsertion attacks, keyed identity/focus retention,
strict SSR adoption and rollback, lifecycle event ordering, and real adapter DOM
removal. The committed results are in
[R4 DOM lifecycle evidence](evidence/r4-dom-lifecycle.md).

## Deliberate boundaries

- Streaming server rendering and server functions are not implemented.
- Adoption reuses a complete versioned seed; it is not partial or heuristic
  hydration of arbitrary HTML.
- External libraries remain native JavaScript behind adapter API v1. PliegoRS
  does not reimplement GSAP, Lenis, Three.js, WebGL, or browser media APIs.
- Browser DOM mutation by code outside the owning scope is allowed only when it
  does not impersonate PliegoRS-owned identities or violate boundary topology.
