# G1 transport, load, and security evidence

**Gate:** G1 Native runtime and dynamic rendering
**State:** Closure candidate; public preview promotion requires protected merge and registry verification
**Toolchain:** Rust 1.86.0 under Debian WSL2 and Windows 11

## Transport contract

`NativeRuntime::serve` owns a bounded accept loop over Hyper's auto connection
builder. Hyper remains the single HTTP parser; PliegoRS configures and proves
the policy around it. `TransportLimits` has a deterministic digest and bounds:

- active TCP connections;
- the absolute HTTP/1 request-head deadline;
- read and write inactivity;
- HTTP/2 peer-initiated concurrent streams;
- HTTP/2 stream and connection flow-control windows; and
- the per-stream HTTP/2 send buffer.

`RequestLimits` supplies the parser's header-count and header-byte ceilings.
One runtime instance can start one native server. Excess accepted connections
are closed before request parsing, shutdown stops admission, every active
request receives `CancelReason::Shutdown`, each Hyper connection begins a
graceful drain, and the host aborts remaining tasks only after the configured
deadline expires.

The exact dependency line is pinned to Axum `0.8.9`, Hyper `1.10.1`, and
hyper-util `0.1.20`. Hyper documents that its lower-level connection API is the
customization boundary and that hyper-util's auto builder supports both HTTP
versions. Hyper also warns that HTTP/2 defaults are not a stability contract,
which is why PliegoRS sets them explicitly:

- <https://docs.rs/hyper/1.10.1/hyper/server/conn/>
- <https://docs.rs/hyper/1.10.1/hyper/server/conn/http2/struct.Builder.html>
- <https://docs.rs/hyper-util/0.1.20/hyper_util/server/conn/auto/struct.Builder.html>

TLS termination and proxy identity remain host-adapter or edge ownership. The
core never derives trusted scheme, client identity, or authorization from
`Host`, `Forwarded`, or `X-Forwarded-*`.

## Real-socket corpus

The `native_socket` integration suite exercises the actual loopback listener,
not only an in-process Tower service. It proves:

- HTTP/1.1 and HTTP/2 prior-knowledge requests over real TCP;
- 16 multiplexed HTTP/2 streams with a global four-request admission ceiling;
- connection rejection before request parsing;
- an HTTP/1 peer that sends bytes every 20 ms is still cut by the 60 ms
  absolute header deadline;
- a response reader that stops consuming releases its request and connection
  through the 75 ms write-inactivity policy;
- conflicting `Content-Length` and `Transfer-Encoding` fail with
  `PLG-RUN-110` before the handler;
- non-identity `Content-Encoding` fails with `PLG-RUN-108` because no
  decoded-byte budget exists;
- multipart fails with `PLG-RUN-109` until bounded part, filename, and storage
  policies exist; and
- parser-level header-count exhaustion terminates before a runtime scope is
  created.

The policy deliberately does not claim an upload or decompression feature.
Failing closed is the G1 contract; bounded parsers belong to G2.

## Fixed-load observation

The ignored `native_load` harness is explicit Linux evidence. It warms one
HTTP/2 connection, samples `/proc/self/status` every 2 ms, then performs 2,000
complete requests with fixed concurrency 32. It retains only latencies and an
atomic receipt count, not response objects or receipts.

Reproduction:

```bash
CARGO_TARGET_DIR="$HOME/.cache/pliegors-g1-target" \
  cargo test -p pliego-runtime --test native_load --locked -- \
  --ignored --nocapture
```

Observed on 2026-07-21:

```json
{
  "protocol": "h2c",
  "requests": 2000,
  "concurrency": 32,
  "p50Us": 2564,
  "p95Us": 4399,
  "p99Us": 42033,
  "baselineRssKiB": 10276,
  "peakRssKiB": 10500,
  "settledRssKiB": 10500
}
```

The observed peak and settled growth were both 224 KiB. This is a bounded
same-machine observation, not a universal performance claim or a comparison
against another framework.

## Streaming layouts and operations

`LayoutStreamDocument` accepts only layout IDs in the matched sealed route and
pre-composes the entire typed shell before response commitment. Ordered chunks
and asynchronous boundaries occupy exactly one internal child slot. A missing,
duplicate, foreign, or colliding slot fails pre-commit with `PLG-REN-008`.
Shell plus streamed content share one output-byte budget, and the request scope
still owns cancellation and LIFO cleanup through the final body frame.

## Logs and telemetry

Every completed request emits one `pliegors::request` structured tracing event
with contract `dev.pliegors.runtime-log/v1`. Its field set is restricted to a
sealed route ID, finite outcome, status, response bytes, coarse duration
bucket, render mode, diagnostic count, and bounded diagnostic code. It excludes
request and deployment IDs, concrete paths, query strings, headers, cookies,
bodies, identities, and diagnostic messages. Operator receipt-sink panics are
isolated from request cleanup.

OpenTelemetry remains separately opt-in and exporter-owned. See
[the OpenTelemetry evidence](g1-opentelemetry-foundation.md).

## ASVS ownership map

[`security/asvs-v5.0.0-g1.json`](../../security/asvs-v5.0.0-g1.json) maps the
framework-owned and shared G1 surface to the exact OWASP ASVS `5.0.0` IDs. It
does not claim that PliegoRS makes an application compliant. The source is the
tagged OWASP release, not the moving main branch:

- <https://github.com/OWASP/ASVS/tree/v5.0.0>
- <https://raw.githubusercontent.com/OWASP/ASVS/v5.0.0/5.0/docs_en/OWASP_Application_Security_Verification_Standard_5.0.0_en.csv>

The repository checker validates unique versioned IDs, allowed statuses,
explicit owners and rationales, and the existence of every evidence path.

## Remaining boundary

This evidence does not establish TLS, HTTP/3, WebSockets, proxy-aware client
identity, G2 loaders/actions/uploads, G3 host portability, a production traffic
SLO, or third-party application adoption. Those claims remain outside G1.
