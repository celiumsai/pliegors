# OpenSDK foundation

**Stability:** experimental preview
**Protocol:** `0.1.0-preview.1`
**RFC:** [RFC-006](rfc/RFC-006-opensdk-planes-and-capabilities.md)

**MSRV:** Rust `1.86.0`. This is the minimum supported Wasmtime `36.0.8`
security floor for the OpenSDK host; older PliegoRS `0.0.2` artifacts retain
their original Rust `1.85.0` evidence.

OpenSDK is the public extension boundary of PliegoRS. The Rust crate, WIT
packages, JSON schemas, browser lifecycle, and tooling protocol describe the
same admission model: identity and permissions are explicit, compatibility is
negotiated, and an extension does not execute until its exact bytes pass policy.

## Repository map

| Path | Authority |
| --- | --- |
| `crates/pliego-sdk` | Rust reference validator, policy, effect broker, and JSON-RPC host |
| `crates/pliego-sdk/wit/*` | Packaged Component Model interfaces and worlds |
| `schemas/pliego.sdk-extension.schema.json` | Author-facing extension manifest |
| `schemas/pliego.sdk-admission.schema.json` | Successful pre-execution admission evidence |
| `schemas/pliego.effect-receipt.schema.json` | Ordered brokered-effect evidence |
| `schemas/pliego.build-transform-receipt.schema.json` | Fuel, memory, output, and digest evidence for a build transform |
| `crates/pliego-adapters` | Browser execution and cleanup contract |

## Host admission

```rust
use pliego_sdk::{CapabilityPolicy, ExtensionManifest, HostContract};
use semver::Version;

let manifest: ExtensionManifest = serde_json::from_slice(manifest_bytes)?;
let host = HostContract::preview(
    Version::parse("0.1.0-preview.1")?,
    CapabilityPolicy::deny_all(),
);
let validated = host.admit(manifest, component_bytes)?;
```

`ValidatedExtension` cannot be constructed directly. This typestate is the
handoff to a runtime: a rejected digest, host range, API version, required
feature, budget, or capability never reaches guest initialization.

Browser entries also declare their exact `customElement`. The validator rejects
reserved or non-lowercase names and requires coherent init, update, suspend,
resume, dispose, and HMR combinations. The CLI opens every entry path one
directory handle at a time without following intermediate or final symlinks.

## Effect broker

Non-deterministic work is passed through `EffectBroker`. The broker requires
both `effect-broker` and the specific effect capability. It checks policy before
calling the executor. Every call that reaches the executor records an ordered
SHA-256 receipt with `outcome: success|error`; denials occur before execution and
therefore do not claim that an effect happened. Failed WIT calls return the same
receipt to the guest instead of losing evidence on the error path.
Operations are bounded visible ASCII, request/response/failure values are gated
by the declared output budget, and one execution may retain at most 4096 effect
receipts in host memory. Limit or policy failures occur before the executor;
an unwinding host executor is converted into a receipted failure at the boundary.

The Wasmtime host links only `pliego:effects/broker@0.1.0`. It never links
filesystem, sockets, environment, clocks, random, or HTTP as ambient WASI.
The application-supplied executor is the resource boundary and must authorize
the concrete operation before performing it. A manifest capability is only a
request; it does not manufacture an executor or a resource handle.

## Executable component test

`sdk test` can invoke the typed `pliego:build/transformer@0.1.0` world:

```console
pliego sdk test pliego-extension.json --input transform-input.json --format json
```

The input follows `BuildTransformInput`. The host verifies exact component
bytes, instantiates without ambient WASI, calls the WIT export, interrupts on
wall-time expiry, exhausts finite fuel, allows one budgeted linear memory, limits
output bytes, and emits `dev.pliegors.build-transform-receipt/v1`. Instantiation
and invocation share the deadline. Each execution gets an isolated Wasmtime
engine so one expired epoch cannot interrupt a concurrent extension.

`inputSha256` and `outputSha256` use deterministic length-prefixed framing. The
output digest binds media type, body bytes, and diagnostics JSON, not only the
body. Paths are normalized relative slash paths; options must be JSON and
diagnostics must be a JSON array before a receipt can be emitted.

## Current preview boundary

The preview includes manifest admission, Component Model inspection, a typed
Wasmtime transform host, effect-broker linking, resource limits, multilang
fixtures, React/Svelte/Lit lifecycle conformance, JSON-RPC tooling, and the MCP
reference client. These surfaces remain preview until RFC-006 is accepted and
the hosted cross-platform evidence closes; none is labeled stable.
