<!-- SPDX-License-Identifier: Apache-2.0 -->

# PliegoRS — founding spec

> The UI is a **fold** of an append-only, verifiable event log. The target is one
> Rust fold contract across client and database, with Hyphae as the reference
> durable engine.

Status: founding (2026-07-10). Method: Kaizen — we studied Leptos's architecture
in depth to *learn* how surgical fine-grained reactivity is built; we take the
lessons, not the dependency. This doc records what we learned, what we reuse
conceptually, and what we build that nobody has.

**The north star (fixed by Mario, 2026-07-10):** PliegoRS is a **general-purpose
web framework** — at maturity it builds *everything* Astro/Next/Vite build today,
content sites included, closing the Celiums ecosystem: Hyphae underneath, PliegoRS
on top. The experience bar the whole roadmap serves: *"loads in microseconds,
powerful, secure, beautiful."* Its unfair advantage remains data-alive apps (the
fold, provenance, memory), but the destination is the full web.

**Current implementation boundary:** PliegoRS does not yet run as one machine or
one log with Hyphae. M5 sends event content from a client hash chain to Hyphae,
which appends it to a separate durable hash chain and returns a different hash.
There is no shared reducer package, authentication, idempotency key, pull/merge,
or field-level provenance yet. Those are protocol and product milestones, not
properties of the current spike.

---

## 1. The model

<!-- markdownlint-disable MD013 -->

```text
                    ┌──────────────────────────────┐
   append(event) ──►│  LOG (append-only, hashed)   │   ← the ONLY write
                    └──────────────┬───────────────┘
                                   │ fan-out
              ┌────────────────────┼────────────────────┐
              ▼                    ▼                     ▼
        Fold<Tasks>          Fold<Filters>         Fold<Session>     ← projections
              │                    │                     │             (cached, incremental)
              ▼                    ▼                     ▼
        RenderEffect         RenderEffect          RenderEffect     ← surgical DOM patch
```

<!-- markdownlint-enable MD013 -->

- **One writable root**: the log. Interactions don't mutate state — they
  `append(event)`. The event is the unit of truth, hash-chained like Hyphae's
  journal.
- **State is derived**: every piece of state is a `Fold` — a pure reducer run over
  the log. Nothing else is writable.
- **The screen is the leaf**: render effects subscribe to folds and patch the
  exact DOM node that changed. No virtual DOM, no component re-runs.

This mirrors Hyphae's architecture (journal → projections) in the client. The
current crates implement the client-side log and fold independently; sharing a
versioned event schema and reducer package with the engine is planned work.

History, replay, and cursor-based time travel work in the local spike because its
log is retained. Durable audit and provenance require the unfinished client ↔
Hyphae protocol plus value/field-level lineage; they do not fall out of the
current implementation automatically.

## 2. What we learned from Leptos (and keep, conceptually)

Studied: `reactive_graph` (the core), tachys (renderer), the ownership model,
SSR modes, islands, the Cloudflare Workers path. Verdict: ~80% of the machinery
is *conceptually reusable*; the missing 20% is precisely our thesis.

**Keep (re-implement on the same principles):**

1. **Runtime dependency tracking.** A thread-local `Observer`; reading a reactive
   value inside a tracked scope registers bidirectional edges (source ↔
   subscriber, weak refs). Dependencies are dynamic: a conditional branch not
   taken is not a dependency.
2. **Two-phase graph coloring (push-pull, à la Reactively).** On write: mark the
   source **dirty (red)** and all descendants **check (green)** — cheap, no
   recompute. On read/effect-run: walk **up** (`update_if_necessary`), recompute
   only what's actually stale, and **equality-gate**: a node that recomputes to
   the same value never wakes its subscribers. Glitch-free diamonds; effects run
   at most once per wave.
   - This matters *more* for us than for Leptos: in our topology **every append
     dirties the whole forest** — lazy pull + equality gating is what keeps an
     append from recomputing the world.
3. **VDOM-free surgical rendering** (tachys's `Render { build / rebuild }` with
   retained `State`): components run once, build real DOM, and leaf effects patch
   the exact node. The renderer neither knows nor cares that values come from
   folds. The current M4 renderer proves HTML output, DOM mounting, and dynamic
   segments; retained keyed reconciliation and hydration remain to be built.
4. **Ownership/disposal tree** with `on_cleanup`: folds and their render effects
   are owned, cancelled and dropped when their UI decision-point re-runs.
5. **Layer separation.** Leptos proved the reactive graph, the renderer, and the
   scheduler can be independent crates. We adopt that decomposition from day one.

**Leptos's honest costs we design around:** WASM bundle/startup (mitigate with an
islands-style mode later; apps amortize it), disposed-signal panics (our handles
are typed against the owner; `try_` variants everywhere), hydration mismatches
   (deterministic folds remove one source of mismatch, but hydration still needs
   an explicit DOM adoption algorithm, serialized prefix contract, and tests).

## 3. The PliegoRS thesis and its implementation state

1. **The incremental fold node (implemented, M3).** Leptos's `Memo` is the
   closest primitive to a materialized fold (derived + cached, equality-gated,
   and lazy), but when dirty it
   **re-runs its closure from scratch**. Our `Fold` extends the memo with an
   **accumulator + a cursor into the log**: on wake it folds **only the tail**
   (`state = reduce(state, events[cursor..])`), then advances the cursor. Reducer
   work is O(new events); cloning and equality checks can still scale with state
   size in the current implementation. Snapshots (accumulator + cursor) make the
   reducer consume only the tail on cold start.
2. **The write discipline (application contract).** Domain interactions append
   events rather than mutate projected state. Reactive signals still expose
   setters because the runtime and UI need writable coordination state; a future
   public application API should make the log discipline harder to bypass.
3. **Pure, deterministic reducers as a contract.** Replay must reproduce state
   bit-for-bit. Tests can validate this property, but Rust's type system does not
   currently enforce reducer purity.
4. **Two tiers of "effect", separated explicitly.** A click handler is not an
   effect — it's an `append`. The only terminal effects are render effects (DOM)
   and declared side-effect ports. The loop: interaction → append → log red →
   folds green → lazy pull → surgical rebuild.
5. **Provenance as a UI primitive (planned).** M5 can display the local creation
   event and the durable acknowledgement hash. A future provenance model must
   track which event range produced each value or field and preserve source
   metadata through the durable fold.
6. **The Hyphae seam (experimental, M5).** The client can push `kind` and
   `payload` to Hyphae and receive its durable sequence and hash. Client and
   server hashes intentionally differ today because they are separate chains.
   A versioned envelope, shared reducers, tenant authentication, client event
   IDs, idempotent batches, pull cursors, persistence, merge, and conflict rules
   are required before this can be called one log or local-first.

## 4. Crate layout

```text
pliegors/
├─ crates/
│  ├─ pliego-log        the append-only, hash-chained client log (event model,
│  │                    cursors, snapshots; not yet wire-identical to Hyphae)
│  ├─ pliego-reactive   the reactive graph: observer tracking, two-phase coloring,
│  │                    equality gating, ownership/disposal  (lessons from
│  │                    reactive_graph, our implementation)
│  ├─ pliego-fold       the incremental fold node: accumulator + cursor + snapshot
│  │                    (THE new primitive; shared semantics with hyphae projections)
│  ├─ pliego-dom        experimental HTML/DOM renderer with dynamic segments
│  ├─ pliego-ssg        deterministic pages, head/SEO, routes, assets, manifests
│  └─ pliego-hyphae     experimental one-way HTTP append seam
├─ planned/
│  ├─ pliego-macros     view! RSX + #[component]
│  ├─ pliego            umbrella crate and user-facing framework surface
│  └─ pliego-cli        create/dev/build/test/preview/deploy workflow
└─ examples/
   └─ spike/            M1: one component = fold of a real Hyphae log (see §6)
```

## 5. Targets

- **Client:** `wasm32-unknown-unknown` via wasm-bindgen. Size-oriented release
  profiles exist; `wasm-opt` and bundle budgets are not CI gates yet. The stable
  production target uses `panic=abort`, so a panic is a terminal WASM trap and
  no post-panic recovery is promised for that instance. Reactive unwind safety
  applies only to targets built with unwinding support.
- **Server/SSR (planned):** the same folds will render HTML server-side. Hydration
  must be implemented and verified; deterministic reducers alone do not adopt or
  reconcile server DOM. Cloudflare Workers is the required deployment target.
- **Native later:** the reactive core + folds are DOM-free crates; a native
  renderer is additive.

## 6. Roadmap

```text
M0  spec (this doc) + scaffold                                    ← done
M1  SPIKE, the thesis on one screen:
    pliego-log + a hand-rolled fold + a hand-rolled render effect (no macros):
    a task list rendered as the fold of a real event log, append via click,
    undo via cursor, provenance visible per item. WASM, in-browser.
    GATE: replay(log) == live state, bit-for-bit.                  ← done
M2  pliego-reactive: the graph (tracking, coloring, equality gate, ownership)
    GATE: diamond runs effect once; untaken branch not tracked.    ← implemented
    NOTE: disposal reclamation, scoped runtimes, and panic safety remain.
M3  pliego-fold as a first-class node + snapshots
    GATE: reducer consumes only the tail from a snapshot.          ← solid foundation
M4  pliego-dom: HTML/DOM proof                                    ← experimental
    NEXT: retained/keyed rebuilds, properties/SVG, cleanup, hydration, macros.
M5  one-way client event push + durable Hyphae acknowledgement    ← experimental
    NEXT: versioned protocol, auth/tenant, idempotency, persistence, pull/merge,
    shared reducers, and field-level provenance.
M6  SSR + SSG on Cloudflare Workers, islands (near-zero WASM for static pages),
    streaming HTML — deterministic SSG, resumable islands, adapter lifecycle,
    file routes, redirects, and the first CLI slice are implemented. Streaming
    SSR and Cloudflare deployment remain.
M7  THE FULL-SITE PATH (the general-purpose goal): static-site generation,
    content collections, routing/pages, head/SEO management, asset pipeline —
    everything a content site needs, so PliegoRS replaces Astro/Next outright.
    GATE: rebuild a production-grade site in PliegoRS with equal-or-better
    Lighthouse scores and no JavaScript-framework dependency. The official
    PliegoRS site proves the public framework path; external acceptance remains.
M8  `pliego` umbrella + CLI owns the complete developer experience while
    composing Cargo, rustc, wasm-bindgen, and mature asset/build primitives.
```

## 7. What PliegoRS is NOT

- Not a from-scratch compiler or bundler. Users should ultimately interact with
  `pliego`, while it composes mature lower-level tooling behind that surface.
  Replacing Vite means replacing the workflow and product contract, not refusing
  to reuse proven compilers, optimizers, or packaging libraries.
- Not a Leptos wrapper or fork: we reimplement the reactive principles ourselves,
  sized to our model (one root, a forest of folds), and we owe the prior art its
  citation: Leptos, Reactively (Milo), SolidJS lineage.
- Not tied to Hyphae: `pliego-log` works standalone. Hyphae is the reference
  backend (the norte), not a hard dependency.
- The official site proves the design-led content-site lane, and
  `pliego-content` proves the generic typed-content contract.
