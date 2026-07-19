# Native PliegoRS reference application

This unreleased G1 application exercises `pliego-router`, `pliego-runtime`,
`pliego-dom`, complete SSR, ordered sibling streaming, assets, health, and
graceful shutdown in one native process.

Run it from the workspace root:

```bash
cargo run -p native-pliego
```

The default address is `127.0.0.1:4310`. A custom loopback address may be set
with `PLIEGO_ADDR`. Binding a non-loopback address is rejected unless
`PLIEGO_EXPOSE=1` is also present.

This application is gate evidence, not a released starter. It does not yet
prove middleware, authored error boundaries, asynchronous boundaries, HTTP/2,
TLS, OpenTelemetry, multipart/decompression policy, fixed-load behavior, or
production deployment.
