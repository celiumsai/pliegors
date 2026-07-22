# Native PliegoRS reference application

This G1 reference application exercises the public-preview `pliego-router` and
`pliego-runtime` crates,
`pliego-dom`, complete SSR, ordered sibling streaming, bounded asynchronous
boundary SSR, assets, health, and graceful shutdown in one native process. Its sealed graph also declares a
response-policy middleware and a safe root error boundary.
The graph and runtime registry both declare its
`mutate-response-headers` capability; a mismatch prevents startup.
The `canonical-entry` pre-route layer rewrites `/start` to the sealed `/`
route before matching and declares only `rewrite-path`.
The `/` and `/hello/:name` pages also prove layout-owned complete-document
composition: the sealed graph names `document-layout`, the runtime admits one
matching structural child slot, and the receipt records that layout identity.

Run it from the workspace root:

```bash
cargo run -p native-pliego
```

The default address is `127.0.0.1:4310`. A custom loopback address may be set
with `PLIEGO_ADDR`. Binding a non-loopback address is rejected unless
`PLIEGO_EXPOSE=1` is also present.

This application is gate evidence, not a released starter. The wider runtime
corpus proves HTTP/2, operator-enabled OpenTelemetry, fail-closed multipart and
decompression policy, and fixed-load behavior. TLS, proxy trust, G3 host
portability, and production deployment remain outside this example.
