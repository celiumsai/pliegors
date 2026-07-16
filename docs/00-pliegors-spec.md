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

**Current implementation boundary:** PliegoRS does not yet run as one deployed
machine or one log with Hyphae. The client crate now defines protocol v2 for
idempotent append, signed receipts and append/page attestations,
snapshot-consistent pull, authority verification, and type-gated replay. There
is still no production gateway, key distribution, durable browser outbox,
shared reducer package, deployed v2 Hyphae service, or field-level provenance.
Those remain product and infrastructure milestones, not properties implied by
the client contract.

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
current crates implement typed/versioned local events, sealed schema catalogs,
transactional projections, and contract-bound snapshots independently. Sharing
the same reviewed schema and reducer package with the Hyphae engine remains
planned integration work.

History, replay, and cursor-based time travel work in the local spike because its
log is retained. Protocol v2 now provides a fail-closed client verification
boundary, but durable audit and provenance still require a conforming deployed
service, production authority policy, persistent client state, and value/field
lineage. They do not follow from local replay or green client tests alone.

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
   segments. R4 adds retained keyed reconciliation, exact lifecycle ownership,
   and strict versioned SSR adoption with mismatch diagnostics.
4. **Ownership/disposal tree** with `on_cleanup`: folds and their render effects
   are owned, cancelled and dropped when their UI decision-point re-runs.
5. **Layer separation.** Leptos proved the reactive graph, the renderer, and the
   scheduler can be independent crates. We adopt that decomposition from day one.

**Leptos's honest costs we design around:** WASM bundle/startup (mitigated by
static output and resumable islands), disposed-signal panics (our handles are
typed against the owner; `try_` variants exist at runtime-controlled
boundaries), and SSR mismatches (R4 now fails closed through a versioned seed,
strict preflight, bounded diagnostics, and complete rollback).

## 3. The PliegoRS thesis and its implementation state

1. **The incremental projection node (verified, M3).** Leptos's `Memo` is the
   closest primitive to a materialized fold (derived + cached, equality-gated,
   and lazy), but when dirty it
   **re-runs its closure from scratch**. Our `Projection` (`Fold` remains an
   alias) extends the memo with candidate state, cached canonical state bytes,
   and an exact `LogCursor` containing position plus content head. On wake it
   resolves and folds only the checked tail, validates state through the codec,
   then publishes state/bytes/cursor together. Reducer work is O(new events);
   cloning, codec validation, and equality checks can still scale with state
   size. Contract-bound snapshots let restore consume only the exact tail on
   cold start.
2. **The write discipline (application contract).** Domain interactions append
   events rather than mutate projected state. Reactive signals still expose
   setters because the runtime and UI need writable coordination state; a future
   public application API should make the log discipline harder to bypass.
3. **Pure, deterministic reducers as a contract.** Replay must reproduce the
   same canonical state bytes and cursor. Generated-case and golden tests can
   validate observed behavior, but Rust's type system does not enforce reducer,
   mapper, upcaster, serializer/deserializer, equality, or custom-codec purity.
4. **Two tiers of "effect", separated explicitly.** A click handler is not an
   effect — it's an `append`. The only terminal effects are render effects (DOM)
   and declared side-effect ports. The loop: interaction → append → log red →
   folds green → lazy pull → surgical rebuild.
5. **Provenance as a UI primitive (planned).** The legacy M5 spike can display a
   local creation event and an unverified acknowledgement hash. Protocol v2 can
   establish verified event and checkpoint authority, but a future provenance
   model must still track which event range produced each value or field.
6. **The Hyphae seam.** The default client surface is now the v2 envelope,
   idempotent batch, exact cursor, signed append/page attestation, receipt,
   snapshot pull, and verified replay contract. The old `{kind,payload}` seam is
   isolated behind `experimental-legacy`. Production tenant authentication,
   transport, persistence, shared reducers, and field-level provenance remain.

## 4. Crate layout

```text
pliegors/
├─ crates/
│  ├─ pliego-log        typed/versioned, hash-chained local history, exact
│  │                    cursors, canonical payloads, and sealed schema catalogs
│  ├─ pliego-reactive   the reactive graph: observer tracking, two-phase coloring,
│  │                    equality gating, ownership/disposal  (lessons from
│  │                    reactive_graph, our implementation)
│  ├─ pliego-fold       transactional projection, canonical state codec, exact
│  │                    cursor, and contract-bound projection snapshot
│  ├─ pliego-dom        experimental HTML/DOM renderer with dynamic segments
│  ├─ pliego-ssg        deterministic pages, head/SEO, routes, assets, manifests
│  ├─ pliego-hyphae     protocol v2 append/pull, authority, and replay contract
│  ├─ pliego-macros     view! RSX + #[component]
│  └─ pliego-cli        create/dev/build/test/preview workflow
├─ planned/
│  └─ pliego            umbrella crate and user-facing framework surface
└─ examples/
   └─ spike/            local event fold plus experimental Hyphae ACK preview
```

## 5. Targets

- **Client:** `wasm32-unknown-unknown` via wasm-bindgen. Size-oriented release
  profiles exist; `wasm-opt` and bundle budgets are not CI gates yet. The stable
  production target uses `panic=abort`, so a panic is a terminal WASM trap and
  no post-panic recovery is promised for that instance. Reactive unwind safety
  applies only to targets built with unwinding support.
- **Server/SSR:** bounded HTML serialization and versioned browser adoption are
  implemented and verified. Streaming SSR and server functions remain future
  work. Cloudflare Workers is the required deployment target.
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
    GATE: reducer consumes only the exact tail from a snapshot.    ← R3 complete
M4  pliego-dom: owned DOM, keyed reconciliation, SSR adoption     ← R4 complete
    NEXT: broader property ergonomics and component macros.
M5  Hyphae seam
    protocol v2 client verification boundary                      ← implemented
    NEXT: production auth/tenant gateway, v2 service, durable outbox/replay
    persistence, shared reducers, and field-level provenance.
M6  SSR + SSG on Cloudflare Workers, islands (near-zero WASM for static pages),
    streaming HTML — deterministic SSG, resumable islands, adapter lifecycle,
    file routes, redirects, and the first CLI slice are implemented. Streaming
    SSR and Cloudflare deployment remain.
M7  THE FULL-SITE PATH (the general-purpose goal): static-site generation,
    content collections, routing/pages, head/SEO management, asset pipeline —
    everything a content site needs, so PliegoRS replaces Astro/Next outright.
    GATE: rebuild a production-grade site in PliegoRS with equal-or-better
    Lighthouse scores and no JavaScript-framework dependency. The official
    PliegoRS site proves the framework path, while Cairn closes the separate R7
    durable-workspace gate. The independent Lighthouse comparison remains.
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
