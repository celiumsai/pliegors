# ADR-002: Hyphae as the active data and navigation plane

**Status:** Accepted
**Decision date:** 2026-07-10
**Scope:** PliegoRS and PLIEGO experiences that use Hyphae

## Context

A durable PliegoRS application cannot depend on a network request for every route decision,
pointer movement, or animation frame. It also cannot reduce Hyphae to a passive
CMS response if the intended experience includes live objects, documents,
semantic relationships, durable history, and verifiable provenance.

The architecture needs both properties: Hyphae must be active in the experience,
and rendering must remain deterministic and responsive without waiting for it.

## Decision

Hyphae is the privileged active plane for:

- journals and versioned projections;
- Pliegos, routes, scenes, objects, documents, fragments, and assets;
- locale, visual tier, fallback, preload, and relationship metadata;
- deterministic prepared navigation queries;
- bounded semantic retrieval for exploratory navigation;
- durable receipts, traceability, and provenance.

Hyphae is not the browser scheduler or frame loop. PliegoRS consumes bounded,
versioned snapshots and event deltas from Hyphae, folds them locally, and renders
from in-memory state.

## Production topology

```text
Browser / PliegoRS island
  local route snapshot + local fold + render state
              ^
              | bounded loader, prefetch, event delta, background refresh
              v
Cloudflare edge
  identity + tenant + scope + exact CORS + cache + quotas
              |
              | private authenticated service request
              v
Hyphae
  journal + projections + object graph + prepared/semantic queries
```

Cloudflare is the public production ingress and delivery boundary. Browser code
does not receive raw Hyphae credentials and does not call an unscoped journal.
The gateway resolves identity and tenant, enforces authorization and quotas, and
can cache bounded read models. Assets remain content-addressed and are delivered
through the appropriate Cloudflare surface.

A self-hosted PliegoRS application may provide an equivalent trusted boundary.
It must preserve the same auth, tenant, scope, replay, and isolation invariants.

## Navigation lanes

Hyphae participates through four explicit lanes:

1. **Build and publish:** compile route, object, document, locale, asset, and tier
   projections into bounded snapshots.
2. **Route load and prefetch:** fetch a deterministic working set before a user
   enters it, then navigate locally within that set.
3. **Background exploration:** request related objects or semantic results outside
   input-critical work, validate them, and append an explicit local transition.
4. **Durable mutation:** send authenticated, namespaced events through the
   versioned, idempotent sync contract and reconcile signed receipts.

Prepared queries own deterministic navigation. Semantic retrieval may suggest or
rank exploratory relationships, but it cannot silently rewrite canonical routes,
permissions, workflow state, or provenance.

## Frame-loop isolation

The following work is forbidden inside `requestAnimationFrame`, a Three.js render
callback, pointer-move handling, or scroll sampling:

- network requests or awaiting Hyphae;
- Hyphae queries, semantic retrieval, or journal append;
- parsing an unbounded snapshot;
- rebuilding an entire projection;
- allocating or decoding a new scene package.

The frame loop may read already-resolved local state, interpolate presentation
values, update bounded renderer state, and enqueue background work. Network
failure cannot blank an already loaded route or freeze motion.

## Data and sync contract

The accepted target is the contract in `docs/01-hyphae-protocol.md`:

- a versioned event envelope and explicit schema version;
- tenant and actor derived at the Cloudflare boundary;
- `app_*` application kinds, with engine opcodes reserved;
- idempotent batches and deduplication by `client_event_id`;
- expected cursor checks, pull by cursor, and conflict handling;
- durable signed receipts and browser verification;
- versioned shared Rust reducers where reuse is valid.

### Amendment: verified sync protocol v2 (2026-07-15)

The client-side target above is now represented by protocol
`pliego-hyphae/2`. V2 adds mandatory signed append and page attestations,
per-event receipt verification, a fixed snapshot cursor, stable logical
authority, explicit event-version admission, and the consuming
`UntrustedPullPage -> ValidatedPullPage -> VerifiedPullPage -> AppliedPullPage`
path. A raw or merely shape-valid page cannot enter a reducer.

This amendment closes the client contract, not the production topology. The
Cloudflare gateway, tenant/key operations, durable browser outbox and replay
state, and a conforming deployed Hyphae v2 service remain pending. The exact
decision and downgrade boundary are recorded in
[ADR-004](ADR-004-hyphae-verified-sync-v2.md).

"One machine" means shared event identity, schema, reducer semantics, and
verifiable lineage. It does not mean one physical store or trust boundary.

## Failure behavior

- A static or prefetched route remains usable when Hyphae is unavailable.
- Pending events remain visibly pending and are never presented as durable.
- Retry cannot create a second semantic event after a lost acknowledgement.
- Stale snapshots carry a version and bounded age; the UI can explain when live
  data is unavailable without exposing infrastructure details.
- A semantic result is optional enrichment. Deterministic navigation and legal
  critical workflows do not depend on model availability.

## Current state and north star

<!-- markdownlint-disable MD013 -->

| Concern | Current fact on 2026-07-10 | Accepted north star |
| --- | --- | --- |
| PliegoRS seam | Individual `{kind,payload}` events can be pushed and acknowledged on a separate Hyphae chain. | Authenticated, idempotent, bidirectional sync with receipts and shared reducer contracts. |
| Namespace | PliegoRS now maps local kinds to `app_*`; Hyphae rejects reserved engine kinds. | A typed, versioned application envelope enforced at edge and engine. |
| Navigation | No production Hyphae object graph or bounded route snapshot is integrated with PliegoRS. | Typed active navigation using prepared queries, semantic exploration, and local snapshots. |
| Cloudflare | Required by architecture, but the PliegoRS/Hyphae gateway is not implemented. | Sole public ingress for identity, tenant resolution, authorization, cache, and quotas. |
| Memory source | `mycelium-do` is the production source of truth. | It remains so until a separately approved and verified Hyphae cutover. |

<!-- markdownlint-enable MD013 -->

Hyphae does not replace `mycelium-do` by implication. Migration requires its own
ADR, compatibility and data-integrity plan, rollback, production canaries, and
explicit approval.

## Consequences

- PliegoRS needs loaders, bounded snapshots, prefetch, and background scheduler
  lanes before Hyphae can become an active production dependency.
- Hyphae schemas become product contracts and need replay, migration, tenant, and
  adversarial tests.
- The browser experience remains responsive and can degrade deliberately during
  network or semantic-service failure.
- Pure static PliegoRS sites may omit Hyphae entirely.

## Rejected alternatives

- **Query Hyphae directly from every component:** rejected because it couples
  rendering to latency, credentials, and unbounded fan-out.
- **Put Hyphae in the animation loop:** rejected because network and query work
  cannot meet a frame-time contract.
- **Export a passive JSON CMS snapshot only:** rejected because it loses durable
  events, relationships, live projections, and provenance.
- **Replace `mycelium-do` immediately:** rejected because no production cutover
  has been approved or verified.

## References

- [PliegoRS founding specification](../00-pliegors-spec.md)
- [PliegoRS / Hyphae protocol](../01-hyphae-protocol.md)
- [Execution backlog](../19-product-execution-backlog.md)
- [Hardening roadmap](../28-hardening-roadmap.md)
- [ADR-003: Adaptive tiers and performance](ADR-003-adaptive-tiers-performance.md)
