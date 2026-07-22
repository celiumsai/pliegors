# Product capability manifest

**Status:** Accepted as the G0 product-truth authority on 2026-07-19  
**Machine-readable authority:** [`product.capabilities.json`](../product.capabilities.json)  
**Schema:** [`pliego.product-capabilities.schema.json`](../schemas/pliego.product-capabilities.schema.json)

PliegoRS maintains one machine-readable inventory for released, preview,
partial, planned, and external product surfaces. It exists to stop the website,
README, framework boundary, support matrix, release metadata, and CLI from
describing different products.

The manifest is published by the official website at
`https://pliegors.dev/capabilities.json`. The repository copy is authoritative;
the website asset is built from those exact bytes.

## Vocabulary

Availability and stability answer different questions.

### Availability

| Value | Meaning |
| --- | --- |
| `released` | Present in the current installable release and covered by its named evidence. |
| `source-preview` | Executable on `main`, but not part of the current released package set. |
| `partial` | A bounded subset exists; the limitation is material to the broader claim. |
| `not-released` | Design or implementation work may exist, but users cannot rely on the surface. |
| `external` | A separate product or system; it is not a PliegoRS repository capability. |

### Stability

| Value | Meaning |
| --- | --- |
| `experimental` | May change or disappear without a compatibility window. |
| `preview` | Deliberate pre-1.0 contract governed by versioning, changelog, and migration guidance. |
| `none` | No public compatibility promise because the surface is not released or is external. |

`source-preview` never means released. `partial` never implies that the missing
part is available through a private service. A Draft RFC is design evidence,
not an implementation or release claim.

## Version authority

The manifest records three distinct versions:

- `releasedVersion`: the latest complete CLI/distribution release;
- `workspaceVersion`: the version currently shared by released workspace
  packages on `main`; and
- `openSdkVersion`: the separately versioned public-preview OpenSDK protocol line.

The product-truth checker compares those values with `Cargo.toml`,
`package.json`, `rust-toolchain.toml`, the released crate set, the README,
constitution, framework contract, and official site source.

## Target authority

Target roles are explicit:

- Linux x64 and ARM64 are production release targets for `0.0.2`.
- macOS x64/ARM64 and Windows x64 are development release targets.
- Chromium is the release-blocking browser lifecycle target.
- Firefox and Safari are compatibility candidates until the same corpus runs
  in CI or committed physical-device evidence.

The released CLI archives are not evidence that a full-stack application
runtime is wired into the CLI for those targets. G1 native runtime crates are a
separate public preview; the Cloudflare application runtime remains
`not-released` until G3 closes.

## Evidence rules

Every surface names at least one repository document. Evidence is interpreted
according to availability:

- released evidence must be tied to the released contract;
- source-preview evidence must state its registry and governance boundary;
- partial evidence must state what is absent;
- not-released evidence may be a Draft RFC or threat model; and
- external evidence defines the ownership boundary, not feature completion.

The verifier rejects missing evidence paths, duplicate surface IDs, incoherent
availability/stability pairs, version drift, target drift, stale public claims,
and failure to publish the exact manifest through the official site.

## Change control

Any public capability change must update the manifest in the same revision as
its code and documentation. Promotion requires:

1. acceptance evidence at an exact revision;
2. the required gate to be closed;
3. compatibility and migration treatment;
4. security review appropriate to the surface; and
5. all product-truth checks passing.

The manifest describes current truth. It is not a roadmap and cannot promote a
surface merely because implementation files exist.

## Verification

Run:

```sh
npm run check:product-truth
```

CI runs the same command. A failed check blocks publication and any competitive
claim that depends on the inconsistent surface.
