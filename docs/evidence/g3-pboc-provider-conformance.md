# G3 PBOC and provider conformance evidence

**Gate:** G3 portable deployment  
**Status:** Release candidate; protected CI and public prerelease promotion pending  
**Date:** 2026-07-22  
**Contract:** `dev.pliegors.g3-provider-evidence/v1`

## Acceptance matrix

| ID | Requirement | Evidence | Result |
| --- | --- | --- | --- |
| G3-A01 | Exact PBOC validation before upload | `pliego pboc validate --root`, `verify_bundle`, extra/missing/modified/symlink rejection | PASS locally |
| G3-A02 | Native and Cloudflare execute one sealed build | Seven-case `provider-tck` corpus over the same PBOC SHA | PASS locally |
| G3-A03 | Static assets and cache policy agree | `/asset.txt` bytes, media type, immutable cache and `nosniff` headers | PASS locally |
| G3-A04 | Complete and ordered responses agree | Home, parameter route, health and unbuffered three-frame stream | PASS locally |
| G3-A05 | Routing failures agree | Bounded `PLG-RTE-404`; `PLG-RTE-405` with canonical `Allow: GET` | PASS locally |
| G3-A06 | Required features fail before upload | Unknown required feature rejected during host admission | PASS locally |
| G3-A07 | Rolling versions are explicit | r1 and r2 coexist with one app, epoch and state schema plus exact previous-release chain | PASS locally |
| G3-A08 | Rollback fails closed | Lower target sequence, exact chain and `rollbackSafe` required; incompatible state and unsafe rollback rejected | PASS locally |
| G3-A09 | Provider secrets stay outside output | Closed schema plus recursive sentinel scan across every bundle file | PASS locally |
| G3-A10 | OCI is portable and least privilege | Static musl binary, pinned distroless base, nonroot, read-only, no capabilities, no-new-privileges | PASS locally |
| G3-A11 | MSRV and WASM targets compile | Rust 1.86 native/musl plus `wasm32-unknown-unknown` workers-rs build | PASS locally |
| G3-A12 | Gate is reproducible in protected CI | Separate `g3-provider-conformance` job uploads JSON receipt and logs | PENDING protected run |
| G3-A13 | Real Cloudflare edge executes the bundle | Wrangler deployment from sealed bundle and public edge replay | PASS platform proof; final release attestation pending |

The local acceptance run used an uncommitted development revision and is not a
release attestation. Its measured observations established the harness:

- 10 declared artifacts plus the PBOC manifest;
- approximately 2.9 MB of sealed application bytes with the musl server;
- a 2.07 MB OCI image;
- 7 equivalent request cases per provider pair; and
- zero provider-secret sentinel occurrences across every declared bundle file.

The real-edge platform proof deployed the exact bundle through Wrangler to
`pliegors-provider-tck-g3.mario-77c.workers.dev` (Cloudflare version
`86ad3cbd-62b1-4bd9-8353-369e7f893ce0`). Its seven request cases matched the
least-privilege OCI host at PBOC SHA-256
`ca3eb6fb8e672605312c89f4292ba1f3eaf276331cb6814d0543fee45bec7cc0`.
Because the source revision predates the final protected merge, this proves the
provider path but is not the release attestation.

Authoritative hashes are generated from the protected commit by
`scripts/run-provider-tck.mjs`; they are not copied from a developer working
tree into this document.

## Maintained commands

```sh
npm run check:pboc
npm run check:security-map
npm run check:provider-tck
cargo test -p pliego-pboc --test contract --locked
cargo clippy --target wasm32-unknown-unknown \
  -p pliego-cloudflare -p provider-tck --lib --locked -- -D warnings
```

`npm run check:provider-tck` produces `target/provider-tck/evidence.json`. The
receipt binds the exact source revision, r1/r2 bundle receipts, four host
admissions, rolling and rollback receipts, negative cases, secret scan, both
provider matrices, skew replay, rollback replay, and OCI posture.

## Security boundary

- PBOC can request a capability but cannot grant it.
- A host rejects unknown required semantics before artifact upload.
- Secret references contain no values and are mapped by the operator after
  portable admission.
- The Worker and native runtime both verify route and runtime identities.
- Provider configuration generated for Wrangler is ignored build output and
  points only to files in the verified PBOC root.
- Native application handlers remain trusted process code; OCI isolation does
  not convert them into an OpenSDK sandbox.

The scoped ASVS map is
[`security/asvs-v5.0.0-g3.json`](../../security/asvs-v5.0.0-g3.json). It is an
ownership map, not an application compliance or certification claim.

## Residual limits

- `v1alpha1` does not define portable databases, queues, schedules, object
  storage, durable objects, or provider billing.
- The reference state schema is stateless; stateful applications must provide
  migration-specific rolling and rollback evidence.
- Cloudflare's account policy and rollout control remain provider/operator
  surfaces; the final protected release must repeat the demonstrated edge run.
- G3 does not stabilize OpenSDK's server plane or claim multi-cloud parity.
