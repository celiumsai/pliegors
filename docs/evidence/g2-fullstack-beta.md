# G2 full-stack beta evidence

**Status:** Complete on `main` as an unreleased source beta
**Date:** 2026-07-22
**Scope:** RFC-010 through RFC-014 implementation and conformance
**Public stability:** Preview; the RFCs remain Draft pending governance review

## Result

G2 adds a provider-neutral full-stack contract without adding an ORM, identity
product, mandatory database, or deployment provider. `pliego-data` owns typed
resources, loaders, actions, idempotency, sessions, CSRF, secrets, outbound
HTTP policy, runtime cache, and invalidation. `pliego-runtime` binds those
contracts to the sealed route graph and request lifecycle.

The reference application at `examples/fullstack-pliego` runs the same sealed
application contract on two native runtime instances. Its HTML login and
account mutation work without JavaScript; enabling JavaScript does not change
their semantics because the beta deliberately ships no client-only mutation
path.

## Acceptance matrix

| Contract | Evidence | Result |
| --- | --- | --- |
| DAT-001 resources | Duplicate/invalid IDs, missing grants, capability and type mismatch, lease closure, redacted debug | Pass |
| DAT-002 loaders | Typed input/revision, request-local deduplication, immutable bounded output, cancellation, LIFO cleanup, failure receipts | Pass |
| ACT-001 admission | Method, media type, encoding, Origin, authentication, authorization, CSRF, unknown fields, and form-field count fail closed | Pass |
| ACT-002 idempotency | Principal/deployment/action/input binding, committed replay, conflicting-key rejection, bounded stored result | Pass |
| ACT-003 commit | Cancellation before, during, and after commit records truthful state; no unproved rollback | Pass |
| UPL-001 uploads | Encoded/decoded, field, part, file, name, and temporary-storage bounds; random paths and cleanup | Pass |
| SES-001 sessions | Secure cookie defaults, opaque CSPRNG token, fixation prevention, rotation, expiry, schema skew, and cross-replica revocation | Pass |
| CAC-001 domains | Public, private-request, and private-session domains reject missing or wrong partition types | Pass |
| CAC-002 keys | Structured Vary plus domain-separated request, session, tenant, and identity partitions | Pass |
| CAC-003 invalidation | Exact-key/tag targeting, target digest, causal receipt, duplicate-delivery replay window, compatibility epoch, and acknowledgement barrier | Pass |
| CAC-004 stampede | One bounded fill per key; cancelled waiters do not cancel admitted shared work | Pass |
| SEC-001 | G2 ASVS 5.0.0 ownership map, SSRF policy, redaction corpus, CSRF/session/cache isolation cases | Pass |

## Two-replica application proof

`examples/fullstack-pliego/tests/two_replicas.rs` proves:

- login rotates the anonymous token and the prior token fails on the other
  runtime;
- two runtimes share one versioned session and idempotency store contract;
- a mutation commits once and an identical retry through the other runtime
  replays the result;
- conflicting input under the same idempotency key returns `409`;
- public and private cache entries are invalidated before read-your-writes
  navigation completes;
- two users never share private output;
- a tampered CSRF proof returns `403` before mutation;
- strict action decoding rejects mass-assignment fields with `422`;
- a typed missing-account loader failure becomes an authored `404` while its
  internal failure text stays out of the response;
- revocation performed through one instance is visible to the other; and
- runtime, data, cache, action, and invalidation receipts omit the adversarial
  credential, CSRF, identity, and submitted-value corpus.

The corpus also binds two real loopback TCP listeners and verifies that both
instances serve the same application-contract SHA-256. It does not infer
distributed durability from two in-process stores.

## Diagnostics

The beta adds three receipt/contract-driven commands:

```text
pliego why request <runtime-receipt.json>
pliego why cache <cache-receipt-or-invalidation.json>
pliego inspect action <id> [--contract <runtime-contract.json>]
```

Inputs are bounded to 4 MiB, decoded through versioned `deny_unknown_fields`
contracts, and checked for the expected contract identity and lowercase
SHA-256 fields. Explanations contain policy IDs, coarse outcomes, commit and
acknowledgement state, and digests. They exclude request bodies, session
payloads, raw cache keys, identities, and secrets.

Generate the application contract from the reference app:

```sh
cargo run -p fullstack-pliego -- contract .pliego/runtime-contract.json
cargo run -p pliego-cli -- inspect action rename-account \
  --contract .pliego/runtime-contract.json
```

## Reproduction

The focused Linux/WSL gate is:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p pliego-data
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p pliego-runtime --test actions_g2 --test session
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p fullstack-pliego --test two_replicas
CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p pliego-cli --bin pliego runtime_diagnostics::tests
node scripts/check-security-map.mjs
node scripts/check-product-truth.mjs
# With the reference server listening on 127.0.0.1:4320:
npm run check:g2-browser
```

The complete workspace formatting, Clippy, test, Rustdoc, documentation-link,
product-truth, and security-map gates remain required before publication.

## Closure validation

The final source-beta closure ran on 2026-07-22 with Rust `1.86.0`:

| Gate | Result |
| --- | --- |
| `cargo test --workspace --all-targets --locked` on Debian WSL2 | Pass; all active workspace tests passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | Pass |
| `cargo audit` plus all three standalone OpenSDK lockfiles | Pass; no vulnerability advisory; one documented allowed unmaintained warning remains |
| Root and both Worker `npm audit --audit-level=high` surfaces | Pass; zero vulnerabilities |
| `npm run test:phase-1` | Pass; 98 active tests passed and one POSIX-only case was skipped on Windows |
| Product truth, documentation links, security map, fuzz contract, distribution, and Phase 1 gates | Pass |
| Official-site deterministic double build and `npm run check:site` | Pass; identical `72df452ce782` build identity, 79 routes |
| `npm run check:site-deployment` | Pass; Cloudflare package dry-run only |
| `npm run check:g2-browser` against a rebuilt native server | Pass with JavaScript enabled and disabled |
| `npm run check:crates` | Pass; 16 released packages verified, runtime and CLI explicitly deferred by the G2 source-preview dependency chain |

RustSec reported `RUSTSEC-2026-0009` in the former `cookie -> time 0.3.44`
chain during closure. G2 removed both dependencies, retained the Rust 1.86 MSRV,
and replaced their narrow session-cookie use with bounded request parsing and
header serialization covered by the runtime session tests. `Cargo.lock` no
longer contains either package.

## Security ownership

[`security/asvs-v5.0.0-g2.json`](../../security/asvs-v5.0.0-g2.json) maps the
applicable G2 controls to framework, application, or shared ownership. It is a
machine-checked ownership map, not an OWASP certification or an application
compliance claim. The normative source is OWASP ASVS 5.0.0.

## Explicit limits

- G2 is present on `main`; changes in `pliego-data`, `pliego-router`,
  `pliego-runtime`, `pliego-cli`, and the reference app are not in the current
  `v0.0.2` release or component prerelease. Registry publication propagates
  this boundary and defers runtime and CLI until the source-preview dependency
  chain receives new coordinated versions.
- In-memory sessions, idempotency, cache, and invalidation are development and
  conformance adapters. No production durable/distributed adapter is claimed.
- The outbound HTTP guard validates policy, resolves and pins an allowed public
  address, and returns a permit. The application adapter must use that pinned
  address and revalidate redirects.
- Authentication protocols, authorization policy, database transactions,
  malicious-file scanning, cache backing services, and secret storage remain
  application or operator responsibilities.
- G2 proves the native host. It makes no G3 PBOC, OCI portability, Cloudflare
  runtime, rolling-deployment, or production availability claim.
