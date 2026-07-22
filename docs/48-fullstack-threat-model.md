# Full-stack runtime threat model

**Status:** Initial G0 baseline; mandatory for G1-G3  
**Owners:** Runtime and Security  
**Updated:** 2026-07-19  
**Reference baseline:** OWASP ASVS 5.0 Level 2 controls applicable to a web framework and its reference runtime

## Scope

This document extends the existing static build, browser lifecycle, artifact,
sync, and supply-chain reviews to the server attack surface proposed by
[RFC-008](rfc/RFC-008-native-runtime.md),
[RFC-009](rfc/RFC-009-route-graph.md), and
[RFC-010](rfc/RFC-010-data-actions-cache.md).

It covers the open-source PliegoRS native runtime, route graph, renderer,
loaders, actions, sessions, cache contracts, OpenSDK server boundary, portable
build output, and official conformance harness. It does not cover a private
Pliego.run control plane, an application's business authorization rules, a
chosen database, or a deployed Hyphae service.

## Security objectives

1. Hostile requests cannot create unbounded memory, CPU, queue, disk, or
   diagnostic growth.
2. Cancellation and shutdown release request-owned resources.
3. Route normalization and dispatch have one deterministic interpretation.
4. Rendering and error handling do not execute injected markup or disclose
   internal data by default.
5. Sessions, secrets, and authenticated data cannot cross users or tenants.
6. Cache keys, variants, staleness, and invalidation preserve authorization and
   privacy boundaries.
7. Extensions receive only explicitly granted capabilities and resources.
8. Deployment and version skew fail closed when required semantics differ.
9. Observability is useful without collecting request payloads or secrets.
10. Releases remain attributable, reproducible, and revocable.

## Assets

- process availability and bounded resource use;
- route, middleware, rendering, and cache semantics;
- session and CSRF keys;
- application secrets and resource handles;
- authenticated identity and authorization results;
- private loader/action inputs and outputs;
- public and private cache entries;
- runtime receipts, traces, logs, and diagnostics;
- PBOC artifacts and deployment manifests; and
- OpenSDK component bytes, manifests, locks, and effect receipts.

## Trust boundaries

| Boundary | Untrusted side | Trusted owner | Admission evidence |
| --- | --- | --- | --- |
| Network transport | Peers, proxies, malformed HTTP | Hyper/Tower host admission | Request-head/body policy and transport diagnostics |
| Route graph | Authored patterns, generated declarations | Sealed `pliego-router` graph | Grammar, collision, feature, and digest validation |
| Request scope | Headers, query, cookies, bodies | `pliego-runtime` scope | Typed decoding, limits, deadline, cancellation token |
| Rendering | Application values and errors | Escaping renderer and error boundary | Contextual encoding, output limits, commitment state |
| Resources | SQL, HTTP, storage, queues, Hyphae adapters | Typed resource registry | Capability, policy, timeout, cleanup contract |
| Session/secrets | Cookies, host-provided values | Session and secret handles | Key ID, rotation, bounds, non-serialization |
| Cache | Keys, variants, shared stores, invalidations | Cache policy and adapter | Namespace, Vary, privacy, TTL/stale, receipt |
| OpenSDK | Third-party component/process/ESM | Admission host and operator policy | Digest, package lock, imports, capabilities, budgets |
| Deployment | Artifact, operator config, provider | PBOC validator and host adapter | Exact-set digest, required features, compatibility report |
| Observability | Application fields and failure text | Redaction and bounded exporter | Attribute allowlist, cardinality bounds, local preview |

A declaration is not authority. Naming a capability, secret, cache, route, or
resource never creates access; the owning host must provide a validated handle.

## Adversaries

- unauthenticated remote clients;
- authenticated clients abusing their own permissions;
- cross-tenant attackers;
- malicious sites causing browser-originated requests;
- compromised upstream services;
- malicious or vulnerable OpenSDK extensions;
- application authors accidentally configuring unsafe policies;
- operators deploying incompatible artifacts or versions; and
- supply-chain attackers modifying dependencies, tools, or release assets.

The framework does not assume application handlers are sandboxed. Native Rust
application code has process authority unless the operator isolates it.

## Threats and required controls

### HTTP parsing and request admission

Threats:

- request smuggling through parser disagreement;
- ambiguous request targets and duplicate normalization;
- oversized heads, cookies, bodies, multipart parts, or decoded payloads;
- slowloris and unbounded request queues;
- decompression bombs; and
- trusted-proxy spoofing.

Controls:

- one upstream HTTP parser path per host and no secondary ad hoc parsing;
- strict single-pass percent decoding and path normalization;
- explicit count and byte limits before application code;
- encoded and decoded body ceilings;
- request/read/write deadlines, bounded concurrency, and overload response;
- no decompression without an independent decoded-byte budget; and
- trusted proxy configuration disabled unless explicit and topology-bound.

Verification includes differential target tests, fuzzing, slow peers, duplicate
header cases, malformed encodings, and fixed-memory load runs.

### Routing and middleware

Threats:

- route collision or shadowing;
- middleware bypass caused by path or method ambiguity;
- authorization placed after an unsafe rewrite or body consumer;
- internal handler exposure; and
- version-skew differences between hosts.

Controls:

- sealed deterministic graph with stable IDs and digest;
- build-time rejection of collisions and unsupported features;
- one documented middleware order;
- explicit rewrite, redirect, body-read, and response-mutation capabilities;
- conformance corpus shared by native and edge hosts; and
- deployment rejection for unknown required graph features.

### Rendering, errors, and metadata

Threats:

- cross-site scripting and unsafe URL/style contexts;
- secret or stack disclosure through error pages;
- response splitting through metadata;
- denial of service through recursive or oversized output; and
- inconsistent failure after stream commitment.

Controls:

- contextual escaping and typed safe wrappers with narrow constructors;
- bounded render depth, node count, and bytes;
- public error types separated from internal diagnostic chains;
- header value validation before commitment;
- immutable response commitment state; and
- functional no-JavaScript fallback errors.

### Streaming, cancellation, and shutdown

Threats:

- memory retained by abandoned clients;
- producers outpacing transport backpressure;
- hidden buffering that defeats limits;
- cleanup skipped when futures are dropped; and
- deployment shutdown terminating committed mutations or streams incorrectly.

Controls:

- backpressure-aware body resources;
- contagious cancellation from disconnect, deadline, abort, and shutdown;
- explicit commit points and drain deadlines;
- LIFO request-resource cleanup with acknowledgement; and
- memory plateau plus disconnect and shutdown conformance tests.

### Loaders, outbound requests, and resources

Threats:

- SSRF, DNS rebinding, redirect to forbidden networks, or credential forwarding;
- SQL/command/template injection in application adapters;
- connection pool exhaustion;
- resource handle confusion; and
- unbounded retry storms.

Controls:

- outbound HTTP clients are injected resources with allow/deny policy, redirect
  limits, destination re-evaluation, timeouts, and credential scoping;
- typed resource registry and capability intersection;
- bounded pools, retries, and circuit behavior owned by adapters;
- no ambient environment or network authority in loaders or extensions; and
- application DAL guidance that evaluates authorization next to the resource.

### Actions, forms, and uploads

Threats:

- CSRF and cross-origin mutation;
- duplicate mutation and idempotency confusion;
- multipart parser exhaustion and path traversal;
- mass assignment; and
- cancellation after an external commit.

Controls:

- Origin/SameSite/CSRF policy per action;
- typed allowlisted input schemas and decoded-byte limits;
- bounded streaming uploads into capability-confined storage;
- idempotency key bound to action, identity/session, input digest, and
  deployment contract; and
- explicit commit and compensation state in action receipts.

### Sessions and secrets

Threats:

- session fixation, theft, replay, and weak rotation;
- insecure cookie attributes;
- secret serialization, debug leakage, or build inclusion;
- key/version confusion; and
- user-controlled session state treated as authorization.

Controls:

- host-only Secure/HttpOnly/SameSite defaults where applicable;
- bounded versioned session payloads and rotation hooks;
- opaque secret handles with redacted formatting and no serialization;
- key IDs and version negotiation without values; and
- separate authentication and application authorization contracts.

### Cache and revalidation

Threats:

- private data stored in a public cache;
- cache poisoning through omitted query/header/identity inputs;
- authorization decisions served stale;
- tag invalidation crossing namespaces;
- inconsistent replicas after mutation; and
- unbounded keys, tags, or values.

Controls:

- separate build, immutable, public-runtime, and private-request domains;
- typed key and Vary policy with fail-closed required inputs;
- explicit freshness, stale, error, and authorization behavior;
- bounded namespaces, tags, keys, and values;
- causal invalidation receipts and replica acknowledgement; and
- two-instance cross-user and stale-divergence adversarial corpus.

### OpenSDK server extensions

Threats:

- capability self-grant;
- component resource exhaustion;
- path, network, clock, random, or environment access outside policy;
- package substitution and dependency confusion;
- buffered HTTP contracts presented as streaming; and
- process bridge protocol injection.

Controls:

- exact component digest and package lock;
- import-derived capabilities intersected with manifest and operator policy;
- fuel, wall time, memory, output, and concurrent-call budgets;
- host-provided resource handles only;
- standard WASI HTTP semantics where supported and explicit feature reporting;
- no silent stream-to-buffer downgrade; and
- bounded framed process protocols with stderr/stdout separation.

### Deployment and version skew

Threats:

- artifact substitution or partial upload;
- deployment of unknown required semantics;
- rolling replicas with incompatible session/cache/action behavior;
- rollback to an artifact that cannot read current state; and
- provider credentials entering portable output.

Controls:

- exact-set PBOC digest and provenance verification;
- required/optional feature negotiation before upload;
- declared compatibility for route, session, cache, action, and schema versions;
- rolling-deploy and rollback conformance cases; and
- provider credentials and private fields kept outside PBOC.

### Observability and diagnostics

Threats:

- secret, cookie, body, query, or personal data leakage;
- log injection;
- high-cardinality denial of service;
- attacker-controlled trace propagation; and
- support bundles capturing private filesystem data.

Controls:

- allowlisted attributes and stable codes;
- bounded value length and cardinality;
- validated trace context and new trust boundary at ingress;
- structured output rather than interpolated log lines; and
- local, previewable, redacted, bounded support bundles.

## ASVS baseline

G1 and G2 maintain a machine-reviewable control map for applicable OWASP ASVS
5.0 Level 2 requirements. `N/A` requires a written ownership reason. Framework
tests prove only framework-owned controls; applications and operators retain
their own control responsibilities.

Minimum mapped areas include architecture, authentication hooks, session
management, access-control integration, validation and encoding, cryptography
interfaces, error/log handling, data protection, communications, malicious
code, business-logic abuse, files/resources, API/web service, and configuration.

## Severity and release treatment

| Severity | Meaning | Gate treatment |
| --- | --- | --- |
| P0 | Remote compromise, cross-tenant disclosure, auth bypass, unbounded trivial DoS, or release-integrity failure | Blocks preview and stable release; owner and incident path required immediately |
| P1 | Material confidentiality, integrity, or availability failure requiring realistic preconditions | Blocks the affected gate until fixed or explicitly removed from scope |
| P2 | Defense-in-depth weakness or bounded failure with no demonstrated boundary escape | Requires tracked remediation and documented residual risk |
| P3 | Hardening or clarity improvement | May enter the normal backlog |

No G1 preview ships with an unresolved P0. A risk is not downgraded because the
feature is called preview.

## Required evidence by gate

### G1

- request parser/normalization differential corpus;
- route and middleware fuzz/adversarial tests;
- slow request/response and overload measurements;
- disconnect cancellation and graceful-shutdown cleanup plateau;
- render escaping/error/commitment cases;
- OTel redaction and cardinality tests; and
- runtime dependency and supply-chain audit.

### G2

- CSRF, session fixation/rotation, and cookie policy tests;
- SSRF and resource-timeout corpus;
- multipart/upload bounds;
- action idempotency and commit-state tests;
- two-instance public/private cache isolation and invalidation tests; and
- cross-user and cross-tenant leakage attempts.

The closed source-beta evidence and exact control ownership are recorded in
[the G2 evidence](evidence/g2-fullstack-beta.md) and
[`security/asvs-v5.0.0-g2.json`](../security/asvs-v5.0.0-g2.json). This map is
not an application compliance or certification claim.

### G3

- exact PBOC validation before upload;
- native/OCI and Cloudflare same-corpus conformance;
- feature-negotiation rejection cases;
- rolling version-skew and rollback tests; and
- proof that provider credentials are absent from portable output.

## Residual risks

- Native application and middleware code is trusted process code.
- Upstream HTTP/TLS/executor vulnerabilities remain supply-chain risks.
- Constant-time behavior is not guaranteed for arbitrary application code.
- A conformant contract cannot make an incorrectly authored authorization rule
  correct.
- Availability across regions depends on the chosen deployment operator.
- WASI and Component Model toolchains continue to evolve; unsupported semantics
  must be rejected rather than approximated.

These risks must remain visible in release documentation and cannot be erased
by a successful happy-path application demo.
