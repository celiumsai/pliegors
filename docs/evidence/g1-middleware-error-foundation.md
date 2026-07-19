# G1 middleware and error foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** Route middleware, capability admission, and root/route error slice; gate remains open

**Toolchain:** Rust 1.86.0 under Debian WSL2

## Sealed contract

`pliego-router` now binds ordered route middleware, its exact capability sets,
and root/route error boundary IDs into route graph digest v3. IDs are portable and bounded,
duplicates fail before sealing, and `NativeRuntimeBuilder` rejects missing or
extra registrations before a socket can be served.

Every referenced middleware must have one reachable declaration. The sealed
set distinguishes `rewrite-path`, `redirect`, `reject`, `read-body`, and
`mutate-response-headers`; changing it changes the graph digest. The native
runtime requires the registered implementation to present the exact same set
before startup. This is authority admission for trusted native Rust code, not
a sandbox: behavioral effect enforcement remains future work for the typed
middleware API and OpenSDK component boundary.

`pliego-runtime` provides a consume-once `MiddlewareNext`. Entered layers run
root-to-leaf and successful or recovered responses unwind leaf-to-root before
the runtime commits headers. A short-circuit cannot accidentally call the next
layer or handler twice.

Errors are reduced to four public classes: not found, unauthorized or
forbidden, invalid request, and internal failure. An application boundary
receives only class, status, bounded stable code, and optional route ID. It has
no accessor for the internal diagnostic message or chain. A failing, panicking,
or status-changing boundary is rejected and the runtime walks outward. The
last fallback remains plain no-JavaScript output owned by PliegoRS.

Runtime receipts record the middleware IDs actually entered and the boundary
that successfully authored the response. Internal messages remain in the
bounded receipt sink rather than the public page.

## Reproduction

From the repository root on Linux with Rust 1.86.0:

```bash
CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo test \
  -p pliego-router -p pliego-runtime -p native-pliego --locked

CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo clippy \
  -p pliego-router -p pliego-runtime -p native-pliego \
  --all-targets --locked -- -D warnings

cargo audit --deny warnings --ignore RUSTSEC-2026-0173
```

Observed focused result:

- `pliego-router`: 20 tests passed;
- `pliego-runtime`: 13 unit, 17 in-process integration, and two real-socket
  tests passed;
- `native-pliego`: four tests passed;
- Clippy with warnings denied passed; and
- doc tests passed.

`cargo audit` reported no vulnerability advisory. It retains one explicitly
ignored, pre-existing unmaintained warning: `RUSTSEC-2026-0173` for
`proc-macro-error2 2.0.1`, reached through
`syn_derive -> rstml -> pliego-macros`. The warning remains tracked supply-chain
work and is not represented as resolved.

The reference process also passed a raw loopback smoke for successful and 404
responses. CSP and `X-Content-Type-Options: nosniff` were present on both, the
404 was authored HTML containing only `PLG-RTE-404`, and `SIGINT` drained the
server. Observed response bodies were 790 bytes for `/`, 815 bytes for
`/stream`, and 471 bytes for `/missing`.

## Evidence boundary

This slice does not implement root pre-route, group, or layout middleware. It
admits exact declared capabilities but does not yet mediate their behavioral
effects inside trusted native Rust middleware. Nested layouts, asynchronous render
boundaries, HTTP/2, middleware fuzzing, fixed-load memory/latency, and
OpenTelemetry evidence remain open. This evidence does not close G1 or change
any capability from `not-released`.
