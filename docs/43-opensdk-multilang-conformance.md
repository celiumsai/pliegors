# OpenSDK multilang conformance

**Status:** executable preview evidence

The first language-neutral build transform fixture is implemented independently
in Rust, TypeScript, and Python process bridges, plus a Rust WebAssembly
Component. Each implementation receives the same bounded request and produces
the same canonical transform response. The gate executes every implementation
twice, compares exact result bytes across all four, then checks the decoded
result.

The TypeScript fixture is explicitly transpiled with esbuild `0.28` and runs on
Node.js `20`; it does not depend on Node.js 24 native type stripping.

```powershell
npm run check:opensdk:multilang
```

The Rust Component is compiled with `wit-bindgen`, componentized, admitted by
exact digest, and invoked through `pliego:build/transformer@0.1.0`. Its gate
also proves fuel exhaustion, wall-time interruption, memory rejection, output
rejection, and a schema-valid execution receipt.

This fixture proves protocol equivalence for one transform. It does not claim
that TypeScript or Python process bridges are sandboxed Component Model guests;
they remain `native-trusted`. A third stable Component SDK still requires
hosted Component Model conformance and remains an RFC-006 acceptance item.
