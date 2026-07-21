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
diagnostics, receipts, layout-owned complete documents, and
complete/ordered/boundary server-rendering modes. Axum, Hyper, Tower, and Tokio
retain HTTP transport, service, and executor ownership.

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

## Layout-owned complete documents

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

Composition walks layouts leaf to root so the final tree follows sealed
root-to-leaf ownership. `before`, `after`, and `wrap` transform a private child
frame that application code cannot clone, extract, or omit; the implementation
does not parse or replace HTML strings. Missing, duplicate, foreign, or invalid
layout identities return `PLG-REN-008` before commitment. Scalar head fields
use inner/page precedence; stylesheet and module script declarations retain
root-to-leaf order and exact duplicates are emitted once. The complete response
is bounded by the same metadata, depth, node, and byte limits and records
`renderMode: "layout"` plus `routeLayouts` in its receipt.

This first contract composes complete documents. Ordered and asynchronous
boundary modes do not yet accept layout frames, and layouts do not yet own
loaders or request cleanup.

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
- concurrent admitted requests;
- ordered render chunks and document metadata;
- asynchronous boundary count, in-flight work, timeout, and identities;
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
- graph-digested middleware capability declarations with exact startup
  admission against the native registry;
- root pre-route middleware that can rewrite before matching or terminate
  without creating false route context;
- fail-closed mediation of path rewrites, redirects, rejection, body reads,
  and downstream response-header changes with `PLG-RUN-507`;
- root and route error boundaries that receive no internal diagnostic message;
- bounded group/layout scope inheritance with deterministic middleware,
  outward error recovery, and scope identity in receipts;
- route-bound complete-document composition with exactly one structural child
  slot per layout, deterministic head merging, and layout identity in receipts;
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

## Deliberately absent

The following remain gate work:

- layout composition for streamed modes plus layout-owned loaders and cleanup;
- OpenTelemetry with redaction and cardinality tests;
- multipart and decompression policies;
- real socket HTTP/2 conformance; and
- fixed-load latency, RSS, disconnect, slow-peer, overload, and shutdown
  evidence.
