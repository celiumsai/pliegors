# pliego-runtime

`pliego-runtime` is the unreleased G1 native runtime for PliegoRS. It owns
bounded request admission, request scope, cancellation, deadline propagation,
concurrency admission, LIFO cleanup, response commitment, streamed-body
ownership, panic isolation, graceful-shutdown draining, diagnostics, and
runtime receipts on top of Axum, Hyper, Tower, and Tokio.

The crate is `0.1.0-preview.1` source work. It is not published on crates.io and
does not promote the `native-http-runtime` or `dynamic-ssr` capabilities in
`product.capabilities.json`. See
[`RFC-008`](../../docs/rfc/RFC-008-native-runtime.md).

This foundation is intentionally incomplete. It does not yet expose PliegoRS
server rendering, middleware phases, OpenTelemetry, multipart/decompression
policies, or a production `pliego serve` command.
