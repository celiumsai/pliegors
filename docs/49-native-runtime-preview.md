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
diagnostics, receipts, and the first complete server-rendering mode. Axum,
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

## Runtime limits

`RequestLimits` currently bounds:

- request target bytes;
- header count and aggregate header bytes;
- declared and streamed request-body bytes;
- response bytes;
- diagnostics and application cleanup callbacks;
- concurrent admitted requests;
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
- exactly-once bounded receipts; and
- pre-commit complete-render failures with stable `PLG-REN-*` diagnostics.

## Deliberately absent

There is no public ordered or boundary streaming API yet. The current
`pliego-dom` walker returns one validated bounded string; slicing that string
into chunks would still be buffering and is not described as streaming.

The following remain gate work:

- a backpressure-aware renderer sink;
- ordered shell/body emission;
- declared asynchronous boundaries;
- middleware and authored error-boundary graphs;
- OpenTelemetry with redaction and cardinality tests;
- multipart and decompression policies;
- real socket HTTP/1.1 and HTTP/2 conformance; and
- fixed-load latency, RSS, disconnect, slow-peer, overload, and shutdown
  evidence.
