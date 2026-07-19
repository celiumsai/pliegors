# Native runtime preview source

**Status:** Unreleased G1 source work

The workspace contains `pliego-router` and `pliego-runtime` at
`0.1.0-preview.1`. They are implementation work for G1, are not published on
crates.io, are not wired to the released CLI, and do not change the public
capability status in `product.capabilities.json`.

## Ownership boundary

`pliego-router` owns the portable route grammar, route IDs, collision
admission, deterministic matching, method dispatch, and graph digest. It does
not depend on Axum or a deployment provider.

`pliego-runtime` owns request admission, scope lifecycle, deadlines,
cancellation, cleanup, response commitment, response-body accounting,
diagnostics, receipts, and complete/ordered server-rendering modes. Axum,
Hyper, Tower, and Tokio retain HTTP transport, service, and executor ownership.

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

## Runtime limits

`RequestLimits` currently bounds:

- request target bytes;
- header count and aggregate header bytes;
- declared and streamed request-body bytes;
- response bytes;
- diagnostics and application cleanup callbacks;
- concurrent admitted requests;
- ordered render chunks and document metadata;
- request deadline; and
- graceful-shutdown drain time.

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
- graph-bound route middleware with consume-once `Next` and reverse response
  unwinding before commitment;
- root and route error boundaries that receive no internal diagnostic message;
- exactly-once bounded receipts; and
- pre-commit complete-render failures with stable `PLG-REN-*` diagnostics.

## Dynamic reference application

[`examples/native-pliego`](../examples/native-pliego/) seals five native
routes in one graph: complete SSR, a typed parameter route, ordered SSR, a
JSON health response, and a stylesheet asset. The executable binds to
`127.0.0.1:4310` by default, rejects non-loopback addresses unless
`PLIEGO_EXPOSE=1` is explicit, and drains through the runtime on `Ctrl+C`.

The application is reproducible G1 evidence, not a released starter or a
production-readiness claim.

Every reference route declares `response-policy` middleware in the sealed
graph. It adds CSP, referrer, and content-type protections before commitment.
The root boundary renders bounded no-JavaScript HTML for not-found, access,
invalid-request, and internal failures. A socket smoke verifies the policy on
both successful and authored 404 responses.

## Deliberately absent

There is no asynchronous boundary streaming API yet. Ordered mode streams
independent body siblings and buffers at most one bounded sibling view; it does
not claim incremental output inside one DOM tree.

The following remain gate work:

- declared asynchronous boundaries;
- pre-route, group, and layout middleware with declared rewrite, redirect,
  reject, body-read, and response-mutation capabilities;
- OpenTelemetry with redaction and cardinality tests;
- multipart and decompression policies;
- real socket HTTP/2 conformance; and
- fixed-load latency, RSS, disconnect, slow-peer, overload, and shutdown
  evidence.
