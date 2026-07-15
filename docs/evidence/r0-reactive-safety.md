# R0 Reactive Safety Evidence

**Status:** Complete, with the panic-recovery contract scoped to targets that
support unwinding.

- Base commit: `f761e720f8f9cb16a6d1301f92f7c87c8eb8df69`
- Resulting runtime commit: `ff60575f8c0f16164ecfc1754eac165ae776d33a`
- Rust: `rustc 1.85.0 (4d91de4e4 2025-02-17)`
- Cargo: `cargo 1.85.0 (d73d2caf9 2024-12-31)`
- Node.js: `v24.16.0`
- Targets checked: Windows host tooling, Debian WSL2 native tests,
  `wasm32-unknown-unknown` stable Clippy, and supplemental WASM EH tests in
  Node.js.

## Scope and panic policy

On native and other unwind-capable targets, user callbacks execute outside the
global runtime borrow. Computation, observer, ownership, update, batching, and
flush state are restored before the first panic resumes. Healthy queued effects
are drained, failed provisional scopes are retired, and the last stable memo
value and dependency graph remain available for retry.

The production browser path remains the stable Rust
`wasm32-unknown-unknown` target. Its panic strategy is `abort`: a panic is a
terminal trap for that WASM instance, and PliegoRS does not promise recovery in
that artifact. This is a deliberate target contract, not evidence that unwind
guards ran. The separate WASM EH suite uses nightly, rebuilt `std`, and Node.js;
it is supplemental runtime evidence and is not a browser-production claim.

## Runtime design summary

- `NodeId` is a slot plus generation. Disposed slots return to a free list, and
  stale handles cannot resolve a later occupant.
- Computations collect sources, children, and cleanups in provisional frames.
  A successful run commits the frame atomically; a failed run discards only its
  provisional scope and leaves the stable scope dirty and retryable.
- `Signal::update` clones a candidate, releases the runtime borrow, executes
  user code, and publishes only after success. `Signal::set` remains available
  for non-`Clone` values.
- Observer, owner, batch, update, computation, and flush state use RAII guards.
- Disposal walks ownership iteratively, separates independent user-controlled
  fields, and runs callbacks and destructors outside the runtime borrow.
- Effects use `VecDeque` FIFO order and a per-node `queued` flag for constant-time
  deduplication. Failed memo chains are rearmed across already-colored DAGs.
- A flush has a deterministic runaway budget:
  `min(1_000_000, max(1_024, 256 * live_nodes_at_flush_start))`. Exceeding it
  emits a diagnostic panic, stops executing callbacks, clears queued flags, and
  rearms dirty memo chains for the next invalidation. A 101-stage finite chain
  is an explicit non-regression test; unique-ID churn is an explicit rejection
  test.

## Behavioral policies

- Same-signal reentrancy: rejected with a deterministic panic; the stable value
  and guards are preserved.
- Writes to a direct or transitive source of an active computation: rejected.
- Nested writes to other signals: committed and batched until the outer update
  finishes; their healthy effects run before an outer panic resumes.
- Scheduler order: FIFO by first enqueue; duplicate invalidations in one batch
  do not add duplicate queue entries.
- Cleanup order: descendants before owners, newest child first, and LIFO within
  each scope. On a successful rerun, the replacement frame commits before the
  previous scope is retired. On a failed rerun, the stable scope remains alive.
- Cleanup panic: remaining cleanups and independent node fields are processed;
  the first panic resumes after runtime invariants are restored.

## Acceptance matrix

| ID | Evidence | Result |
| --- | --- | --- |
| R0-A01 | `gate_diamond_runs_effect_once` | PASS |
| R0-A02 | `gate_untaken_branch_not_tracked` | PASS |
| R0-A03 | `gate_equality_gate_stops_propagation` | PASS |
| R0-A04 | `memo_is_lazy` | PASS |
| R0-A05 | `observer_is_restored_and_failed_memo_can_retry` | PASS |
| R0-A06 | `flushing_is_restored_after_effect_panic`, scheduler panic tests | PASS |
| R0-A07 | `update_runs_without_runtime_borrow_and_rolls_back_on_panic` | PASS |
| R0-A08 | Transactional update panic regression | PASS |
| R0-A09 | `nested_updates_are_batched_and_same_signal_reentrancy_is_rejected` | PASS |
| R0-A10 | `rerun_disposes_previous_children_exactly_once` | PASS |
| R0-A11 | `recursive_disposal_is_lifo_and_idempotent` | PASS |
| R0-A12 | Idempotent second disposal in recursive disposal test | PASS |
| R0-A13 | `stale_handle_cannot_resolve_reused_slot` | PASS |
| R0-A14 | `scheduler_is_fifo_and_deduplicates_within_a_batch` | PASS |
| R0-A15 | FIFO ordering assertion in scheduler test | PASS |
| R0-A16 | `arena_reaches_plateau_after_ten_thousand_cycles` | PASS |
| R0-A17 | `pliego-fold`, `pliego-dom`, and workspace gates | PASS |
| R0-A18 | Stable WASM Clippy with warnings denied | PASS |

Additional adversarial coverage includes transitive source writes, automatic
and manual memo recovery, equal-value recovery, cleanup panics, independent
panicking destructors, deep iterative disposal, effect feedback cycles, finite
effect chains, unique-ID scheduler churn, queue-abort rearming, and thread-local
handle confinement.

## 10,000-cycle measurements

- Baseline total slots: `0`
- Peak/plateau total slots: `2`
- Final total slots: `2`
- Final live nodes: `0`
- Final free slots: `2`
- Final pending effects: `0`
- Final ownership, source, subscriber, and cleanup counts: `0`

Each cycle creates one signal and one effect, then disposes both. The arena
reuses the same two generational slots instead of growing with 10,000 cycles.

## Commands executed

- `cargo fmt --check`: PASS
- `cargo clippy -p pliego-reactive --all-targets --locked -- -D warnings`: PASS
- `cargo test -p pliego-reactive --locked`: PASS (`36` unit tests and `1`
  compile-fail doctest; final native replay used Debian WSL2 because Windows
  Application Control blocked a regenerated local test executable)
- `cargo test -p pliego-fold --locked`: PASS (`5` tests)
- `cargo test -p pliego-dom --locked`: PASS (`5` tests)
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS
- `PLIEGORS_SOURCE_REV=ff60575f8c0f16164ecfc1754eac165ae776d33a cargo test --workspace --all-targets --locked`: PASS
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`: PASS
- `cargo clippy --target wasm32-unknown-unknown --locked -p pliegors-site-client -p spike -- -D warnings`: PASS
- `npm run check:docs`: PASS
- `RUSTFLAGS="-Cpanic=unwind -Cllvm-args=-wasm-use-legacy-eh=false" CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo +nightly-2026-06-26 -Zbuild-std=std,panic_unwind test -p pliego-reactive --target wasm32-unknown-unknown --test wasm_unwind --locked`: PASS (`5` Node.js WASM EH tests)

No R0 test is ignored, and no lint allowance was added to hide a failure.
The final workspace replay supplied the verified source revision because the
minimal Debian WSL image has no `git` executable. The first replay failed closed
only in the existing starter-provenance test; supplying the documented revision
input made the unchanged full workspace gate pass.

## Known residual risks

- Transactional `update` protects the stored value. A `Clone` implementation
  that shares interior mutable state can still mutate that shared state; safe
  Rust cannot provide a deep rollback for arbitrary `T`.
- Rust aborts on a second panic from within one destructor that is already
  unwinding. PliegoRS isolates independent node fields and cleanup callbacks,
  but it cannot repair a type whose own destructor double-panics internally.
- A first evaluation of an extremely deep, user-authored lazy memo chain still
  follows nested user closures. A synthetic 50,000-level chain can exhaust the
  native stack even though graph pulling and disposal use explicit stacks.
- The scheduler budget is a safety boundary, not proof of semantic convergence.
  Exceeding it aborts that flush explicitly and rearms discarded dirty chains.
- Stable browser WASM uses terminal `panic=abort`. The supplemental EH tests run
  in Node.js and do not certify browser support or the production build path.
