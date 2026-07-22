# RFC-013: Sessions, cookies, and secret handles

**Status:** Draft for G2 preview
**Owner:** Runtime and Security
**Parent:** [RFC-010](RFC-010-data-actions-cache.md)
**Created:** 2026-07-21

**Implementation:** Complete on `main` for the G2 source beta; see
[G2 evidence](../evidence/g2-fullstack-beta.md).

## Decision

PliegoRS owns a session and cookie contract, not an identity product. OIDC,
OAuth, WebAuthn, password authentication, directories, and application RBAC are
integrations. A session carries bounded authenticated state; it never grants
resource authorization by itself.

## Cookie defaults

Production application sessions default to:

- host-only scope with no `Domain` attribute;
- `Secure`;
- `HttpOnly` unless a separately named browser-readable cookie is required;
- `SameSite=Lax`;
- `Path=/` or a narrower explicit path;
- bounded `Max-Age` and absolute expiry; and
- the `__Host-` prefix when the deployment topology permits it.

Cross-site flows require a named policy, explicit `SameSite=None; Secure`, CSRF
coverage, and a threat-model amendment. Public suffix and broad-domain cookies
are rejected by the reference policy.

## Session envelope

The cookie contains either a cryptographically authenticated opaque session ID
or a bounded authenticated/encrypted envelope produced by an established
cryptographic implementation. PliegoRS does not implement primitives.

The logical envelope declares:

- contract and schema version;
- session ID digest for diagnostics, never the raw ID;
- creation, last-rotation, idle-expiry, and absolute-expiry epochs;
- key ID and algorithm suite ID;
- authentication assurance and provider reference;
- CSRF binding revision; and
- bounded application claims with an explicit schema.

Cookie bytes, decoded payloads, claim count, and individual values are bounded.
Unknown versions and algorithms fail closed.

## Rotation and fixation

Authentication, privilege elevation, recovery, credential change, and operator
policy can require rotation. Rotation creates a new session ID and CSRF binding,
invalidates the old identifier through the configured store, and preserves only
allowlisted state. The reference tests prove an attacker-chosen pre-auth ID
cannot survive login.

Key rotation supports an active write key plus bounded read-only predecessors.
Key identifiers are visible; key values are not. Rolling replicas must share a
compatible key and session schema window before deployment admission.

## Session store

The store API supports create, read, rotate, revoke, and bounded expiry cleanup.
The in-memory adapter is development-only. Distributed adapters declare their
consistency and revocation lag; absence of cross-replica revocation support is
machine-visible and cannot claim G2 multi-instance conformance.

## Secret handles

Secrets are registered operator resources. `SecretHandle` exposes only stable
secret ID, version, rotation identity, optional length class, and a controlled
borrow/callback operation owned by the integration that needs the bytes. It is
not serializable, cloneable into an unconstrained value, or printable.

Source, PBOC, logs, traces, errors, receipts, support bundles, and debug output
must pass a canary corpus proving secret values are absent.

## Acceptance evidence

SES-001 closes only when tests prove secure defaults, rejection of unsafe
cookie combinations, fixation prevention, rotation and revocation across two
instances, expiry behavior, version-skew rejection, CSRF rebinding, and secret
non-serialization/redaction.
