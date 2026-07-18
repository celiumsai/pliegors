# ADR-006: OpenSDK Wasmtime security floor

**Status:** Proposed
**Date:** 2026-07-18
**Scope:** OpenSDK preview and subsequent PliegoRS releases

## Context

The initial OpenSDK host prototype used Wasmtime `34.0.2` to preserve the Rust
`1.85.0` MSRV. A current RustSec audit reports multiple advisories against that
runtime, including critical sandbox-escape classes. OpenSDK executes untrusted
Component Model guests, so an advisory exception is not an acceptable release
strategy.

Wasmtime `36.0.8` is the earliest maintained patch line that resolves the full
reported set and requires Rust `1.86.0`. PliegoRS `v0.0.2` was built and sealed
before OpenSDK and keeps its existing Rust `1.85.0` evidence.

## Decision

1. The OpenSDK host uses exactly Wasmtime `36.0.8` with only `component-model`,
   `cranelift`, `runtime`, and `std` features.
2. The next PliegoRS release line declares Rust `1.86` and CI uses exactly
   `1.86.0`.
3. Release gates run `cargo audit` against the workspace and standalone guest
   tool locks. Known runtime vulnerabilities are release blockers.
4. Winch, pooling allocation, ambient WASI, threads, and default Wasmtime
   features remain disabled unless a later RFC admits and tests them.
5. Preview executions use an isolated engine, finite fuel, store limits, and an
   epoch deadline covering instantiation and invocation. Sharing one epoch
   across concurrent untrusted extensions is forbidden because one timeout
   could otherwise trap an unrelated store.

## Consequences

The compiler floor rises by one Rust release. In return, the OpenSDK preview
does not ship a sandbox with known critical vulnerabilities. Future MSRV holds
are subordinate to security fixes on the component runtime boundary.

This ADR remains Proposed until the OpenSDK RFC acceptance review. The code and
CI may implement the security floor before that review because reverting to the
vulnerable runtime is not a valid release option.
