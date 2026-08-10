# ADR-010: Effects enqueue events for the next causal transaction

**Status:** Accepted for source-preview implementation
**Decision date:** 2026-08-09
**Scope:** causal event ordering, transaction phases, rollback, limits, and receipts

## Context

`pliego-reactive` safely batches graph writes and drains effects to quiescence,
but one reactive flush is not a causal application transaction. Effects can
write signals during the same flush, and those writes have no independent
transaction identity or state-root receipt.

`pliego-fold` already owns typed append, fail-closed reduction, canonical state,
snapshots, and StateRoot. It is therefore the smallest host-independent location
for proving event-to-projection causality before DOM planning is introduced.

## Decision

Add a single-threaded `CausalScheduler<S, E>` to `pliego-fold`.

The scheduler owns its `ReactiveLog` and `Projection`; callers do not receive a
writable log handle. Typed events are queued in two generations:

```text
ready -> Transaction N
events enqueued while N is active -> deferred -> Transaction N+1
```

The drain loop is iterative. Dispatch during an active transaction only queues
and never recursively reduces.

The observable phases are:

```text
Idle
Collecting
Reducing
Planning
Committing
RunningEffects
RecordingReceipt
Recovering
```

`Planning` currently binds the prepared projection point. DOM planning will
extend the coordinator later without redefining these phase names.

Each committed or rejected transaction emits a local deterministic receipt with
the event count and exact before/after `LogCursor + StateRoot`. Receipts contain
no event payload or state bytes and do not claim authority or provenance.

Default limits are 64 events per transaction, 256 transactions per drain, and
4,096 events per drain. Exceeding a limit fails explicitly and clears
non-executed queued work.

## Failure semantics

- Typed admission or projection failure rejects the complete transaction;
  before and after points are equal.
- Reducer/schema/codec panic is recoverable only on unwind-capable targets.
- Effect panic occurs after commit. The transaction remains committed, healthy
  effects finish, queued events run in the next transaction, and the returned
  error carries every emitted receipt.
- Stable production `wasm32-unknown-unknown` uses `panic=abort`; no browser
  post-panic recovery is claimed.
- A `DrainGuard` restores `Idle` and clears hidden queued work on unexpected
  unwinds.

## Consequences

- Effect-produced events are provably assigned to N+1.
- Receipt continuity makes transaction gaps and state-root drift inspectable.
- Existing direct `ReactiveLog::append_typed` remains compatible but is outside
  the causal scheduler guarantee.
- DOM commit, cancellation, external effect receipts, routing, and HMR remain
  later coordinator layers.
- This is not a reason to rename or reuse the native HTTP `pliego-runtime` crate.

## Rejected alternatives

- **Treat one reactive flush as one transaction:** no independent event
  generations or deterministic receipts.
- **Repeated public append for a multi-event transaction:** exposes intermediate
  state and flushes effects between events.
- **Recursive dispatch:** feedback depth becomes call-stack depth.
- **Rollback after effect panic:** external work may already have observed the
  committed state.
- **Global singleton scheduler:** prevents isolated tests and applications.

## References

- [StateRoot v1](ADR-009-state-root-v1.md)
- [Projection snapshot contract](ADR-005-projection-snapshots.md)
- [Next transition program](../52-pliegors-next-transition.md)
