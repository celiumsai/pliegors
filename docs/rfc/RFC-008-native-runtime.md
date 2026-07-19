# RFC-008: Native HTTP runtime and dynamic rendering

**Status:** Draft  
**Owner:** Runtime  
**Target gate:** G1  
**Created:** 2026-07-19

## Summary

PliegoRS will add a production server runtime without turning the CLI static
preview server into an application server. The reference native host will use
Tokio as the executor, Hyper as the HTTP implementation, Tower for middleware
and load control, and Axum for routing interoperability. PliegoRS will own the
route, rendering, lifecycle, cache, effect, receipt, and diagnostic semantics.

The runtime is not released until request limits, cancellation, streaming,
graceful shutdown, observability, and failure behavior pass one public
conformance corpus under load.

## Motivation

The released framework produces deterministic static output and focused
Rust/WASM browser experiences. `tiny_http` serves that output in `pliego dev`
and `pliego preview`; it is not a production HTTP contract. Converting that
server in place would mix developer convenience, application authority, and
network attack surface.

The native runtime must preserve the framework's existing properties:

- explicit ownership and cleanup;
- bounded inputs and outputs;
- stable diagnostics;
- capability admission;
- replayable evidence; and
- provider-neutral output.

## Goals

- HTTP/1.1 and HTTP/2 through the upstream Hyper stack.
- Bounded request heads and bodies before application code runs.
- Request-scoped cancellation and deadlines propagated to all owned work.
- Synchronous, asynchronous, and streamed response bodies.
- Backpressure without hidden conversion of streams into complete buffers.
- Graceful shutdown with an explicit drain deadline.
- Tower-compatible middleware and an Axum interoperability boundary.
- OpenTelemetry HTTP spans and metrics using standard semantic conventions.
- Structured, stable PliegoRS diagnostics and request receipts.
- A native Linux deployment path that does not require Pliego.run.

## Non-goals

- A custom TCP stack, HTTP parser, TLS implementation, or async executor.
- An ORM, authentication provider, queue, scheduler, or database.
- React Server Components compatibility.
- Partial Prerendering before dynamic SSR and cache correctness are complete.
- Hiding provider limitations behind a false lowest-common-denominator API.
- Treating OpenSDK's current buffered `pliego:http` WIT as stable.

## Crate boundary

G1 introduces two public ownership boundaries:

| Crate | Owns | Does not own |
| --- | --- | --- |
| `pliego-router` | Route graph, path grammar, matching, route IDs, parameters, method dispatch, middleware order, error boundaries | Sockets, body transport, executor |
| `pliego-runtime` | Request scope, limits, cancellation, rendering, response streaming, diagnostics, runtime receipt, graceful shutdown | TLS implementation, database, hosted deployment control plane |

The CLI may invoke these crates for `pliego serve`, but it does not absorb their
public contracts. `pliego dev` and `pliego preview` retain their static-output
meaning until a separately documented migration promotes a production command.

## Request lifecycle

Every admitted request follows one state machine:

```text
accepted -> head-admitted -> route-resolved -> scope-open
         -> handler-running -> response-committed -> body-streaming
         -> scope-draining -> closed
```

Any transition may instead enter `rejected`, `cancelled`, or `failed`. A state
change emits a stable diagnostic code and a low-cardinality span event. A
response commitment is irreversible: later failures terminate the stream and
record an error, but cannot fabricate a second status line.

`RequestScope` owns:

- a cancellation token;
- the effective deadline;
- stable request, route, deployment, and trace IDs;
- request-local resources and cleanup stack;
- response commitment state;
- effect and cache receipts; and
- a bounded diagnostic collector.

Owned work must either finish or observe cancellation before the scope closes.
Detached work requires an explicit application-owned background resource and
is outside the request scope.

## Limits and overload

The runtime exposes typed policies for:

- request-line and normalized path bytes;
- header count, name/value bytes, and aggregate head bytes;
- encoded and decoded body bytes;
- multipart part count and per-part bytes;
- request deadline;
- concurrent requests and streams;
- render depth and output bytes;
- diagnostic and receipt count; and
- graceful-shutdown drain time.

Defaults are conservative and versioned. Operators may tighten them. Raising a
limit requires an explicit configuration value and appears in the runtime
receipt. Queue bounds and overload responses are configured through Tower load
control; unbounded queues are forbidden.

## Cancellation and disconnects

Cancellation is contagious. Client disconnect, deadline expiry, shutdown, or
an application abort cancels:

- loaders and route handlers;
- rendering boundaries;
- OpenSDK calls;
- cache fills owned by the request unless promoted to a bounded shared task;
- response body producers; and
- registered cleanup callbacks.

Dropping a future is not accepted as proof of cleanup. The conformance suite
observes cancellation acknowledgement, resource release, and memory plateau.

## Rendering

The renderer supports three explicit modes:

1. `complete`: render a bounded response before commitment;
2. `ordered`: stream in document order; and
3. `boundary`: stream stable placeholders and resolve declared async
   boundaries without changing document semantics.

Every mode defines head commitment, error behavior, cancellation, and cache
eligibility. A host that cannot preserve a mode must reject deployment or
report it unsupported; it may not silently buffer a stream.

Useful HTML remains the baseline. Client enhancement is optional and cannot be
required for error pages, form submission acknowledgement, or navigation to
rendered content unless the application explicitly opts out of progressive
behavior.

## Middleware

Middleware is ordered by the sealed route graph. Four phases are distinct:

1. transport admission owned by the host;
2. pre-route application middleware;
3. route middleware and handler;
4. response middleware before commitment.

Security admission, body limits, tracing, request IDs, compression, and timeout
layers have documented ordering constraints. A plugin cannot insert itself
before host admission.

## Observability

The runtime uses OpenTelemetry HTTP semantic conventions for server spans and
metrics. PliegoRS adds only namespaced, low-cardinality attributes such as:

- `pliego.route.id`;
- `pliego.render.mode`;
- `pliego.cache.outcome`;
- `pliego.action.id`; and
- `pliego.runtime.receipt`.

Paths, query values, secrets, cookies, request bodies, user identifiers, and
unbounded error text are excluded by default. Export is operator-configured;
the framework does not ship a mandatory collector.

## Runtime receipt

One bounded receipt records the facts needed to explain a request without
storing its private payload:

- contract and runtime versions;
- route and deployment IDs;
- effective limit policy digest;
- render mode;
- cache and effect receipt digests;
- lifecycle outcome and diagnostic codes;
- response status class and byte counts; and
- timing buckets suitable for local diagnosis.

The receipt is evidence, not authentication. Signing and retention belong to
the deployment boundary.

## OpenSDK server boundary

The native host will not stabilize a second HTTP model. Standard WASI HTTP is
the preferred transport semantic where the Component Model toolchain can
preserve streaming and cancellation. PliegoRS-specific WIT is limited to
capabilities, cache hints, trace context, lifecycle, and receipts.

The existing `pliego:http@0.1.0` world remains experimental because
`list<u8>` request and response bodies require complete buffering. A host must
never advertise streaming conformance while using that contract.

## Security and failure requirements

RFC-008 is governed by the [full-stack threat model](../48-fullstack-threat-model.md).
Before preview release, the runtime must demonstrate:

- bounded behavior under slow request and slow response peers;
- rejection of ambiguous and invalid request targets;
- encoded and decoded body limits;
- panic isolation where the target supports unwinding;
- cancellation and cleanup after disconnect;
- no secret or body value in default telemetry;
- overload behavior without unbounded memory growth; and
- graceful shutdown under active streams.

## Acceptance evidence

G1 closes only when one dynamic reference application passes:

- native Linux build and launch from a sealed source revision;
- route, middleware, error, and streaming conformance;
- fixed-load p50/p95/p99 latency and peak RSS reporting;
- slowloris, oversized body, decompression, disconnect, and overload cases;
- cancellation acknowledgement and cleanup plateau;
- OpenTelemetry trace/metric shape validation;
- zero unresolved P0 security finding; and
- a reproducible runtime receipt and diagnostics corpus.

## Alternatives rejected

- **Grow `tiny_http`:** rejected because it would make the development file
  server carry a production runtime contract.
- **Write the network stack in PliegoRS:** rejected because it adds risk without
  differentiating application semantics.
- **Use only Axum types as the public API:** rejected because PliegoRS must keep
  route, rendering, receipt, and portability semantics host-neutral.
- **Stabilize the current HTTP WIT first:** rejected because its buffered model
  cannot express the lifecycle required by this RFC.
