# R2 Verified Sync Evidence

**Status:** implementation documented; final acceptance commands and resulting
commit are not recorded yet. R2 remains active until this record is completed
against one committed implementation tree.

## Security objective and threat model

R2 makes the modern Hyphae client contract fail closed before untrusted network
data can reach an application reducer or be represented as durable authority.
It is designed to reject:

- protocol downgrade, unknown selection modes, unknown event versions, and
  malformed identifiers;
- missing, unknown, revoked, stream-unauthorized, mismatched, or invalid
  signature authority;
- receipt, batch, request, page, completion, event-set, cursor, or snapshot
  substitution;
- zero-based or discontinuous durable sequences, hash-chain gaps, equal-position
  forks, cross-stream replay, snapshot rollback, and cursor jumps;
- duplicate or moved event identities inside the bounded replay window; and
- direct application of raw or merely validated pages through the public Rust
  API.

The crate verifies a protocol contract through a caller-supplied trust store. It
does not ship production keys, choose a signature algorithm, authenticate a
gateway, implement Hyphae storage, or prove that an authorized signer is honest.

## Protocol v2 and downgrade boundary

The modern wire constant is exactly `pliego-hyphae/2`. Event envelopes, append
batches, append responses, pull requests, and pull pages reject any other
protocol. Append preconditions and pull cursors use explicit
`StreamCursor::genesis()` rather than an absent value with two possible
meanings.

`PullSelection` contains only `WholeStream`. Serde rejects an unknown or partial
selection rather than interpreting it locally. `SnapshotSelection::Latest`
starts a signed pull cycle; `SnapshotSelection::Exact(cursor)` fixes every
continuation to the same authenticated checkpoint.

The historical M5 ACK seam is not protocol v2. It is compiled only through the
non-default `experimental-legacy` feature and has no conversion into a verified
append, page, event, or replay state.

## Canonical signed claims

All variable-length fields use explicit length prefixes and all integer fields
use fixed big-endian encoding. Three literal domains prevent cross-purpose
signature reuse:

| Claim | Domain | Bound values |
| --- | --- | --- |
| Receipt | `pliego-hyphae/2/receipt` | Protocol, client event, stream, envelope hash, absolute sequence, durable hashes, commit time, and key ID. |
| Append attestation | `pliego-hyphae/2/append` | Authority, batch ID, stream, expected and next cursors, event count, ordered event/receipt digest, commit time, and key ID. |
| Page attestation | `pliego-hyphae/2/page` | Authority, request ID, stream, request cursor, snapshot mode, whole-stream selection, limit, next and snapshot cursors, completion, event count, ordered event/receipt digest, issuance time, and key ID. |

The ordered event digest commits to each canonical envelope hash, complete
receipt signing payload, and receipt signature. Golden-vector tests pin all
three domains independently.

Every pull page carries a page attestation, including an empty page. The
`complete` bit must equal `next_cursor == snapshot_cursor`. An empty complete
page must also preserve the request cursor. This proves completion at one signed
checkpoint, not permanent absence of future events.

Every receipt sequence is absolute and contiguous from the request or append
cursor. Sequence zero, the empty-stream sentinel as an accepted hash or journal
head, and hash self-loops fail before signature verification.

## Authority contract

`ReceiptVerifier` receives `VerificationContext` containing signature purpose,
claimed logical authority when present, stream, signed time, and key ID. It
returns a validated `AuthorityId` or one structured failure:

- `UnknownKey`;
- `RevokedKey`;
- `UnauthorizedStream`;
- `InvalidSignature`;
- `Unavailable`; or
- `AuthorityMismatch`.

Append and page attestations verify before their individual receipts. Every
receipt must resolve to the same logical authority as the enclosing attestation.
Different key IDs are allowed when the verifier maps an authorized rotation to
that same authority.

## Typestate and API closure

Pull replay follows one consuming path:

```text
PullRequest + PullPage
    -> UntrustedPullPage
    -> ValidatedPullPage
    -> VerifiedPullPage
    -> AppliedPullPage
```

Validated and verified wrappers have private fields and are not deserializable.
`VerifiedPullPage` is not cloneable and its `apply` method consumes it. Raw
`PullPage`, `AcceptedEvent`, and `ValidatedPullPage` have no apply method.
`ReplaySink` accepts only `VerifiedAcceptedEvent`, whose immutable accessors
expose the authenticated envelope, receipt, and authority.

Append retry returns `ValidatedAppendResponse`, never a durability claim. Its
consuming verification transition checks the append attestation and every
receipt before producing `VerifiedAppendResponse`.

Six compile-fail doctests cover raw apply, validated apply, construction of a
verified page, passing raw events to a sink, applying an `AppliedPullPage`, and
using a consumed `VerifiedPullPage` twice.

## Stream-bound replay

`ReplayState::new(stream_id)` starts at explicit genesis and is permanently
bound to that canonical stream. The first applied page binds a logical
authority. A fixed snapshot remains active until its cursor is reached, and a
later cycle may advance but cannot roll back or fork the authenticated head.

The state retains at most 1,025 position-indexed anchors. Each event anchor
binds absolute position, durable head, client event ID, and server hash. Exact
overlap is idempotent. A changed hash, moved ID, unknown old position, future
cursor, cross-stream page, or equal-position fork fails closed.

Application and framework mutation are ordered deliberately:

1. revalidate current stream, cursor, snapshot, authority, overlap, and all
   candidate state;
2. calculate every fallible result needed for the return value;
3. call the reducer once only when the fresh verified set is non-empty; and
4. publish the candidate `ReplayState` only after reducer success.

The reducer contract itself must be transactional. R2 can leave framework state
unchanged after a reducer error, but cannot undo arbitrary mutations performed
inside user code.

## Event-version policy

Wire validation requires a positive schema version but does not equate that
with application compatibility. `EventVersionPolicy` must explicitly admit
each `(kind, schema_version)` before a page can become verified. R2 rejects an
unknown pair; R3 owns upcasters, reducer identity, and snapshot schema evolution.

## Acceptance matrix

| ID | Evidence | Result |
| --- | --- | --- |
| R2-A01 | Six compile-fail typestate and sink-boundary doctests | NOT RECORDED |
| R2-A02 | `v2_rejects_v1_and_unknown_selective_replay` | NOT RECORDED |
| R2-A03 | `append_response_requires_exact_one_based_sequences`, `pull_page_rejects_sequence_gaps_and_zero` | NOT RECORDED |
| R2-A04 | `append_attestation_rejects_receipt_set_mutation_before_receipt_verification` | NOT RECORDED |
| R2-A05 | `append_attestation_blocks_batch_substitution_and_authority_mismatch` | NOT RECORDED |
| R2-A06 | `empty_page_is_attested_and_never_calls_sink` | NOT RECORDED |
| R2-A07 | `page_attestation_binds_request_snapshot_limit_completion_and_events` | NOT RECORDED |
| R2-A08 | `signed_request_mutation_fails_verification`, `signed_snapshot_and_completion_mutation_fails_verification` | NOT RECORDED |
| R2-A09 | `authority_errors_are_structured_and_fail_closed` | NOT RECORDED |
| R2-A10 | `append_verification_accepts_key_rotation_within_one_authority` | NOT RECORDED |
| R2-A11 | `invalid_receipt_signature_never_produces_verified_page` | NOT RECORDED |
| R2-A12 | `unknown_event_version_fails_before_replay_sink_exists` | NOT RECORDED |
| R2-A13 | `stream_and_future_cursor_substitution_fail_before_verification` | NOT RECORDED |
| R2-A14 | `fork_verified_before_state_change_is_rejected_before_sink` | NOT RECORDED |
| R2-A15 | `verified_replay_applies_once_and_deduplicates_absolute_overlap` | NOT RECORDED |
| R2-A16 | `exact_snapshot_continuation_cannot_change_cycle_head` | NOT RECORDED |
| R2-A17 | `authority_is_bound_to_replay_state` | NOT RECORDED |
| R2-A18 | `reducer_error_leaves_replay_state_unchanged` | NOT RECORDED |
| R2-A19 | `receipt_rejects_zero_hash_self_loop_and_impossible_journal_head` | NOT RECORDED |
| R2-A20 | `canonical_signature_payloads_have_distinct_golden_vectors` | NOT RECORDED |
| R2-A21 | `legacy_ack_is_explicitly_feature_gated_and_unverified` | NOT RECORDED |

## Verification record

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | NOT RECORDED |
| `cargo test -p pliego-hyphae --no-default-features --locked` | NOT RECORDED |
| `cargo test -p pliego-hyphae --all-features --locked` | NOT RECORDED |
| `cargo test -p pliego-hyphae --doc --locked` | NOT RECORDED |
| `cargo clippy -p pliego-hyphae --all-targets --all-features --locked -- -D warnings` | NOT RECORDED |
| `cargo clippy -p pliego-hyphae -p spike --target wasm32-unknown-unknown --all-features --locked -- -D warnings` | NOT RECORDED |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | NOT RECORDED |
| `cargo test --workspace --all-targets --locked` | NOT RECORDED |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | NOT RECORDED |
| `npm run check:docs` | NOT RECORDED |
| `npm run check:distribution` | NOT RECORDED |
| `git diff --check` | NOT RECORDED |

- Base commit: `5253950`
- Resulting implementation commit: NOT RECORDED
- Rust/Cargo/Node versions: NOT RECORDED
- Targets checked: NOT RECORDED

## Known residual risks

- `ReceiptVerifier` is an enforcement boundary, not a cryptographic or PKI
  implementation. Production key material, algorithms, discovery, pinning,
  rotation, revocation, and incident recovery remain external.
- An attestation proves what its authorized signer asserted at one time. It does
  not prove signer honesty, eternal stream completeness, or correct server
  storage.
- Raw Serde collections allocate before structural validation. Concrete
  transports must enforce response-body and time limits before deserializing.
- Replay anchors are intentionally bounded. Overlap older than the retained
  window is rejected rather than accepted or retained indefinitely.
- Reuse of an event ID after its anchor has been evicted depends on the trusted
  authority enforcing global stream uniqueness; the bounded client alone cannot
  remember every historical ID.
- `ReplayState` and the outbox are in-memory contracts. Durable persistence,
  crash recovery, and coordinated commit with application projection state are
  not implemented in R2.
- The reducer must apply its supplied batch transactionally. PliegoRS does not
  roll back arbitrary user state after a reducer violates that contract.
- The transport traits do not implement authentication, tenant isolation,
  quotas, cancellation, jittered backoff, telemetry redaction, or bounded
  network decoding.
- R2 rejects unknown event versions. Upcasting and snapshot/reducer identity are
  deferred to R3.
- The legacy feature remains unverified by design and must never be used as
  provenance or verified durability evidence.
