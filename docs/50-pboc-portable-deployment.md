# PBOC portable deployment preview

**Status:** G3 public preview in `0.3.0-beta.1`  
**Contract:** `dev.pliegors.pboc/v1alpha1`  
**MSRV:** Rust 1.86  
**Reference hosts:** native Linux/OCI and Cloudflare Workers

The Pliego Build Output Contract (PBOC) is the public boundary between an
application build and a deployment host. One sealed bundle contains one
canonical manifest, exact artifacts, route and runtime identities, capability
requirements, cache policy, secret references, telemetry hooks, compatibility
metadata, SBOM, and provenance. Provider credentials and control-plane state do
not belong in that bundle.

PBOC is a preview contract. It proves portability across the two maintained G3
hosts without claiming that every cloud feature has a portable equivalent.

## Install

Keep the coordinated PliegoRS package graph on one exact version:

```toml
[dependencies]
pliego-pboc = "=0.3.0-beta.1"
pliego-runtime = "=0.3.0-beta.1"

# Only when the application builds a Cloudflare Worker.
pliego-cloudflare = "=0.3.0-beta.1"
```

The CLI validates bundles and host admission locally:

```sh
pliego pboc validate target/app/pliego.pboc.json --root target/app
pliego pboc admit target/app/pliego.pboc.json \
  --host native \
  --root target/app
pliego pboc admit target/app/pliego.pboc.json \
  --host cloudflare \
  --root target/app
```

`validate --root` rejects missing, additional, modified, linked, or
non-portable files. `admit` repeats semantic validation, selects the exact host
target, negotiates required and optional features, applies host byte limits,
and returns a JSON admission receipt before upload.

## Bundle layout

The reference corpus produces:

```text
pliego.pboc.json
artifacts/
  native/server
  cloudflare/index.js
  cloudflare/index_bg.wasm
  cloudflare/package.json
  cloudflare/worker/pboc.mjs
  cloudflare/worker/shim.mjs
public/
  _headers
  asset.txt
supply/
  sbom.spdx.json
  provenance.intoto.json
```

The manifest is not included in its own artifact ledger. Its SHA-256 is the
deployment identity, while `artifactLedgerSha256` binds the ordered artifact
records. Every listed artifact records path, length, SHA-256, role, and media
type. SBOM and provenance are ordinary required artifacts and cannot point
outside the exact set.

## Manifest model

| Section | Authority |
| --- | --- |
| `framework` | PliegoRS version and exact source revision |
| `build` | Application/release identity, graph/runtime digests, ledger and provenance |
| `compatibility` | Epoch, monotonic sequence, state schema, previous release, rollback declaration |
| `capabilities` | Global required or optional semantics with version numbers |
| `artifacts` | Every deployable byte and its exact identity |
| `targets` | Host kind, target artifacts, required and optional features |
| `assets` | Request path, artifact path, cache policy, immutability |
| `routes` | Method, canonical pattern, kind, renderer and required features |
| `functions` | Entrypoint, render modes, response budget, secret and permission handles |
| `cachePolicies` | Domain, revalidation, TTL/stale policy, Vary fields and tags |
| `permissions` | Named resources and requested capabilities; never self-grants |
| `secretReferences` | Identifier, purpose and required flag; never a secret value |
| `telemetryHooks` | Named signal, requirement and explicit redaction fields |

The JSON Schema rejects unknown fields. Rust validation additionally enforces
canonical ordering, exact references, portable identifiers and paths, route
shape, public/private cache invariants, feature closure, and the secret-value
boundary.

## Native and OCI

`pliego-runtime` exposes `native_pboc_host_profile` and verifies route graph and
runtime contract digests before a `NativeRuntime` serves traffic. The reference
OCI artifact is a Rust 1.86 `x86_64-unknown-linux-musl` executable in a pinned
`distroless/static` image. Its conformance invocation is deliberately strict:

```sh
docker run --rm \
  --read-only \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --tmpfs /tmp:rw,noexec,nosuid,nodev,size=16m \
  -p 127.0.0.1:4330:4330 \
  your-application-image
```

The image runs as `65532:65532`. TLS termination, trusted proxy topology,
container scheduling, registry policy, persistent stores, and provider
credentials remain operator responsibilities.

## Cloudflare Workers

`pliego-cloudflare` is implemented in Rust with `workers-rs`. An application
constructs one `Application`, registers exactly the functions declared by its
PBOC, maps required secret references to Worker bindings, and seals the
registry. Admission fails before request handling when a graph digest, runtime
digest, capability, function, or required secret mapping differs.

Static routes pass to the declared Cloudflare Assets binding. Dynamic routes
use the same `PbocRouter` as the native host. Complete and ordered streaming
responses remain native Worker responses; the adapter does not hide a
stream-to-buffer downgrade.

The canonical PBOC is imported as a Worker text module from inside the verified
bundle. It is not copied into a size-limited environment variable or mutable
KV record. Provider account IDs, API tokens, zones, domains, billing, rollout
policy, and observability destinations stay in operator configuration. The
reference deployment script generates an ignored Wrangler configuration that
points only to files inside an already verified bundle.

## Rolling updates and rollback

Check a promotion before changing traffic:

```sh
pliego pboc compatibility \
  active/pliego.pboc.json \
  candidate/pliego.pboc.json \
  --direction rolling
```

In `v1alpha1`, rolling coexistence requires:

- one application identity;
- one compatibility epoch;
- one state schema;
- a higher candidate sequence; and
- `candidate.previousReleaseId == active.releaseId`.

Check rollback in the opposite direction:

```sh
pliego pboc compatibility \
  candidate/pliego.pboc.json \
  active/pliego.pboc.json \
  --direction rollback
```

Rollback additionally requires the active release to declare `rollbackSafe`,
the target sequence to be lower, and the release chain to be exact. The
declaration is meaningful only when the application's state migration and
provider store have passed their own tests. PBOC never fabricates rollback for
an irreversible business or database mutation.

## Stable diagnostics

| Code | Meaning |
| --- | --- |
| `PLG-PBOC-100` | One manifest in a compatibility check is invalid |
| `PLG-PBOC-101` | Application identities differ |
| `PLG-PBOC-102` | Compatibility epochs differ |
| `PLG-PBOC-103` | State schemas differ during coexistence |
| `PLG-PBOC-104` | Release sequence moves in the wrong direction |
| `PLG-PBOC-105` | Previous-release chain is not exact |
| `PLG-PBOC-106` | The active release does not declare rollback safe |
| `PLG-CF-002` | Cloudflare function registry differs from PBOC |
| `PLG-CF-003` | Required secret reference has no binding map |
| `PLG-CF-004` | Mapped Worker secret binding is unavailable |
| `PLG-CF-005` | Route graph digest differs |
| `PLG-CF-006` | Runtime contract digest differs |

CLI failures use exit code `10`. Routing retains the common `PLG-RTE-404` and
`PLG-RTE-405` codes and preserves the HTTP `Allow` header across hosts.

## Conformance

Run the complete Linux gate from a clean checkout with Docker available:

```sh
npm ci
npm ci --prefix examples/provider-tck
npm run check:provider-tck
```

The gate builds with Rust 1.86, seals r1 and r2, validates both hosts before
upload, rejects unsupported features and incompatible state, scans all bundle
bytes for a provider-secret sentinel, runs both versions concurrently, checks
rolling skew and rollback, builds the rootless OCI image, and replays the same
HTTP corpus against native/OCI and Cloudflare. CI preserves the machine-readable
receipt and service logs as the `g3-provider-conformance` artifact.

See [G3 acceptance evidence](evidence/g3-pboc-provider-conformance.md),
[RFC-007](rfc/RFC-007-pliego-build-output-contract.md), and the
[G3 ASVS ownership map](../security/asvs-v5.0.0-g3.json).

## Current limits

- The PBOC schema is `v1alpha1`, not a stable 1.0 contract.
- Native Linux/OCI and Cloudflare Workers are the only maintained hosts.
- Portable queues, schedules, object storage, databases, and durable objects
  are not defined by this contract.
- The Cloudflare adapter covers complete and ordered responses; asynchronous
  boundary parity remains a later conformance expansion.
- Cache declarations are portable, but production distributed cache and
  invalidation providers remain separate integrations.
- PBOC describes and verifies application output; it is not a hosted control
  plane, image registry, TLS authority, billing system, or secret manager.
