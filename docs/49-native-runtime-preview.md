# Native runtime public preview

**Status:** G1 complete; coordinated public beta

The workspace contains `pliego-router` and `pliego-runtime` at
`0.3.0-beta.1`. Both are published on crates.io and tracked by the coordinated
[`v0.3.0-beta.1` release](https://github.com/celiumsai/pliegors/releases/tag/v0.3.0-beta.1).
They share one exact package graph with the CLI and remain preview APIs.

## Ownership boundary

`pliego-router` owns the portable route grammar, route IDs, collision
admission, deterministic matching, method dispatch, and graph digest. It does
not depend on Axum or a deployment provider.

`pliego-runtime` owns request admission, scope lifecycle, deadlines,
cancellation, cleanup, response commitment, response-body accounting,
diagnostics, receipts, structured request events, operator-enabled telemetry,
route-owned complete and streamed layout shells, and
complete/ordered/boundary server-rendering modes. Hyper retains HTTP parsing,
Axum retains service composition, and Tokio retains transport execution;
PliegoRS owns bounded connection admission and the explicit transport policy.

## Complete rendering

`CompleteDocument` accepts authored document metadata and a `pliego-dom`
`View` for the body. The renderer validates and escapes the shell, renders the
body through the existing bounded DOM walker, and accounts for the entire
document against one output limit.

```rust
use pliego_dom::{IntoView, el};
use pliego_runtime::{
    CompleteDocument, CompleteRenderOptions, render_complete_document,
};

let document = CompleteDocument::new(
    "Account",
    el("main").child("Signed in").into_view(),
)
.language("en")
.description("Account overview")
.canonical("https://example.com/account")
.stylesheet("/assets/account.css")
.module_script("/assets/account.js");

let response = render_complete_document(
    &document,
    CompleteRenderOptions::default(),
)?;
```

The default response is `200 text/html; charset=utf-8`. A non-body status such
as `204` or `304` is rejected instead of emitting invalid HTTP semantics.
Authored error pages may select a body-bearing status:

```rust
let options = CompleteRenderOptions::default()
    .status(pliego_runtime::StatusCode::NOT_FOUND);
```

`adoptable()` is explicit. Plain rendering does not insert browser-adoption
markers. In either mode, text and attributes are escaped by their owning typed
renderer, and the receipt records `renderMode: "complete"`.

## Route-owned layout documents

`RouteMatch::layout_ids()` exposes only layout scopes from the sealed graph;
pathless groups remain available through `scope_ids()` but never claim document
composition. `LayoutDocument` requires one `LayoutLayer` for each matched
layout, ordered root to leaf. Every layer owns exactly one structural
child frame by construction:

```rust
use pliego_dom::{IntoView, el};
use pliego_runtime::{
    DocumentHead, LayoutDocument, LayoutLayer, render_layout_document,
};

let shell = LayoutLayer::new("account-layout")?
    .before(el("nav").child("Account"))
    .wrap(el("div").class("account-shell"))
    .head(DocumentHead::new().stylesheet("/assets/account.css"));

let document = LayoutDocument::new(
    context.route(),
    el("main").child("Profile").into_view(),
)
    .layout(shell)?
    .title("Profile");

let response = render_layout_document(
    &document,
    CompleteRenderOptions::default(),
)?;
```

Complete composition walks layouts leaf to root so the final tree follows sealed
root-to-leaf ownership. `before`, `after`, and `wrap` transform a private child
frame that application code cannot clone, extract, or omit. Missing, duplicate, foreign, or invalid
layout identities return `PLG-REN-008` before commitment. Scalar head fields
use inner/page precedence; stylesheet and module script declarations retain
root-to-leaf order and exact duplicates are emitted once. The complete response
is bounded by the same metadata, depth, node, and byte limits and records
`renderMode: "layout"` plus `routeLayouts` in its receipt.

`LayoutStreamDocument` applies the same sealed ownership and head merge to
ordered and asynchronous boundary streams. The entire typed shell is rendered
and validated before commitment, split around one reserved internal child
slot, and then streamed under one shell-plus-content byte budget. Absence,
duplication, or authored collision of that slot fails pre-commit with
`PLG-REN-008`. Request cancellation and LIFO cleanup remain owned by the same
scope through the last streamed frame.

```rust
use pliego_runtime::{
    LayoutStreamDocument, OrderedRenderOptions,
    render_layout_ordered_document,
};

let document = LayoutStreamDocument::new(context.route())
    .layout(shell)?
    .title("Profile");

let response = render_layout_ordered_document(
    &document,
    chunks,
    OrderedRenderOptions::default(),
)?;
```

Data loaders and resource handles begin in G2; G1 does not invent ambient
network or database authority inside a layout.

## Operator-enabled OpenTelemetry

The runtime emits no request telemetry by default. An operator first configures
the global OpenTelemetry tracer and meter providers, then opts the runtime in:

```rust
use pliego_runtime::{
    HttpScheme, NativeRuntimeBuilder, OpenTelemetryConfig, RemoteTracePolicy,
};

let telemetry = OpenTelemetryConfig::new(HttpScheme::Https)
    .known_method("PROPFIND")?
    .remote_trace_policy(RemoteTracePolicy::AcceptW3c);

let runtime = NativeRuntimeBuilder::new(graph, "production-a")?
    .open_telemetry(telemetry)
    // handlers, middleware, and boundaries
    .build()?;
```

The required `HttpScheme` is trusted operator configuration, never inferred
from an attacker-controlled `Host` or forwarding header. PliegoRS captures the
operator's global providers when
`open_telemetry` runs. It never installs an exporter, endpoint, credential,
batch processor, or collector. A `SERVER` span begins before admission and
ends only when the response body completes, fails, or disconnects. This keeps
stream errors and last-byte duration inside the same request lifecycle.

The stable signal surface is:

- `http.server.request.duration` in seconds;
- `http.server.active_requests` in requests, with identical increment and
  decrement attributes;
- `http.server.response.body.size` in bytes; and
- the server span named from the admitted method and sealed route template.

Attributes are restricted to known/explicitly allowlisted methods, the trusted
scheme, protocol version, sealed `http.route`, status, response size, route ID,
render mode, runtime outcome, the receipt contract, and a finite framework
diagnostic-code allowlist. Unknown methods and application diagnostic codes
become `_OTHER`.
Concrete paths, query strings, server addresses, request headers, cookies,
bodies, user identifiers, request IDs, deployment IDs, and diagnostic messages
never enter the default signal. The privacy profile therefore deliberately
omits `url.path` from the otherwise stable HTTP server span convention rather
than mislabeling a route template as a concrete path.

Inbound `traceparent` is ignored by default. Explicit `AcceptW3c` admits only a
valid parent parsed by the W3C propagator. Inbound `tracestate`, baggage, and
other propagation formats are discarded so a peer cannot inject provider
state into exported telemetry. Runtime receipts stay exporter-independent and
record a coarse duration bucket rather than precise wall time.

## Structured request events

Every terminal request emits one `tracing` event on target
`pliegors::request` with contract `dev.pliegors.runtime-log/v1`. The exact
fields are sealed route ID, outcome, status, response bytes, coarse duration
bucket, render mode, diagnostic count, and bounded diagnostic code. Concrete
path and query, headers, cookies, body, user/request/deployment identity, and
diagnostic messages are excluded. PliegoRS selects no subscriber, formatter,
destination, clock, retention, or alert policy; those remain operator-owned.
Receipt sink and OpenTelemetry callbacks are panic-isolated from request
cleanup.

## Ordered rendering

`OrderedDocument` emits the validated document shell first. Each
`OrderedViewChunk` is a `Send` factory that constructs one `pliego-dom` view
only when the response body is polled for its next frame. The view is rendered,
released, and sent before the input stream is polled again.

```rust
use futures_util::stream;
use pliego_dom::{IntoView, el};
use pliego_runtime::{
    OrderedDocument, OrderedRenderOptions, OrderedViewChunk,
    render_ordered_document,
};

let document = OrderedDocument::new("Activity");
let chunks = stream::iter([
    OrderedViewChunk::new(|| el("h1").child("Activity").into_view()),
    OrderedViewChunk::new(|| el("p").child("Ready").into_view()),
]);

let response = render_ordered_document(
    &document,
    chunks,
    OrderedRenderOptions::default(),
)?;
```

Ordered output has no `Content-Length`. The response owns at most one rendered
sibling fragment at a time and does not poll the next factory until the body
consumer requests another frame. The shell and every emitted fragment share
one output-byte budget. Fragment depth and nodes are bounded, chunk count has a
hard ceiling, and document metadata has count plus aggregate-byte limits.

A panic in the input stream or chunk factory becomes a post-commit body error.
It terminates the stream and produces a failed runtime receipt; it cannot
replace the committed status. Ordered mode is intentionally plain SSR until a
single adoption contract can span the streamed sibling sequence.

## Asynchronous boundary rendering

`BoundaryDocument` accepts a finite declaration of uniquely identified
`AsyncBoundary` values. Each boundary owns a `Send` future that resolves to a
factory for one non-`Send` `pliego-dom` view. The scheduler starts at most four
futures concurrently by default, but emits the resolved HTML in declaration
order. This gives independent I/O overlap without changing accessible document
order.

```rust
use pliego_dom::{IntoView, el};
use pliego_runtime::{
    AsyncBoundary, BoundaryDocument, BoundaryRenderOptions,
    render_boundary_document,
};

let document = BoundaryDocument::new("Account");
let boundaries = [
    AsyncBoundary::map("heading", async { "Account" }, |title| {
        el("h1").child(title).into_view()
    })?,
    AsyncBoundary::try_map("activity", load_activity(), |items| {
        el("p").child(format!("{} items", items.len())).into_view()
    })?,
];

let response = render_boundary_document(
    &document,
    boundaries,
    BoundaryRenderOptions::default(),
)?;
```

Before each result, the stream emits an inert, stable anchor:

```html
<template data-pliego-boundary="activity"></template>
```

The resolved HTML follows its anchor. No inline bootstrap, DOM replacement, or
client JavaScript is required, and useful HTML remains the baseline. This first
contract deliberately does not deliver later boundaries out of order. It is
not a claim of React Flight, Suspense patching, or partial prerendering.

Boundary identities are ASCII, 1-64 bytes, unique per response, and validated
before commitment. Defaults allow 32 declarations, four in flight, and five
seconds per future; hard ceilings are 256, 32, and 60 seconds. Applications may
tighten those values. Shell, all placeholder anchors, and every resolved view
share one output budget. Depth and node limits apply independently to each
resolved view.

Timeout, future panic, factory panic, output exhaustion, or scheduler failure
after the prefix is committed terminates the body. It cannot rewrite the status
or emit a second error document. `try_map` converts an application failure into
`PLG-REN-210` without retaining its potentially sensitive error text. Dropping
the client response still cancels the request scope and drops every pending
boundary through the host-owned body.

## Runtime limits

`RequestLimits` currently bounds:

- request target bytes;
- header count and aggregate header bytes;
- declared and streamed request-body bytes;
- response bytes;
- diagnostics and application cleanup callbacks;
- 64 exact known telemetry methods with 32-byte method identities;
- concurrent admitted requests;
- ordered render chunks and document metadata;
- asynchronous boundary count, in-flight work, timeout, and identities;
- request deadline; and
- graceful-shutdown drain time.

`TransportLimits` separately bounds active connections, HTTP/1 head-read
time, read/write inactivity, HTTP/2 concurrent streams, flow-control windows,
and per-stream send buffers. Header count and bytes come from `RequestLimits`
and are applied to both the parser and runtime admission. The transport policy
has its own deterministic SHA-256 digest available through
`NativeRuntime::transport_policy_sha256()`.

Non-identity `Content-Encoding`, multipart media types, and conflicting
`Content-Length` plus `Transfer-Encoding` fail closed with `PLG-RUN-108`,
`PLG-RUN-109`, and `PLG-RUN-110`. PliegoRS performs no implicit decompression
or multipart parsing before G2 supplies independent decoded-byte, part-count,
filename, and storage policies.

`RenderLimits` separately bounds DOM depth, node count, and total document
output. Raising a policy changes the limit-policy digest stored in every
runtime receipt.

## Failure behavior

The source implementation currently demonstrates:

- overload rejection before the second handler runs;
- strict route decoding and portable case/Unicode collision rejection;
- handler panic isolation when unwinding is available;
- cancellation wakeups for pending response streams;
- client-disconnect and shutdown cleanup;
- raw TCP HTTP/1.1 loopback dispatch and graceful shutdown;
- real TCP HTTP/2 prior-knowledge dispatch and multiplexed overload behavior;
- connection admission, absolute slow-head timeout, slow-reader release, and
  fixed-load Linux RSS/latency evidence;
- graph-bound route middleware with consume-once `Next` and reverse response
  unwinding before commitment;
- graph-digested middleware capability declarations with exact startup
  admission against the native registry;
- root pre-route middleware that can rewrite before matching or terminate
  without creating false route context;
- fail-closed mediation of path rewrites, redirects, rejection, body reads,
  and downstream response-header changes with `PLG-RUN-507`;
- root and route error boundaries that receive no internal diagnostic message;
- bounded group/layout scope inheritance with deterministic middleware,
  outward error recovery, and scope identity in receipts;
- route-bound complete and streamed layout composition with exactly one child
  slot, deterministic head merging, and layout identity in receipts;
- bounded structured completion logs that exclude user values and isolate
  operator sink panics;
- operator-enabled OpenTelemetry spans and HTTP metrics across the complete
  body lifecycle, with W3C propagation opt-in and bounded redaction/cardinality;
- exactly-once bounded receipts; and
- pre-commit complete-render failures with stable `PLG-REN-*` diagnostics.

## Dynamic reference application

[`examples/native-pliego`](../examples/native-pliego/) seals six native
routes in one graph: complete SSR, a typed parameter route, ordered SSR,
async-boundary SSR, a JSON health response, and a stylesheet asset. The executable binds to
`127.0.0.1:4310` by default, rejects non-loopback addresses unless
`PLIEGO_EXPOSE=1` is explicit, and drains through the runtime on `Ctrl+C`.

The application is reproducible G1 evidence, not a released starter or a
production-readiness claim.

The reference graph declares a `canonical-entry` pre-route rewrite and every
route inherits `response-policy` middleware and its
`mutate-response-headers` capability in the sealed graph. The runtime admits
the implementation only when its registered capability set matches. It adds
CSP, referrer, and content-type protections before commitment.
The root boundary renders bounded no-JavaScript HTML for not-found, access,
invalid-request, and internal failures. A socket smoke verifies the policy on
both successful and authored 404 responses.

## Deliberately outside G1

The following remain later-gate work:

- G2 typed loaders, actions, caches, sessions, uploads, multipart parsing, and
  bounded decompression;
- TLS termination and provider-specific proxy trust policy beyond the G3
  PBOC, OCI, and Cloudflare preview;
- G4 independent-team adoption;
- G6 operator-specific retention, alerting, incident, and SLO policy; and
- HTTP/3 and WebSockets, which have no current gate promise.

The exact G1 transport measurements and ASVS ownership map are recorded in
[transport, load, and security evidence](evidence/g1-transport-load-security.md).
