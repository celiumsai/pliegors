# pliego-router

`pliego-router` is the public G1 route-graph preview for PliegoRS.
It parses a bounded route grammar, rejects ambiguous shapes, seals registration
order into one deterministic graph, resolves admitted paths, and produces a
stable SHA-256 graph digest.

The crate is published on crates.io as `0.1.0-preview.1`. Its API may change on
another preview line and it is not wired into the `0.0.2` CLI. See
[`RFC-009`](../../docs/rfc/RFC-009-route-graph.md).
