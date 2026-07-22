# Provider TCK reference application

This application is the executable G3 corpus. One route graph and one runtime
contract are packaged into one PBOC bundle with two target units:

- `native-linux-oci`, executed by `provider-tck-native`;
- `cloudflare-workers`, executed by the Rust `workers-rs` module.

Both targets expose the same static asset, complete dynamic response, ordered
stream, health response, method rejection, not-found behavior, release identity,
and bounded error codes. `provider-tck-pack` seals every byte, writes
`pliego.pboc.json`, and verifies the exact bundle before it can be deployed.

The npm package is private and repository-local. It exists only to pin Wrangler
and `worker-build`; no first-party package is published to the npm registry.
