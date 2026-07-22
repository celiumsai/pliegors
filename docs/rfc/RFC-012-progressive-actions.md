# RFC-012: Progressive actions and commit semantics

**Status:** Draft for G2 preview
**Owner:** Runtime and Data
**Parent:** [RFC-010](RFC-010-data-actions-cache.md)
**Created:** 2026-07-21

**Implementation:** Complete on `main` for the G2 source beta; see
[G2 evidence](../evidence/g2-fullstack-beta.md).

## Decision

An action is one typed server mutation with an ordinary HTML form contract and
an optional enhanced client transport. Both paths invoke the same admitted
action definition. JavaScript cannot create a second authorization, validation,
idempotency, or invalidation path.

Actions are POST-oriented by default. Other unsafe methods require an explicit
resource-handler contract and are not inferred from file names.

## Action policy

Every action declares:

- stable ID and semantic revision;
- accepted media types and maximum encoded/decoded bytes;
- input, field-error, and success schema IDs;
- Origin, SameSite, and CSRF policy;
- authentication and authorization hooks;
- idempotency scope, retention, and maximum result bytes;
- resources and transaction boundary;
- commit-point owner;
- causal cache invalidation intents; and
- success/failure navigation for an ordinary form submission.

Unknown fields fail by default. Mass assignment through automatic struct or
model mapping is outside the framework contract.

## Progressive response

The non-JavaScript path returns a standards-based redirect or authored HTML
response. Field errors preserve labels, focus destination, submitted safe
values, and an accessible summary. Secret fields are never reflected.

The enhanced path may render pending state and apply an in-place result, but it
uses a versioned wire envelope and preserves history, focus, and form semantics.
If the client and server action revisions differ, the client falls back to a
full navigation instead of approximating the result.

## CSRF and authorization

Unsafe browser-originated actions must pass:

1. request method and media-type admission;
2. configured Origin/Referer policy;
3. SameSite expectations;
4. a session-bound, action-bound CSRF proof when policy requires it;
5. authentication; and
6. application authorization evaluated next to the protected resource.

CSRF success does not imply authorization. Authentication middleware does not
authorize an action by placement alone. Every invocation runs the declared
hooks.

## Idempotency

The idempotency identity binds:

- action ID and semantic revision;
- authenticated principal or anonymous session partition;
- admitted input digest;
- deployment compatibility epoch; and
- caller-supplied or form-generated idempotency key.

Reusing a key with a different admitted input or partition is a conflict. A
completed result may be replayed only within its declared retention and result
bound. In-progress duplicates receive a bounded wait or conflict policy; they
do not execute a second mutation silently.

## Commit state

The runtime records these states:

```text
not-started -> pre-commit -> committing -> committed
                         \-> failed
                         \-> outcome-unknown
committed -> compensation-required
```

The application or resource adapter calls the commit marker at the smallest
boundary it can prove. PliegoRS does not claim atomicity for an external system.
Cancellation before commit stops work. Cancellation during or after commit
records `outcome-unknown` or `committed` and allows bounded reconciliation;
the runtime never reports an unproven rollback.

## Uploads and decompression

Form, JSON, and multipart parsers are established libraries behind one
framework admission boundary. Encoded bytes, decoded bytes, field count, part
count, per-part bytes, total temporary storage, filename length, nesting, and
parse time are bounded independently. Filenames never become filesystem paths.
Temporary artifacts are capability-confined and deleted on cancellation,
failure, or scope cleanup.

## Acceptance evidence

ACT-001 through ACT-003 and UPL-001 require:

- the same authenticated mutation succeeds with JavaScript disabled/enabled;
- hostile Origin, CSRF, cookie, content type, and authorization cases fail;
- duplicate retries execute one committed mutation;
- conflicting key reuse fails closed;
- cancellation before, during, and after commit yields truthful state;
- field errors are typed and accessible;
- multipart/decompression corpora remain inside memory/disk/time budgets; and
- action receipts redact bodies, fields, identities, tokens, and secrets.
