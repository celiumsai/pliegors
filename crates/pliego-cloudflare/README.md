# pliego-cloudflare

`pliego-cloudflare` is the Cloudflare Workers adapter for PBOC. It admits an
exact `cloudflare-workers` target before upload, verifies required capabilities,
matches the sealed route table, forwards declared static assets through the
Workers Static Assets binding, and dispatches dynamic handlers written in Rust.

The adapter uses the official `workers-rs` runtime. It does not introduce a
Node.js application shim and it never places provider credentials in PBOC.
