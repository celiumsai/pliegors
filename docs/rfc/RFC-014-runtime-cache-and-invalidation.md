# RFC-014: Runtime cache and causal invalidation

**Status:** Draft for G2 preview
**Owner:** Runtime and Data
**Parent:** [RFC-010](RFC-010-data-actions-cache.md)
**Created:** 2026-07-21

**Implementation:** Complete on `main` for the G2 source beta; see
[G2 evidence](../evidence/g2-fullstack-beta.md).

## Decision

G2 defines runtime cache as explicit policy plus a provider contract. It does
not infer privacy, lifetime, or invalidation from HTTP method, function name,
route location, deployment provider, or response status.

Build cache and immutable assets remain separate existing domains. G2 adds
public runtime cache and private request/session cache.

## Cache policy

Every `CachePolicy` declares:

- stable policy ID and semantic revision;
- domain: public runtime, private request, or private session;
- namespace and compatibility epoch;
- exact key recipe and required Vary inputs;
- identity/tenant partition requirement;
- freshness and optional bounded stale windows;
- error behavior;
- maximum key, tag, and serialized value bytes;
- stampede policy and fill deadline;
- tag namespace and maximum tags per entry; and
- invalidation adapter requirements.

Missing required input, identity, tenant, or policy version is an error, not a
cache bypass that might change privacy.

## Typed keys and Vary

A key is canonical structured data, not string concatenation. Its digest binds
the policy revision, namespace, deployment compatibility epoch, loader/action
revision, admitted input digest, declared Vary values, and private partition.

Headers and queries can participate only through allowlisted, normalized,
bounded values. Cookies never vary a public entry. Authorization results and
private loader output never enter a public domain.

## Outcomes and receipts

Every lookup emits one bounded outcome:

```text
hit | miss | stale | bypass | private | invalidated | rejected
```

The receipt includes policy ID/revision, namespace, key digest, outcome,
freshness bucket, value-size bucket, invalidation sequence when applicable, and
causal action/operator receipt. Raw keys, headers, query values, identities,
session IDs, values, and tags containing application data are excluded.

## Invalidation

Invalidation targets an exact key digest or a bounded tag set inside one policy
namespace and compatibility epoch. Each event declares event ID, causal receipt,
monotonic coordinator sequence, issued-at bucket, and required acknowledgement
set. Adapters must make duplicate delivery idempotent.

An action can request read-your-writes. The runtime then waits for the configured
local or replica acknowledgement barrier before following the success
navigation. Timeout yields an explicit degraded or failed action policy; it
never silently claims consistency.

## Stampede and cancellation

At most one bounded fill owns a key when the policy enables coalescing. Waiters
have independent cancellation and deadlines. A cancelled waiter does not cancel
a shared fill still required by other admitted waiters. A fill promoted beyond
one request becomes an explicit host-owned bounded task with memory, time, and
shutdown policy.

## Reference adapters

G2 ships:

1. request-local private cache;
2. bounded in-memory public cache for development and conformance; and
3. an in-process two-replica coordinator harness that proves protocol
   semantics without claiming production durability.

Redis-compatible and Cloudflare adapters are integrations. They pass the same
public cache TCK before being listed as conformant.

## Acceptance evidence

CAC-001 through CAC-004 close only when tests prove:

- public/private types and runtime checks prevent domain confusion;
- omitted Vary or partition input fails closed;
- two users and two tenants cannot share private output;
- key/tag/value bounds resist adversarial input;
- exact/tag invalidation is causal and idempotent across two replicas;
- read-your-writes observes the declared acknowledgement barrier;
- rolling compatibility epochs cannot read incompatible entries;
- stampede waiters cancel independently; and
- `pliego why cache` output passes the redaction corpus.
