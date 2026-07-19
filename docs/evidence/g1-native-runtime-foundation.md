# G1 native runtime foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** In progress; foundation only

**Revision:** Working tree after accepted G0 revision `7236113`

**Toolchain:** Rust 1.86.0 under Debian WSL2

## Implemented slice

The unreleased `pliego-router` crate now provides:

- a bounded portable grammar for literals, parameters, terminal optional
  parameters, terminal catch-all parameters, and pathless groups;
- compile-time duplicate ID, route-shape, and optional-expansion collision
  rejection;
- deterministic literal/parameter/optional/catch-all precedence;
- method dispatch with a stable `Allow` set;
- strict admitted-path validation; and
- a graph digest independent of registration order.

The unreleased `pliego-runtime` crate now provides:

- Axum request dispatch while keeping the route graph host-neutral;
- bounded request target, headers, declared/streamed body, response,
  diagnostics, cleanup, concurrency, deadline, and shutdown policies;
- an explicit request lifecycle with contagious cancellation;
- LIFO application cleanup and internal registry cleanup;
- overload rejection before a second handler runs;
- panic isolation for handler creation and polling when unwinding is available;
- streamed response ownership with cancellation wakeups and disconnect cleanup;
- graceful-shutdown admission closure and active-scope cancellation; and
- bounded, exactly-once runtime receipts without request payloads.

The host continues to use upstream Tokio, Hyper, Tower, and Axum semantics. No
custom HTTP parser, socket stack, or executor was introduced.

## Reproduction

From the repository root on Linux with Rust 1.86.0:

```bash
CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo test \
  -p pliego-router -p pliego-runtime --locked

CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo clippy \
  -p pliego-router -p pliego-runtime --all-targets --locked -- -D warnings

CARGO_TARGET_DIR=/tmp/pliegors-g1 RUSTDOCFLAGS="-D warnings" cargo doc \
  -p pliego-router -p pliego-runtime --no-deps --locked
```

Observed foundation result:

- `pliego-router`: 17 unit tests passed;
- `pliego-runtime`: 8 unit tests and 9 integration tests passed;
- doc tests: passed;
- Clippy with warnings denied: passed; and
- rustdoc with warnings denied: passed.

The integration corpus includes real Axum service dispatch, oversized declared
body rejection, method rejection, deadlines, overload, handler panic,
shutdown of a pending stream, client disconnect, portable case/Unicode alias
rejection, Unicode normalization, cleanup, and receipts.

## Gate still open

This evidence does **not** close G1 and does not promote either crate or its
capabilities to a released state. The following acceptance work remains:

- PliegoRS complete, ordered, and boundary server rendering;
- group and layout middleware (the capability-mediated
  pre-route, route-local, and root/route boundary slice is recorded in
  [`g1-middleware-error-foundation.md`](g1-middleware-error-foundation.md));
- normalized query and generated typed parameter contracts;
- multipart and decompression limits;
- OpenTelemetry spans, metrics, redaction, and cardinality tests;
- differential request-target corpus, property tests, and fuzz targets;
- real socket HTTP/2 conformance (HTTP/1.1 loopback evidence is recorded in
  [`g1-native-socket-foundation.md`](g1-native-socket-foundation.md));
- slowloris, slow-reader, overload, disconnect, and shutdown load evidence;
- fixed-load latency and peak RSS measurements with a memory plateau;
- dependency audit and an updated threat-control map; and
- a sealed dynamic reference application with zero unresolved P0 findings.

The canonical product manifest therefore retains `native-http-runtime`,
`fullstack-routing`, and `dynamic-ssr` as `not-released`.
