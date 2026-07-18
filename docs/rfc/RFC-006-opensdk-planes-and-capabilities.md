# RFC-006: OpenSDK planes and capability model

**Status:** Draft
**Target:** `0.1.0-preview`
**Authors:** Celiums Solutions LLC
**Last updated:** 2026-07-18

## Summary

PliegoRS exposes four extension planes through public, language-neutral
contracts. Build and server extensions use WebAssembly Component Model worlds;
browser extensions use ESM plus Custom Elements and the Pliego lifecycle;
tooling uses transport-neutral JSON-RPC 2.0. Every extension is rejected before
execution unless identity, byte digest, API compatibility, requested powers,
resource budgets, and required features are admitted by the host.

Rust is the reference host implementation. It is not a participation
requirement.

## Motivation

Traditional plugin APIs commonly inherit the permissions of the build process
or application server. That gives adoption speed at the cost of an invisible
security and reproducibility boundary. It also couples integrations to a host
language or undocumented internals.

OpenSDK instead makes five properties inspectable:

1. what extension is executing;
2. which exact bytes were admitted;
3. which host and protocol versions were negotiated;
4. what the extension can observe or mutate;
5. which non-deterministic effects occurred.

## Normative language

The key words MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are to be
interpreted as described by RFC 2119.

## Four planes

| Plane | Entry kind | Boundary | Initial lifecycle |
| --- | --- | --- | --- |
| Build | `wasm-component` | `pliego:build@0.1.0` | init, transform, dispose |
| Server | `wasm-component` | `pliego:http@0.1.0` | init, handle, suspend, resume, dispose |
| Browser | `browser-esm` | Custom Element plus adapter API v1 | mount, update, suspend, resume, unmount, HMR |
| Tooling | `json-rpc-process` | JSON-RPC 2.0 over stdio or another declared transport | handshake, request, cancel, shutdown |

An entry kind is valid in exactly one plane during preview. A future version
may define bridges, but it MUST negotiate them as features rather than silently
changing execution semantics.

## Package identity and resolution

The initial WIT packages are:

```text
pliego:manifest@0.1.0
pliego:build@0.1.0
pliego:http@0.1.0
pliego:effects@0.1.0
pliego:diagnostics@0.1.0
pliego:component@0.1.0
pliego:deploy@0.1.0
```

WIT defines interfaces and worlds, not package distribution. PliegoRS therefore
MUST resolve every package to an exact digest before execution and MUST write
that resolution into build evidence. Mutable aliases are not accepted as
release inputs.

## Admission order

The host MUST perform the following order without invoking guest code:

1. parse the manifest with unknown fields denied;
2. validate schema, identity, normalized entry path, budgets, and sorted sets;
3. negotiate exact OpenSDK API version and host SemVer range;
4. verify all required features and choose supported optional features;
5. hash the entry bytes and compare the declared SHA-256 digest;
6. inspect component imports or browser declarations;
7. require requested capabilities to equal or exceed those imports;
8. intersect requested capabilities with explicit project/operator grants;
9. emit an admission receipt;
10. construct and invoke the runtime.

Failure in steps 1-9 MUST leave the guest unexecuted.

## Capability model

No extension receives ambient filesystem, network, environment, clock, random,
HTTP, DOM, media, animation, GPU, or high-frequency frame access. The initial
capability vocabulary is the enum in
[`pliego.sdk-extension.schema.json`](../../schemas/pliego.sdk-extension.schema.json).

Project policy grants are an upper bound. A manifest request never grants
itself power. Host implementations MUST NOT translate a broad request such as
`filesystem-read` into access to the project root without an independently
declared resource scope.

Component Model imports are a second, structural boundary: a component without
an import cannot call that interface. Manifest capabilities provide the human
and policy-facing boundary. Both MUST agree.

During preview, any `wasi:filesystem/*` import is conservatively classified as
both `filesystem-read` and `filesystem-write`, because the shared types
interface can expose both classes of operation. A future scoped filesystem
world MAY refine this only through a new negotiated contract.

## Determinism classes

| Class | Rule |
| --- | --- |
| `pure` | Requests no capabilities and produces output only from declared input bytes and options. |
| `recorded-effect` | Uses `pliego:effects/broker`; every non-deterministic input and output is digest-bound in an ordered receipt. |
| `native-trusted` | Executes outside the sandbox only under explicit operator policy and is never treated as reproducible evidence. |

The preview host rejects a `pure` extension with any requested capability. A
`recorded-effect` extension MUST request `effect-broker`.
An executor attempt MUST emit a receipt whether it succeeds or fails. The
receipt records `outcome` and hashes the success payload or failure value.
Policy denials happen before the executor and MUST NOT fabricate an effect
receipt.
Operation names MUST be bounded visible ASCII. Request, response, and failure
values MUST be bounded by the admitted runtime budget, and the preview host MUST
stop before retaining more than 4096 effect receipts for one execution.

## Budgets

CPU, wall time, linear memory, and output bytes are mandatory. Preview ceilings
are intentionally conservative and encoded in both the JSON Schema and Rust
validator. Hosts MAY apply lower policy ceilings. Exceeding a budget terminates
the guest and produces a structured diagnostic; partial output is not
publishable.

## Browser contract

A browser module MUST register one Custom Element name declared by its
descriptor. Pliego owns admission, lazy activation, reduced-motion and
Save-Data policy, abort, HMR ordering, and cleanup. The component owns only its
DOM scope and explicitly registered effects.

The descriptor field is `entry.customElement`. It MUST match the module's
exported `pliegoComponent.tagName`, use a non-reserved lowercase Custom Element
name, and remain stable for the admitted extension version. Preview lifecycle
contracts require init and dispose, require suspend/resume as a pair, and allow
HMR only when browser update is also declared.

The existing adapter runtime remains the preview execution mechanism. React,
Svelte, Lit, vanilla JavaScript, and Rust/WASM adapters MUST pass the same
lifecycle suite. A framework wrapper is an authoring convenience, not a new
runtime contract.

## Tooling contract

Tooling begins with a JSON-RPC 2.0 `pliego/handshake` request. The result states
protocol version, host version, supported methods, and features. Requests with
an incompatible protocol fail before any project method runs. Notifications do
not produce responses. MCP and editor integrations are clients of this same
host, not privileged backdoors.

## Compatibility and deprecation

- API versions use SemVer including prerelease identifiers.
- The preview requires an exact `apiVersion` because prerelease compatibility
  is not implied by SemVer.
- Host support is a SemVer range declared by the extension.
- Additive optional features require negotiation; their mere presence cannot
  alter old behavior.
- A stable field or capability may be removed only after one minor release of
  deprecation diagnostics and a documented replacement.
- Unknown schemas, capabilities, entry kinds, required features, or lifecycle
  values fail closed.

## Security considerations

WASI is not a permission grant. The host constructs only the interfaces and
resource handles admitted by policy. Host environment variables, inherited
stdio, current directory, network sockets, clocks, and randomness are absent by
default. Component execution uses an isolated Wasmtime engine and a deadline
that covers instantiation plus invocation, so an expired epoch cannot interrupt
an unrelated concurrent extension. Process bridges are `native-trusted` during
preview and cannot satisfy sandbox conformance.

## Required conformance evidence

The preview cannot graduate until:

- denied filesystem, network, and environment attempts fail without invoking
  their executor;
- incompatible extensions fail before guest initialization;
- one transform produces equivalent bytes under Rust, TypeScript, and a third
  toolchain;
- adversarial disposal leaves zero listeners, timers, scopes, workers, or GPU
  contexts;
- admission and effect receipts validate against their public schemas.

## References

- [WIT packages](https://component-model.bytecodealliance.org/design/packages.html)
- [WIT worlds and strict imports](https://component-model.bytecodealliance.org/design/worlds.html)
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
- [External adapter contract](../12-external-adapters.md)

## Unresolved before acceptance

- exact component package distribution and lockfile format;
- filesystem and network resource-scope grammar;
- stable runtime fuel-to-CPU accounting across architectures;
- the third reference SDK selected by hosted conformance evidence.
