# RFC-007: Pliego Build Output Contract

**Status:** Draft
**Target:** `0.2.0`
**Last updated:** 2026-07-18

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

## Proposed top-level model

```text
schema
framework identity
build identity and provenance
assets[]
routes[]
functions[]
cache policies[]
permissions[]
secret references[]
telemetry hooks[]
adapter requirements[]
```

Every path is normalized and relative. Every byte-bearing object has a digest.
Secret references contain identifiers and purpose, never secret values. A host
must reject unknown required capabilities before uploading any artifact.

## Portability gate

PBOC does not become stable until one corpus passes unchanged on the official
Cloudflare target and a native reference host. Host-specific additions live in
namespaced optional sections and cannot be required by the core fixture.

## Repository boundary

This RFC, the WIT package, schemas, fixtures, validator, and reference native
host are public. Pliego.run UI, orchestrator, deployment engine, billing,
Cloudflare account configuration, and operational policy remain in a private
repository.

## Open questions

- whether cache tags are opaque strings or typed dependency edges;
- function streaming and commit-boundary representation;
- portable queue, durable object, and scheduled task subsets;
- receipt format for cost estimation versus final billed usage.
