# RFC-007: Pliego Build Output Contract

**Status:** Implemented preview; stabilization pending
**Target:** `0.3.0-beta.1`
**Last updated:** 2026-07-22

## Summary

The Pliego Build Output Contract (PBOC) is a provider-neutral, digest-bound
description of deployable assets, routes, functions, cache policy, permissions,
secret references, and telemetry hooks. It is open source and belongs to
PliegoRS. Pliego.run is a separate closed-source operator that may consume PBOC
but does not own or extend it through private required fields.

## Goals

- run the same output on at least two independent hosts;
- make runtime powers and cache behavior inspectable before deployment;
- bind every referenced byte to the existing artifact ledger and provenance;
- let a project export or leave Pliego.run without rebuilding its source;
- keep provider billing, control planes, infrastructure, and operator secrets
  outside the framework repository.

## Non-goals

- standardizing every cloud resource;
- exposing Pliego.run implementation details;
- embedding provider credentials;
- making a deployment portable when it explicitly chooses a provider-only
  capability.

## Implemented top-level model

```text
schema
framework identity
build identity, artifact ledger, SBOM and provenance
compatibility epoch, sequence, state schema and rollback declaration
capabilities[]
artifacts[]
targets[]
assets[]
routes[]
functions[]
cachePolicies[]
permissions[]
secretReferences[]
telemetryHooks[]
```

The wire identifier is `dev.pliegors.pboc/v1alpha1`. Every path is normalized
and relative. Every artifact records bytes, SHA-256, role, and media type.
Secret references contain identifiers and purpose, never secret values. A host
rejects unknown required capabilities before uploading any artifact. Unknown
JSON fields fail closed.

The implemented Rust authority is `pliego-pboc`; the machine schema is
`schemas/pliego.pboc.schema.json`. `verify_bundle` rejects missing, additional,
modified, linked, or non-portable files. `HostAdmission` selects one exact
target and returns the upload set, unsupported optional features, and required
secret references.

## Compatibility contract

Rolling coexistence requires one application ID, compatibility epoch, and
state schema; a higher sequence; and an exact `previousReleaseId` link. Rollback
requires the reverse sequence, the same identity/epoch/schema, the exact link,
and `rollbackSafe: true` on the active release. Both checks emit bounded JSON
receipts and stable `PLG-PBOC-100` through `PLG-PBOC-106` failures.

The declaration never proves an irreversible data migration safe. Stateful
applications and providers retain migration-specific evidence ownership.

## Portability gate

The G3 corpus executes one sealed PBOC unchanged on the Rust native/OCI host and
the Rust Cloudflare Workers host. It covers complete responses, ordered
streaming, immutable assets, cache headers, 404/405 semantics, required-feature
rejection, rolling skew, rollback, and provider-secret exclusion. Passing G3
promotes PBOC to public preview, not stable 1.0.

## Repository boundary

This RFC, the WIT package, schemas, fixtures, validator, and reference native
host are public. Pliego.run UI, orchestrator, deployment engine, billing,
Cloudflare account configuration, and operational policy remain in a private
repository.

## Deferred beyond v1alpha1

- typed cache dependency edges beyond opaque tags;
- asynchronous boundary parity beyond complete and ordered streaming;
- portable queue, object storage, durable object, and scheduled task subsets;
- provider cost-estimate and final-billing receipts; and
- additional independent deployment hosts.

The operational guide is
[`docs/50-pboc-portable-deployment.md`](../50-pboc-portable-deployment.md) and
the acceptance record is
[`docs/evidence/g3-pboc-provider-conformance.md`](../evidence/g3-pboc-provider-conformance.md).
