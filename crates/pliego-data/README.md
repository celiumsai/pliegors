# pliego-data

`pliego-data` is the provider-neutral G2 ownership boundary for request data,
resources, loaders, actions, sessions, idempotency, secrets, outbound HTTP
policy, runtime cache, and causal invalidation.

It contains contracts and development/conformance adapters. It does not contain
an ORM, identity provider, database driver, cloud SDK, executor, or HTTP
transport. `pliego-runtime` binds these APIs to an admitted request and sealed
route; applications provide concrete resources and production stores.

## Source-beta surface

- `DataContext`: request identity, deadline, cancellation, admitted values,
  cleanup, grants, and bounded redacted receipts;
- `ResourceRegistry`: sealed typed handles with provider capabilities and
  request-owned leases;
- `LoaderPolicy` and `Loader`: typed immutable output, input/revision identity,
  output bounds, cancellation, and request-local deduplication;
- `ActionPolicy` and `Action`: strict progressive input, authentication and
  authorization admission, commit state, field errors, navigation, causal
  invalidation, and result bounds;
- `IdempotencyManager`: key/input/principal/deployment binding with conflict
  detection and committed-result replay;
- `SessionManager`: opaque server-side sessions, secure cookie policy,
  rotation, revocation, idle/absolute expiry, and schema-version admission;
- `CsrfManager`: session-, action-, revision-, and secret-bound proofs;
- `CachePolicy`: explicit public/private domains, typed partitions, Vary,
  freshness/stale windows, value/tag limits, fill coalescing, receipts, and
  compatibility epochs;
- `InMemoryInvalidationCoordinator`: exact-key/tag target digests, causal
  receipts, idempotent delivery, sequence, and acknowledgement barriers;
- `OutboundHttpGuard`: exact scheme/host/port/path policy, public-address DNS
  pinning, redirect revalidation, deadline, and cancellation; and
- `SecretHandle`: redacted opaque secret metadata and controlled byte access.

## Minimal resource and loader

```rust
use pliego_data::{
    CapabilitySet, LoaderPolicy, ResourceRegistryBuilder, ResourceRequirement,
    ResourceSpec,
};

let read = CapabilitySet::none().allowing("read")?;
let resources = ResourceRegistryBuilder::new()
    .register(
        ResourceSpec::new("catalog", "application-store")?
            .with_capabilities(read),
        MyCatalog::connect()?,
    )?
    .seal();

let loader = LoaderPolicy::new(
    "catalog-loader",
    1,
    "dev.example.catalog-query/v1",
    "dev.example.catalog-view/v1",
)?
.resource(ResourceRequirement::new("catalog")?.requiring("read")?)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The example is schematic: concrete resource setup belongs to the application.
See `examples/fullstack-pliego` for an executable two-runtime application.

## Security and correctness boundary

Every policy identity and bound is explicit. Debug output and receipts omit
resource values, admitted input, session payloads, raw cache keys, identities,
tokens, and secret bytes. Native application code is trusted process code;
capability mediation constrains framework handles rather than sandboxing Rust.

The in-memory session, idempotency, cache, and invalidation implementations are
for development and conformance. A production adapter must provide its own
durability, consistency, expiry, revocation-lag, availability, and operational
evidence through the same public contracts.

## Status

The G2 implementation is published on crates.io as `0.2.0-beta.1` in the
coordinated framework graph. Pin every `pliego-*` crate to that exact version
and expect another beta line to change the API.

Read [RFC-010](../../docs/rfc/RFC-010-data-actions-cache.md), its four
subcontracts, the [G2 evidence](../../docs/evidence/g2-fullstack-beta.md), and
the [G2 ASVS ownership map](../../security/asvs-v5.0.0-g2.json).
