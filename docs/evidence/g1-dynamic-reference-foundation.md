# G1 dynamic reference foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** Launchable reference slice; gate remains open

**Toolchain:** Rust 1.86.0 under Debian WSL2

**Application:** [`examples/native-pliego`](../../examples/native-pliego/)

## Demonstrated path

The application composes the unreleased PliegoRS router, runtime, DOM, complete
renderer, and ordered renderer in one native binary. Its sealed route graph
owns:

- `/` for a complete server-rendered document;
- `/hello/:name` for portable parameter resolution and escaped output;
- `/stream` for sibling-granularity ordered server rendering;
- `/health` for an explicit JSON operational response; and
- `/assets/site.css` for a response-time asset.

Every route declares response-policy middleware in the sealed graph. The
application also registers a root error boundary that returns bounded
no-JavaScript documents without receiving internal diagnostic messages.

The process defaults to loopback, requires `PLIEGO_EXPOSE=1` before accepting a
non-loopback bind address, and connects `Ctrl+C` to the runtime graceful
shutdown future.

## Reproduction

From the repository root on Linux with Rust 1.86.0:

```bash
CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo test \
  -p native-pliego --locked

CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo clippy \
  -p native-pliego --all-targets --locked -- -D warnings

CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo run \
  -p native-pliego --locked
```

Observed automated result:

- four application tests passed;
- complete HTML, ordered HTML, CSS, authored 404 output, response policy, and
  bind policy passed;
- Clippy with warnings denied passed; and
- doc tests passed.

The launched binary also passed a loopback HTTP smoke against `/`, `/stream`,
`/health`, `/assets/site.css`, and `/missing`. The successful response bodies
were 790, 815, 51, and 1,183 bytes respectively; the authored 404 was 471
bytes. CSP and `X-Content-Type-Options` were present on successful and error
responses, and `SIGINT` completed a clean process exit.

## Evidence boundary

This application has not passed the fixed-load acceptance profile. It does not
yet prove p50/p95/p99 latency, peak RSS, memory plateau, slow peers, overload,
HTTP/2, TLS, group/layout middleware,
asynchronous render boundaries, OpenTelemetry, multipart/decompression policy,
or deployment from a sealed release candidate. It does not close G1 or change
any capability from `not-released`.
