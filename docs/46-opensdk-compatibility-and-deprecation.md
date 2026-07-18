# OpenSDK compatibility and deprecation

OpenSDK publishes a machine-readable compatibility matrix through:

```console
pliego sdk compatibility --format json
```

The output conforms to `schemas/pliego.sdk-compatibility-matrix.schema.json`. It is the public record of tested hosts, language toolchains, browser surfaces, tooling protocols, and deprecations. An integration is supported only at the boundary and version listed in this matrix.

## Stability

- `experimental`: may change without a compatibility window and is never enabled implicitly.
- `preview`: versioned and tested, but may make breaking changes in a new preview protocol.
- `stable`: preserves the contract for the documented compatibility window.

OpenSDK `0.1.0-preview.1` is preview. Process bridges for TypeScript and Python are conformance implementations, not sandboxed Component Model SDKs. Rust is the reference Component Model toolchain.

## Deprecation contract

Deprecations are explicit matrix entries. A deprecated contract must name a replacement and an `earliestRemoval` version later than the version in which deprecation began. A removed contract records its removal version. Active contracts cannot declare removal metadata.

The host validates these transitions before emitting the matrix. Schema validation alone is not treated as sufficient evidence.

Removal requires all of the following:

1. A published deprecation entry.
2. A replacement with equivalent conformance coverage.
3. At least one later OpenSDK protocol version before removal.
4. Migration documentation and a diagnostic that identifies the replacement.

Security fixes may disable unsafe behavior immediately. The contract identifier remains in the matrix with a security note until the next protocol release.

## Source boundary

The matrix source is `celiumsai/pliegors`. It never describes or requires the private implementation of `pliego.run`; a future deployment provider integrates only through public, provider-neutral contracts and conformance suites.
