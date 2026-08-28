# ADR-012: Optional PliegoCSS build seam

**Status:** Accepted for source preview
**Decision date:** 2026-08-26
**Scope:** Product topology, optional CSS compilation, and runtime separation

## Context

PliegoRS owns application product topology while PliegoCSS owns typed style
identity, static CSS, reachability, bundle plans, and evidence. The existing
cross-repository fixture converts `ProductRegistry` through application-specific
Rust and JavaScript code, creating a duplicated and non-versioned boundary.

## Decision

PliegoRS publishes a canonical `pliegors-product-topology/1` JSON snapshot from
`ProductRegistry`. It contains only component IDs and source units, route IDs
and paths, island IDs and rendered names, and route-to-island occurrences.

PliegoCSS consumes this document as framework-owned input. It remains optional:
PliegoRS does not link PliegoCSS crates, download a compiler, ship style
metadata to the browser, or reinterpret `StyleId`. A `Style` continues to cross
the view boundary only as an ordinary CSS class string.

The topology snapshot does not contain CSS policy, theme configuration, bundle
IDs, deployment URLs, or compiler output. Those remain PliegoCSS or application
policy.

## Consequences

- Framework topology is declared once and can be hash-bound by both projects.
- PliegoCSS no longer needs to depend on PliegoRS Rust types to collect
  reachability.
- Future build/dev integration can use a closed data protocol rather than an
  unrestricted shell hook.
- The SSG `Site` and runtime route graph still require later explicit parity
  checks; this ADR does not silently merge those models.
