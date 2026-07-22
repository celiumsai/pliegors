# pliego-runtime

`pliego-runtime` is the public G1 native-runtime preview for PliegoRS. It owns
bounded request admission, request scope, cancellation, deadline propagation,
concurrency admission, LIFO cleanup, response commitment, streamed-body
ownership, panic isolation, graceful-shutdown draining, diagnostics, and
runtime receipts on top of Axum, Hyper, Tower, and Tokio.

Route-local middleware and root/route error boundaries are sealed by
`pliego-router`. Middleware uses a consume-once `MiddlewareNext`, unwinds
responses before commitment, and remains active for recovered downstream
errors. Error boundaries receive only a bounded public class, status, code,
and optional route ID; internal diagnostic messages remain receipt-only.
Middleware capability sets are part of route graph v4 and must exactly match
the native registry before startup. The runtime then mediates path rewrites,
redirects, rejection, request-body reads, and downstream response-header
changes at the `Next` boundary; undeclared effects fail before commitment with
`PLG-RUN-507`. This is a framework boundary, not a general sandbox for trusted
application code.
Root pre-route middleware has a distinct context without route authority and
runs before matching; it may rewrite or short-circuit while preserving the
single post-unwind response commitment.

The source tree also contains bounded `complete`, `ordered`, and `boundary`
server-rendering modes over `pliego-dom`. They emit typed HTML, validate
metadata and response status, preserve backpressure, and bind the render mode
into the runtime receipt before commitment. Boundary mode starts a bounded
number of declared futures concurrently, emits stable inert placeholders, and
delivers resolved HTML in declaration order without requiring client-side
JavaScript.

Complete responses use `LayoutDocument`, while ordered and boundary streams
use `LayoutStreamDocument`, to bind composition to the exact root-to-leaf
layout identities in the sealed route match. Each `LayoutLayer`
transforms one private child frame through typed `before`, `after`, and `wrap`
operations, so it cannot drop or duplicate the child. Missing, duplicate, or
foreign layers fail before response commitment. Head contributions merge in
route ownership order, leaf/page scalars win, asset order stays stable, and
exact duplicate assets are emitted once. Groups never masquerade as layouts,
and receipts record both the complete scope chain and layout-only identity.
Streamed shells validate one internal child slot before commitment and account
for shell plus streamed chunks under one output budget.

`NativeRuntime::serve` uses one configurable Hyper HTTP/1.1 and HTTP/2 parser
path behind a bounded accept loop. `TransportLimits` caps connections, the
absolute HTTP/1 head deadline, read/write inactivity, HTTP/2 streams,
flow-control windows, and send buffers. The generic request path rejects
conflicting body framing and implicit parsing. G2 action routes opt into
independent encoded/decoded, field, part, file, and temporary-storage budgets
before form, JSON, gzip, or multipart decoding. Every completed request emits one bounded
`pliegors::request` structured event; operator receipt sinks are panic-isolated.

`NativeRuntimeBuilder::open_telemetry` binds the runtime to global
OpenTelemetry providers configured by the operator. PliegoRS does not install
an exporter or collector, and the builder remains uninstrumented by default.
Enabled runtimes emit a `SERVER` span from request admission through the last
response-body frame plus `http.server.request.duration`,
`http.server.active_requests`, and `http.server.response.body.size`. Method,
operator-trusted `HttpScheme`, protocol, sealed route template, status, render
mode, bounded framework error type, and receipt contract are allowlisted.
Concrete paths, query strings, headers, cookies, bodies, user IDs, request IDs,
diagnostic messages, and deployment IDs are excluded. Valid W3C `traceparent`
values are ignored by default and require `RemoteTracePolicy::AcceptW3c`;
inbound `tracestate` and baggage are discarded so provider state remains
local. Custom HTTP methods require an explicit bounded allowlist entry.
Receipts retain only a coarse duration bucket.

The crate is published on crates.io as `0.1.0-preview.1`. Its API may change on
another preview line, it is not wired into the `0.0.2` CLI, and publication is
not a stable production-support promise. See
[`RFC-008`](../../docs/rfc/RFC-008-native-runtime.md).

The conformance corpus includes raw TCP HTTP/1.1 and HTTP/2, multiplexed
overload, graceful shutdown with a pending stream, connection admission,
slow-head and slow-reader peers, parser/body-policy cases, and an explicit
2,000-request Linux RSS/latency harness. TLS and proxy identity remain host
adapter work rather than implicit trust in forwarding headers.

Current `main` also contains the unreleased G2 source beta: sealed loader,
action, session, idempotency, cache, upload, decompression, invalidation, and
application-contract registries backed by `pliego-data`. These additions are
not in the published `0.1.0-preview.1` runtime crate. G3 host adapters and a
production `pliego serve` command remain unavailable.

See the [G1 transport evidence](../../docs/evidence/g1-transport-load-security.md),
[G2 full-stack evidence](../../docs/evidence/g2-fullstack-beta.md), and the
[G1](../../security/asvs-v5.0.0-g1.json) and
[G2](../../security/asvs-v5.0.0-g2.json) ASVS ownership maps.
