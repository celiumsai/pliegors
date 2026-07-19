# RFC-009: Full-stack route graph

**Status:** Draft  
**Owner:** Router  
**Target gate:** G1  
**Created:** 2026-07-19

## Summary

PliegoRS will compile one explicit route graph for static generation, dynamic
rendering, route handlers, layouts, middleware, errors, cache policy, telemetry,
and deployment inspection. File placement may generate declarations, but the
sealed graph is the authority.

This RFC extends the existing collision-safe static route model. It does not
replace it with string matching at request time.

## Goals

- Static, dynamic, optional, and catch-all path segments.
- Nested layouts and pathless route groups.
- Method-specific page and resource handlers.
- Typed decoded parameters and normalized query access.
- Pre-route and route middleware with deterministic order.
- Authored not-found, forbidden, unauthorized, and failure boundaries.
- Stable route IDs for diagnostics, cache, deployment, and telemetry.
- One conformance corpus shared by build-time and server hosts.

## Path grammar

Authored patterns use a bounded portable grammar:

```text
segment          = literal | :parameter | :parameter? | *catch_all
route            = / segment *( / segment )
group            = (name)  # graph organization only; no URL segment
```

Literal segments pass the existing portable namespace rules. Parameter names
are lowercase ASCII identifiers and cannot repeat within one route. Optional
parameters may only appear in a suffix whose expansions are collision-checked.
A catch-all is terminal and captures at least one segment unless explicitly
declared optional by a future grammar version.

The route compiler rejects:

- parent traversal, encoded separators, NUL, backslash, and invalid percent
  encoding;
- portable case or Unicode aliases;
- literal/parameter ambiguity at the same precedence;
- duplicate method and pattern pairs;
- a page and resource handler that claim the same method;
- unreachable error or layout nodes; and
- any expansion beyond configured route-count and depth bounds.

## Matching and decoding

The host parses the request target once. Percent decoding is strict and
performed once per path segment after the transport rejects malformed bytes.
Decoded separators never create new segments. Matching order is deterministic:

1. literal;
2. required parameter;
3. optional parameter expansion;
4. catch-all.

Generated parameter types expose admitted UTF-8 values. Handlers never receive
the undecoded raw path as authority. The raw target may be retained only as a
bounded diagnostic field with redaction.

## Route node

Each sealed node declares:

- stable route ID;
- authored pattern and compiled matcher digest;
- methods;
- parent layout and route group;
- render or resource handler identity;
- loader and action identities;
- middleware sequence;
- error boundary sequence;
- cache policy identity;
- capability requirements; and
- supported runtime features.

Changing any behavior-bearing field changes the graph digest. Route IDs remain
stable across refactors only when application authors preserve them explicitly.

## Layouts and groups

Layouts form an acyclic ownership tree. A layout may own loaders, head metadata,
error boundaries, and cleanup. Child output is inserted through one declared
slot; implicit multiple insertion is forbidden.

Groups organize routes and middleware without changing the public URL. Two
groups that resolve to the same method and route are a compile-time collision.

## Pages and resource handlers

Page handlers produce a declared PliegoRS render mode. Resource handlers
produce typed HTTP responses and may stream only when the active runtime reports
that capability. A single route may provide different method handlers, but
`HEAD` derives from `GET` only when the response contract permits it.

Automatic `OPTIONS`, redirects, and trailing-slash behavior are versioned route
policies. They are visible in `pliego inspect routes` and cannot vary silently
between native and edge hosts.

## Middleware order

The graph stores middleware rather than discovering it at runtime. Order is:

1. root pre-route middleware;
2. nested group middleware from root to leaf;
3. nested layout middleware from root to leaf;
4. route middleware;
5. handler;
6. response middleware from leaf to root before commitment.

Middleware declares whether it can rewrite, redirect, reject, read a body, or
mutate response headers. A body-consuming middleware must publish the resulting
body resource or no later layer may read it again.

## Error boundaries

Errors are typed into four public classes:

- not found;
- unauthorized or forbidden;
- invalid request;
- internal failure.

Application boundaries may render bounded public responses. They do not receive
secrets or internal diagnostic chains by default. If a boundary fails, the
runtime walks outward. The final fallback belongs to PliegoRS and remains
functional without JavaScript.

Failures after response commitment terminate the stream and emit diagnostics;
they cannot change the status or render a second page.

## Inspection and diagnostics

The route compiler emits stable codes for syntax, collision, ambiguity,
unsupported capability, and ownership errors. Planned commands are:

```text
pliego inspect routes
pliego why route <route-id>
pliego why request <request-id>
```

Inspection reports matcher order, inherited layouts/middleware, loaders,
actions, cache policy, runtime requirements, and graph digest without exposing
secret values.

## Portability

PBOC records the sealed route graph and required features. A deployment host
must reject unknown required grammar or route features before accepting an
artifact. Host-specific routes may be namespaced extensions, but cannot alter
the meaning of portable routes.

## Acceptance evidence

- Property tests for grammar normalization and collision behavior.
- Fuzz targets for pattern parsing, target decoding, and matching.
- Golden corpus shared by static, native, and Cloudflare implementations.
- Nested layout, middleware, error, and method dispatch tests.
- Stable diagnostics and deterministic graph digests across replicas.
- Route-scale build and match benchmarks with bounded memory.

## Alternatives rejected

- **Runtime order of registration:** rejected because imports and plugin order
  would change matching semantics.
- **Filesystem paths as the authority:** rejected because source layout is one
  authoring tool, not the deployment contract.
- **Opaque regular expressions:** rejected because they cannot provide portable
  typed parameters, deterministic precedence, or bounded conformance.
- **Next.js route semantics by name:** rejected because compatibility requires
  explicit behavior and evidence, not copied directory conventions.
