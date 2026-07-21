# G1 asynchronous-boundary foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** In progress; bounded declaration-order boundary mode

**Base revision:** `b00259c12fb5b069b2a50461bd244c9f098e834b`

**Toolchain:** Rust and Cargo 1.86.0 on Windows x86-64

## Contract under test

The unreleased `pliego-runtime` source now provides the third rendering mode
defined by RFC-008. A `BoundaryDocument` receives a finite declaration of
`AsyncBoundary` futures. The response streams:

1. one validated document prefix;
2. one stable inert `<template data-pliego-boundary="id"></template>` anchor
   followed by each resolved view in declaration order; and
3. one document suffix.

Up to four futures start concurrently by default. `FuturesOrdered` permits
later I/O to make progress while preventing later HTML from overtaking an
earlier declaration. The result requires no inline script, client bootstrap,
or DOM replacement. It is useful progressive HTML, not an out-of-order patch
protocol.

The implementation enforces:

- ASCII boundary identities of 1-64 bytes and pre-commit duplicate rejection;
- default/hard declaration ceilings of 32/256;
- default/hard in-flight ceilings of 4/32;
- default/hard per-boundary timeouts of 5/60 seconds;
- one aggregate byte budget for shell, every anchor, and resolved HTML;
- DOM depth and node bounds on every resolved view;
- panic isolation around futures, factories, rendering, and scheduling;
- application-failure conversion without retaining authored error text;
- post-commit body termination without status replacement;
- cancellation by the runtime-owned response body; and
- `renderMode: "boundary"` in the exactly-once runtime receipt.

## Reproduction

```powershell
cargo test -p pliego-runtime -p native-pliego --locked
cargo clippy -p pliego-runtime -p native-pliego --all-targets --locked -- -D warnings
```

Observed targeted result:

- 17 `pliego-runtime` unit tests passed;
- 26 native runtime integration tests passed;
- 2 raw socket tests passed;
- 6 native reference application tests passed; and
- both crates' doc tests passed; and
- Clippy passed for every target in both crates with warnings denied.

The concurrency tests hold the first boundary pending, observe the second
future start when the ceiling is two, prove it cannot start when the ceiling is
one, verify that no second result overtakes the first, and check the exact
placeholder/result sequence. Separate cases reject invalid and duplicate
declarations before commitment and terminate the body after timeout or panic.
Axum integration consumes a complete boundary response and inspects its receipt;
a timeout case proves the committed `200` is not rewritten while the receipt
ends `failed` with `PLG-RUN-501`. The reference application verifies that
`/boundary` resolves with no script element.

Workspace regression also passed tests and Clippy for every target, Rustdoc with
warnings denied, the WASM target lint, 98 JavaScript contract tests with one
declared skip, documentation and product-truth validation, the 77-route site
contract, Worker dry-run packaging, and the crates.io package/publish dry-run.
`cargo audit` reported no vulnerability and retained the already documented
allowed `RUSTSEC-2026-0173` unmaintained warning.

## Evidence boundary

This slice establishes bounded asynchronous work and stable declaration-order
HTML delivery. It does not implement out-of-order browser patching, fallback
replacement, layout-owned child slots, loader semantics, partial prerendering,
or a distributed cache. It does not close G1 or promote `dynamic-ssr` from
`not-released`.
