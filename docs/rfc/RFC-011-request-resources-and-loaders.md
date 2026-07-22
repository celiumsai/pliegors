# RFC-011: Request resources and typed loaders

**Status:** Draft for G2 preview
**Owner:** Runtime and Data
**Parent:** [RFC-010](RFC-010-data-actions-cache.md)
**Created:** 2026-07-21

**Implementation:** Complete and published in `0.2.0-beta.1`; see
[G2 evidence](../evidence/g2-fullstack-beta.md).

## Decision

G2 introduces `pliego-data` as the provider-neutral ownership boundary for
request data semantics. `pliego-runtime` creates one `DataContext` for every
admitted request and supplies the request cancellation signal, effective
deadline, stable request/route/deployment identities, and granted resources.

No transport, executor, database, ORM, identity provider, or hosted service
type may appear in the stable data API. Integrations may use those libraries
internally.

## Stable identities

Loader, resource, policy, action, and provider IDs use the same bounded portable
grammar:

```text
[a-z][a-z0-9]*(?:-[a-z0-9]+)*
```

IDs are at most 96 bytes. They are behavior identities, not display names.
Changing admitted input, output schema, resource requirements, privacy, or
cache behavior requires a semantic revision in the sealed application graph.

## Resources

An application-owned `ResourceRegistry` is sealed before the runtime starts.
Each entry declares:

- stable resource ID and provider ID;
- concrete Rust type for native callers;
- granted capability set;
- whether the handle is shared or request-local;
- optional maximum lease duration; and
- a redacted description suitable for diagnostics.

A declaration is not authority. A route or loader receives a resource only
when the sealed application requirement, registered provider grant, and
operator policy intersect. Type mismatch, missing capability, unknown resource,
expired lease, and closed request all fail before provider code executes.

The registry stores process-owned handles. A `ResourceLease<T>` is a bounded,
request-owned reference to one handle; it exposes cancellation and deadline but
does not expose the registry or other resources. Debug output contains IDs and
capabilities only.

## Request context

The data-facing context contains:

- stable request, route, and deployment IDs;
- admitted route parameters;
- admitted query values;
- bounded request metadata allowlisted by the runtime;
- effective deadline and cancellation observation;
- granted resource leases;
- request-local loader registry; and
- bounded data receipts.

Raw cookies, headers, environment variables, filesystem, network, clock,
randomness, secrets, and session payloads are not ambient fields. Callers must
request a typed handle or admitted value from the owning boundary.

## Loaders

A `Loader` is a typed, cancelable, immutable read boundary:

```text
Loader<Input, Output>
```

Its `LoaderPolicy` declares stable ID, semantic revision, input/output schema
IDs, required resources and capabilities, cache policy ID, maximum output
bytes, and whether identical invocations may deduplicate inside one request.

Inputs are admitted before invocation. Output serialization is bounded and
validated before publication to a render boundary. Published output is shared
as immutable data.

Request-local deduplication keys include loader ID, semantic revision, admitted
input digest, identity partition when required, and cache policy version. A
failed or cancelled invocation is never reused as a successful value. Cross-
request reuse exists only through an explicit cache policy from RFC-014.

## Cancellation and cleanup

Loader work observes the request cancellation signal and effective deadline.
Resource leases cannot outlive the request context. Request cleanup runs in
LIFO order and records acknowledgement or a bounded diagnostic. Detached work
requires a separately owned background resource and is not a loader.

## Diagnostics and receipts

Each invocation records a bounded `DataReceipt` containing stable IDs, coarse
outcome and duration buckets, cache outcome, cancellation state, and output
size bucket. Inputs, outputs, query values, resource values, secrets, sessions,
and user identifiers are excluded.

Minimum diagnostics:

| Code | Meaning |
| --- | --- |
| `PLG-DAT-001` | Invalid stable data ID |
| `PLG-DAT-101` | Resource unavailable or not granted |
| `PLG-DAT-102` | Resource type mismatch |
| `PLG-DAT-103` | Required capability missing |
| `PLG-DAT-201` | Loader input rejected |
| `PLG-DAT-202` | Loader output exceeded its bound |
| `PLG-DAT-408` | Loader deadline or cancellation |
| `PLG-DAT-500` | Loader failed without a public mapping |

## Acceptance evidence

DAT-001 and DAT-002 close only when tests prove:

- registry sealing rejects duplicate and invalid IDs;
- a loader cannot acquire an undeclared resource or capability;
- leases observe cancellation and cannot be acquired after scope close;
- request-local identical loader calls execute once when allowed;
- different input, identity partition, revision, or policy never deduplicates;
- output bounds fail before render publication;
- cleanup is LIFO and acknowledged; and
- receipts and debug output contain none of the adversarial secret corpus.
