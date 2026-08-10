# ADR-008: Adapt the native host before evaluating Vite

**Status:** Accepted
**Decision date:** 2026-08-08
**Scope:** development host abstraction, Vite evaluation, CLI compatibility, and daemon admission

## Context

The original Next proposal made Vite the stable initial `BuildHost` and deferred
a Rust `NativeBuildHost` until after a public beta. The current product already
ships a bounded native development path: project discovery, verified Cargo and
wasm-bindgen execution, deterministic SSG publication, filesystem watching, a
Rust development server, SSE updates, HMR classification, diagnostics, and
explanation commands.

That implementation is coupled inside `pliego-cli` and is not a host-neutral
contract. It also does not provide a persistent compiler session or causal
state-preserving HMR. Vite, Rolldown, and Oxc are not current framework build
dependencies.

Replacing the existing path before measuring it would discard a working
baseline and make host choice a preference rather than an evidence-based
decision.

## Decision

1. Define a host-neutral build-session contract before adding a new host.
2. Adapt the current native build and development path as the reference
   implementation. Preserve its public behavior while moving host-specific
   process, watcher, server, and transport details behind the boundary.
3. Implement Vite as private experimental repository tooling after the native
   adapter and Phase 0 baseline exist.
4. Compare hosts using the same source fixtures, Pliego-owned manifests,
   diagnostics, artifact identities, state roots, conformance cases, and clean-
   shutdown tests.
5. Keep `--host` as the released network-bind option. A build implementation
   selector uses `--build-host` if and when more than one implementation is
   exposed.
6. Do not publish a Vite plugin to npm during the spike. First-party Node
   packages remain private under the current product constitution.
7. Do not make `pliegod` mandatory by architecture alone. Introduce a daemon
   only after a bounded spike demonstrates material value for persistent compile
   sessions, graph authority, inspection, or multi-client coordination.
8. A daemon protocol, if admitted, uses versioned bounded messages, references
   large artifacts by verified identity, authenticates or confines its local
   endpoint, and supports deterministic shutdown.
9. Vite never defines public Pliego semantics. Vite/Rolldown/Oxc types and plugin
   hooks stay inside the adapter.

## Host comparison gate

A host is eligible for supported or default status only when it reports:

- cold and warm startup;
- small Rust/WASM update p50, p95, and p99;
- CSS and static-asset update p50, p95, and p99;
- server-to-browser-visible latency;
- process-tree memory and long-session high-water behavior;
- orphan-process and shutdown results;
- manifest, diagnostic, artifact, and state-root equivalence;
- plugin compatibility and reproducibility limitations;
- implementation and maintenance cost.

A small performance difference alone does not justify two complete supported
implementations. A default change requires a material product advantage,
structural compatibility need, or invariant unavailable from the reference
host.

## Consequences

- Current users retain a functioning native `pliego dev` while extraction
  proceeds.
- Phase 0 measurements include a real legacy comparator.
- Vite can provide mature browser tooling without leaking into framework state,
  runtime, or artifact authority.
- The design may discover that an in-process persistent `BuildSession` is
  sufficient and a daemon is unnecessary.
- Native runtime/server code is not automatically reused as development-host
  code; their current security and lifecycle boundaries remain distinct.

## Rejected alternatives

- **Vite first with no native adapter:** rejected because it removes the current
  comparator and risks changing public behavior before the boundary is known.
- **Native only without a Vite spike:** rejected because Vite may solve measured
  compatibility or development-experience gaps.
- **Daemon as the first implementation task:** rejected because no baseline yet
  proves which cost a separate process must remove.
- **Reuse `--host` for implementation selection:** rejected because it already
  accepts a network interface address.
- **Publish npm packages during the experiment:** rejected because registry
  publication creates a support and governance surface before the gate passes.

## References

- [PliegoRS 0.5 roadmap](../../ROADMAP.md)
- [Transition program](../52-pliegors-next-transition.md)
- [Golden developer experience](../32-golden-developer-experience.md)
- [G4 engineering readiness](../51-g4-engineering-readiness.md)
- [Product constitution](../34-product-constitution.md)
