# pliego-router

`pliego-router` is the unreleased G1 route-graph implementation for PliegoRS.
It parses a bounded route grammar, rejects ambiguous shapes, seals registration
order into one deterministic graph, resolves admitted paths, and produces a
stable SHA-256 graph digest.

The crate is `0.1.0-preview.1` source work. It is not published on crates.io and
does not promote the `fullstack-routing` capability in
`product.capabilities.json`. See
[`RFC-009`](../../docs/rfc/RFC-009-route-graph.md).
