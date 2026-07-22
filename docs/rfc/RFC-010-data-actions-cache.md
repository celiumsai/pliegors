# RFC-010: Data loaders, actions, sessions, and cache

**Status:** Draft  
**Owner:** Runtime and Data  
**Target gate:** G2  
**Created:** 2026-07-19

**Implementation:** Complete on `main` as an unreleased source beta; see
[G2 evidence](../evidence/g2-fullstack-beta.md). Draft status is the governance
state, not an implementation-status claim.

## Subcontracts

G2 is split into four reviewable contracts. This RFC remains the umbrella and
acceptance authority; the subcontracts own the exact preview API semantics:

- [RFC-011](RFC-011-request-resources-and-loaders.md): request context,
  resources, leases, and typed loaders;
- [RFC-012](RFC-012-progressive-actions.md): progressive forms, actions,
  idempotency, commit, and bounded uploads;
- [RFC-013](RFC-013-sessions-and-secrets.md): sessions, cookies, rotation, and
  opaque secret handles; and
- [RFC-014](RFC-014-runtime-cache-and-invalidation.md): public/private runtime
  cache, Vary, receipts, and coordinated invalidation.

The public ownership boundary is `pliego-data`. It contains provider-neutral
contracts and in-memory reference implementations. `pliego-runtime` binds
those contracts to request admission, route identity, cancellation, deadlines,
diagnostics, and response commitment. Provider clients and authentication
products remain integrations rather than core dependencies.

## Summary

PliegoRS will add request-scoped loaders, progressively enhanced actions,
sessions, secrets, and explicit cache policies after the G1 request lifecycle
exists. These surfaces share cancellation, resource ownership, receipts, and
diagnostics. They do not introduce a mandatory database, ORM, identity provider,
or hosted service.

## Principles

1. Data access is owned by a request or an explicit background resource.
2. Cancellation, deadline, and capability policy cross every async boundary.
3. Cache behavior is declared and inspectable, never inferred from function
   names or deployment provider defaults.
4. Mutations are progressive by default and can complete without JavaScript.
5. Authentication verifies identity; authorization remains application policy.
6. Secrets are handles supplied by the host, never values embedded in PBOC.
7. Multi-instance correctness is part of the contract, not a deployment note.

## Loaders

A loader declares:

- stable loader ID and input schema;
- owning route or layout;
- required resources and capabilities;
- cache policy ID;
- deadline behavior;
- serialization boundary; and
- public error mapping.

The runtime deduplicates identical loader invocations only within the scope and
policy that declare it safe. A request-local registry owns resources and runs
cleanup in LIFO order. Loader output is immutable after publication to a render
boundary.

Loaders receive typed route parameters, admitted query values, request metadata,
and resource handles. They do not receive ambient filesystem, network,
environment, clock, random, session, or secret authority.

## Actions

An action is a POST-oriented mutation contract integrated with an HTML form and
available to typed clients. Each action declares:

- stable action ID;
- accepted content types and maximum decoded bytes;
- input and field-error schema;
- CSRF policy;
- authentication and authorization hooks;
- idempotency policy;
- transaction/resource requirements;
- invalidation intents; and
- success and failure navigation behavior.

JavaScript enhancement may submit in place, but the same server action must
support a standards-based form submission unless the application explicitly
declares a non-progressive surface. Client disconnect cancels work unless the
action has atomically crossed its documented commit point.

## Idempotency and commit

The runtime distinguishes:

- no commit;
- commit in progress;
- committed;
- compensation required.

An idempotency key is scoped to the action, authenticated principal or anonymous
session, and deployment contract. Reusing a key with different admitted input
is an error. The framework coordinates receipts and replay of the result; it
does not pretend to make an external database transaction atomic.

## Sessions and cookies

The session contract owns cookie encoding, size, rotation metadata, SameSite,
Secure, HttpOnly, path, domain, expiry, and key identifiers. Session payloads
are bounded and versioned. Hosts inject key handles; keys never appear in source,
build output, diagnostics, telemetry, or receipts.

Default application cookies are host-only, Secure in production, HttpOnly when
not intentionally browser-readable, and SameSite=Lax. Cross-site workflows
require an explicit policy and threat-model update.

Session identity does not authorize data access. Applications provide a data
access layer hook that evaluates the admitted identity close to each protected
resource.

## Secrets and resources

Resources are registered by type and capability in a runtime-owned registry.
Examples include SQL pools, HTTP clients, object storage, queues, CMS clients,
and Hyphae. Core PliegoRS APIs depend on resource contracts, not provider SDKs.

A secret is an opaque resource handle. The host may expose length, version, and
rotation identity where required, but not the value through debug formatting or
serialization.

## Cache domains

Four cache domains are distinct:

| Domain | Example | Default lifetime |
| --- | --- | --- |
| Build | Compiler transform and generated artifact | Bound to exact build inputs |
| Immutable asset | Content-addressed media | Indefinite by digest |
| Public runtime | Shared page or loader result | Explicit policy and validator |
| Private request | User or session-specific memoization | One request unless a private store is declared |

Private data never enters a public cache. A policy declares key components,
vary inputs, freshness, stale behavior, tags, invalidation channel, error
behavior, and maximum value bytes. Missing identity or vary inputs fail closed.

## Revalidation and multi-instance behavior

Invalidation is an event with:

- stable cache namespace and policy version;
- exact key or bounded tag set;
- causal action or operator receipt;
- issued-at sequence from the configured coordinator; and
- delivery and acknowledgement state.

Every conformant multi-instance adapter must prove that an authenticated
mutation cannot leave one replica serving a forbidden stale value beyond the
declared policy. A local in-memory adapter is valid for development but cannot
claim multi-instance support.

## Diagnostics

Planned inspection surfaces are:

```text
pliego why request <request-id>
pliego why cache <cache-key-or-receipt>
pliego inspect action <action-id>
```

They report policy identities, cache outcomes, invalidation causality, commit
state, and stable diagnostic codes. Secret values, session payloads, form data,
and user identifiers are redacted by default.

## OpenSDK boundary

Extensions may implement resource providers, cache stores, and action tooling
through versioned contracts. Core request semantics, cancellation, authorization
hooks, and cache isolation remain host-owned. A plugin cannot grant itself a
resource by declaring a capability.

## Security requirements

The [full-stack threat model](../48-fullstack-threat-model.md) governs this RFC.
G2 requires evidence for:

- CSRF and Origin/SameSite policy;
- session fixation and rotation;
- bounded multipart and form parsing;
- SQL/HTTP/resource timeouts;
- SSRF-resistant HTTP client policy;
- public/private cache isolation;
- cache-key poisoning and Vary correctness;
- action idempotency and replay;
- secret redaction and non-serialization; and
- cross-user and cross-tenant adversarial cases.

## Acceptance evidence

One authenticated SaaS reference application must run on two native instances
and demonstrate:

- progressive login and mutation without JavaScript;
- typed loader/action failures;
- cancellation before and after commit;
- idempotent retry;
- session rotation;
- public and private cache behavior;
- coordinated invalidation with bounded lag;
- no cross-user leakage or stale authorization result; and
- complete request, action, and cache diagnostics.

## Alternatives rejected

- **Bundle an ORM:** rejected because storage choice is outside framework
  semantics and would reduce portability.
- **Infer cache from HTTP method or function name:** rejected because it hides
  privacy, vary, staleness, and invalidation decisions.
- **JavaScript-only actions:** rejected because progressive behavior is a
  competitive reliability requirement.
- **Provider-global cache:** rejected because PliegoRS must specify correctness
  independently of Pliego.run or any one cloud.
