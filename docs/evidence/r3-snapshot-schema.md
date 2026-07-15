# R3 Snapshot and Schema Evidence

**Status:** complete. The implementation and adversarial acceptance matrix were
verified against commit `c341ed271f52510d5098482a297195bbb6448ec4` on
2026-07-15.

## Acceptance boundary

R3 is accepted only when the exact committed implementation proves all of the
following as one contract:

- typed, versioned `app_*` events enter reducers only through a sealed catalog;
- typed append proves an exact serialize/decode/value round trip before an
  `Event` or log mutation exists;
- payloads, reducer configuration, codec configuration, and JSON state use
  deterministic bounded canonical bytes;
- every schema migration is an explicit validated upcast edge;
- projection batches encode and validate state bytes before publishing state,
  bytes, cursor, and counters transactionally;
- snapshots bind the exact history head, catalog, reducer, configuration,
  codec, state bytes, and complete snapshot metadata;
- restore fails closed before state installation on any mismatch; and
- deterministic generated-case tests show live fold, full replay, and
  snapshot-plus-tail parity.

R3 state and snapshot digests are integrity checks, not authority. Signing,
key distribution, revocation, and PKI remain external. The verified Hyphae
history contract is covered separately by
[R2 evidence](r2-verified-sync.md).

## Threat model

Acceptance tests must reject:

- unknown, zero, excessive, or incorrectly namespaced application event schemas;
- asymmetric schema serialization, including omitted required fields or
  omitted non-default values restored through a default;
- duplicate, non-adjacent, missing, cross-kind, or observably nondeterministic
  mappers and upcasts;
- catalog mutation after sealing or digest dependence on registration order;
- state publication after partial reducer, mapper, upcaster, codec, or
  validation failure;
- history position, history hash, catalog, reducer, reducer configuration,
  codec identity/revision/configuration, state, or metadata substitution during
  restore;
- same-position history forks and snapshots from another projection contract;
- corrupt, oversized, trailing, or otherwise malformed snapshot bytes; and
- divergence among live application, full replay, and snapshot-tail replay.

The R3 package is not expected to prove that an authorized Hyphae signer is
honest, bind stream authority inside a local projection snapshot, implement
production snapshot storage, or authenticate a snapshot received across a
trust boundary.

## Acceptance matrix

Every row was executed against the exact implementation commit recorded below.

| ID | Required evidence | Result |
| --- | --- | --- |
| R3-A01 | `schema_constants_fail_closed` rejects invalid `app_*` identity, zero, and out-of-contract versions. | PASS |
| R3-A02 | `typed_append_requires_an_exact_schema_value_round_trip`, `catalog_rejects_unknown_kind_and_version`, and `typed_u128_payload_round_trips_through_its_exact_catalog_schema` prove exact typed append and catalog admission. | PASS |
| R3-A03 | `canonical_json_sorts_objects_and_removes_whitespace`, `canonical_json_normalizes_equivalent_decimal_lexemes_without_float_rounding`, `typed_serialization_rejects_non_finite_floats_at_any_depth_in_one_pass`, duplicate/reserved-key tests, and `canonical_json_enforces_byte_depth_and_node_bounds` cover canonical payloads. | PASS |
| R3-A04 | `catalog_rejects_cross_kind_skipped_duplicate_and_missing_edges` rejects duplicate schema/current/edge registrations and incomplete graphs. | PASS |
| R3-A05 | The same graph test rejects cross-kind/non-adjacent edges, missing current schemas, missing adjacent edges, gaps, and schemas past current. | PASS |
| R3-A06 | `catalog_upcasts_to_typed_current_payload` and `r3_a22_sealed_catalog_upcasts_version_mix_before_reduction` exercise a complete adjacent chain into typed reducer input. | PASS |
| R3-A07 | `upcaster_failure_and_nondeterminism_fail_closed`, `current_mapper_identity_and_observed_output_fail_closed`, and `r3_a18_schema_err_and_panic_never_publish_candidate_state` reject callback failure, panic, and observed divergence. | PASS |
| R3-A08 | `sealed_catalog_is_send_sync_and_has_no_mutation_api` exercises the sealed admission surface; builder mutation is absent from the sealed type. | PASS |
| R3-A09 | `catalog_digest_is_registration_order_independent` and `catalog_digest_binds_schema_step_and_mapping_ids` bind schema, mapper, target, and upcast identities independently of registration order. | PASS |
| R3-A10 | `r3_a09_snapshot_binds_history_schema_reducer_codec_and_state`, `restore_binds_codec_configuration_not_only_its_name`, and `snapshot_digest_binds_every_contract_field` exercise reducer and codec ID/revision/configuration bindings. | PASS |
| R3-A11 | `r3_a16_reducer_err_discards_the_whole_batch` proves reducer `Err` cannot partially publish state, cached bytes, cursor, or counter. | PASS |
| R3-A12 | `r3_a17_reducer_panic_discards_the_whole_batch_and_is_reported`, `r3_a18_schema_err_and_panic_never_publish_candidate_state`, `codec_rejection_and_panic_never_publish_candidate_state`, and `codec_global_limit_is_enforced_before_candidate_publication` exercise unwind-target failure atomicity. | PASS |
| R3-A13 | `r3_a09_snapshot_binds_history_schema_reducer_codec_and_state` checks exact position/head, catalog, reducer, full codec identity, state bytes, and both digests. | PASS |
| R3-A14 | `envelope_and_state_digests_match_golden_vectors` and `snapshot_digest_binds_every_contract_field` pin the digest and mutate every bound field. | PASS |
| R3-A15 | `r3_a10_decoder_rejects_every_truncation_trailing_and_oversize_input` and `envelope_rejects_corruption_truncation_and_trailing_data` cover the bounded binary decoder. | PASS |
| R3-A16 | `r3_a11_state_and_snapshot_corruption_fail_closed` distinguishes state-digest and complete-envelope corruption. | PASS |
| R3-A17 | `r3_a12_restore_rejects_schema_reducer_and_codec_mismatch` and `restore_binds_codec_configuration_not_only_its_name` reject contract substitution before installation. | PASS |
| R3-A18 | `r3_a14_restore_rejects_fork_at_snapshot_position` and `r3_a15_restore_rejects_snapshot_ahead_of_history` reject a local content-head fork or unavailable prefix. | PASS |
| R3-A19 | `r3_a13_restore_rejects_noncanonical_codec_output` and the codec mismatch cases reject noncanonical state and full codec-identity mismatch. | PASS |
| R3-A20 | `r3_a19_restore_folds_exactly_the_tail` proves successful restore resumes strictly after the exact local head. | PASS |
| R3-A21 | `r3_a20_live_genesis_and_snapshot_tail_are_equal_for_deterministic_cases` compares live, full replay, and every split for 16 fixed seeds with mixed admitted versions. | PASS |
| R3-A22 | `r3_a23_live_replay_is_invariant_across_batch_partitions` checks seven batch widths over one deterministic generated history. | PASS |
| R3-A23 | `snapshot_reuses_the_atomically_committed_state_bytes`, `r3_a21_snapshot_creation_refuses_a_rejected_tail`, and `dropping_projection_releases_its_reactive_closure` cover cached bytes, rejected tails, and automatic cleanup. | PASS |
| R3-A24 | `golden_hash_and_catalog_vectors_are_stable` plus snapshot golden/mutation tests pass unchanged on Linux and Windows; WASM remains a separate compile/lint gate, and digest equality remains integrity, not authority. | PASS |

## Verification gates

Commands, tool versions, targets, test counts, durations, and resulting commit
SHAs are recorded below. Linux ran from an exact archived tree, independently
of the dirty documentation worktree.

| Gate | Required result | Recorded result |
| --- | --- | --- |
| Rust formatting | `cargo fmt --all -- --check` | PASS on Debian WSL2 and Windows native. |
| R3 focused tests | `pliego-log`, `pliego-fold`, `pliego-hyphae`, and `spike` focused default/all-feature unit, integration, and documentation tests | PASS: log 25, fold 28, Hyphae 39 default and 40 all-features, spike 5. |
| Workspace tests | `cargo test --workspace --all-targets --locked` | PASS on Linux: 331 tests in 27 suites, zero ignored. Windows reached an unrelated temporary build-script block described below. |
| Workspace lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS on Debian WSL2 and Windows native. |
| Rust documentation | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | PASS on Debian WSL2 and Windows native. |
| Linux native | Focused and workspace gates on `x86_64-unknown-linux-gnu` | PASS from exact archived tree in a fresh target directory. |
| Windows native | Focused tests and lint on `x86_64-pc-windows-msvc` | PASS: all R3 focal tests, workspace clippy, rustdoc, and formatting. |
| WASM | Focused compile/lint on `wasm32-unknown-unknown` with zero warnings; no browser-runtime claim | PASS for `pliego-log` and `pliego-fold`. |
| Documentation links | `npm run check:docs` | PASS: 46 Markdown files in the evidence worktree. |
| Distribution policy | `npm run check:distribution` | PASS: 15 source crates and 5 candidate targets. |
| Patch hygiene | `git diff --check` | PASS for the exact implementation diff and evidence worktree. |
| Independent adversarial audit | No unresolved P0/P1 finding or public bypass of catalog, transaction, or restore boundaries | PASS after two independent focused audits and root review. |

## Verification record

- Base commit: `e9971e026a5b6b32990432ea82e4979ebf76ef6e`
- Resulting implementation commit: `c341ed271f52510d5098482a297195bbb6448ec4`
- Exact implementation tree: `9f402030d0d1d5474e291d9e2161519d750ce1f6`
- Evidence commit: this document's commit.
- Debian 13 WSL2: Rust `1.85.0`, Cargo `1.85.0`, Node `20.19.2`, npm
  `11.13.0`, Linux Git `2.47.3`, kernel
  `6.18.33.1-microsoft-standard-WSL2`.
- Windows: Rust `1.85.0`, Cargo `1.85.0`, Node `24.16.0`, npm `11.13.0`,
  `x86_64-pc-windows-msvc` host.
- Linux target directory: `/tmp/pliegors-r3-c341ed2`.
- Linux focused counts: log 25/25; fold 28/28; Hyphae default 39/39;
  Hyphae all-features 40/40; spike 5/5.
- Generated cases: 16 deterministic fixed seeds, 25 snapshot split points per
  generated history, and seven batch widths for the partition-invariance case.
- Independent audit disposition: no unresolved P0 or P1; retained P2 boundaries
  are listed below.

The exact Linux run set
`PLIEGORS_SOURCE_REV=c341ed271f52510d5098482a297195bbb6448ec4`,
used `git archive` through Linux Git, and completed all gates with zero failures,
warnings, or ignored tests. Durations were 0.360 s formatting, 6.601 s log,
2.683 s fold, 1.806 s Hyphae default, 1.938 s Hyphae all-features,
1.009 s spike, 36.621 s workspace tests, 14.382 s workspace clippy,
4.931 s focused WASM clippy, 7.104 s rustdoc, 2.437 s documentation,
2.247 s distribution policy, and 0.081 s exact diff hygiene.

Windows completed the R3 focal suites, workspace clippy, rustdoc, formatting,
and focused WASM clippy. Its complete workspace run stopped only when Windows
Application Control rejected a newly generated `io-lifetimes` build-script
binary in a temporary first-use CLI fixture (`os error 4551`). The same
workspace suite passed 331/331 on the exact Linux implementation tree; no R3
test was skipped or weakened to accommodate the host policy.

## Required mutation set

The recorded test suite must mutate at least one byte or semantic value in each
required binding independently:

1. history position;
2. history head hash;
3. schema catalog digest;
4. reducer identifier;
5. reducer version;
6. reducer configuration digest;
7. codec identifier;
8. codec revision;
9. codec configuration digest;
10. encoded state bytes;
11. state digest; and
12. snapshot digest or other bound metadata.

Every failure must be observed before active projection state or cursor changes.

## Residual-risk record

The final evidence must retain, not erase, these boundaries:

- Digests detect mutation under the canonical encoding; they do not prove
  signer identity, authorization, secure key custody, or provenance.
- Cross-trust-boundary snapshot distribution requires external signatures and
  PKI that bind the complete snapshot digest and stream scope.
- Deterministic output cannot prevent a deliberately dishonest but accepted
  reducer package from computing the wrong business result.
- Event upcasters do not migrate projection-state snapshots. Contract changes
  require full replay unless a separately specified migration system exists.
- The application must bind initial state and all fold-affecting configuration
  into `ReducerIdentity`; R3 cannot detect omitted semantics automatically.
- In-memory atomicity is not durable filesystem or database transactionality.
  Production persistence must publish complete snapshot bytes atomically.
- The built-in state codec preflights 8 MiB, depth 64, and 262,144 JSON values;
  custom codecs own any stricter shape budget while remaining under the global
  encoded-state ceiling.
- Mapper, upcaster, reducer, custom-codec, and application `Serialize`,
  `Deserialize`, and `PartialEq` implementations are trusted code. Exact typed
  round trips, double execution, and bounded observations detect some asymmetry
  or divergence but cannot prove arbitrary host callbacks truthful or pure.
- Panic containment is available only on unwind-capable targets. Stable WASM
  without exception handling uses abort semantics and cannot return a typed
  panic error.
- `Log::import_raw` and projection-tail synchronization bound each item but not
  total valid event count, aggregate bytes, or processing time.
- Deterministic generated cases sample an input domain; golden vectors and
  targeted mutation tests remain necessary regression evidence.

## References

- [Event schema and snapshot contract](../30-event-schema-and-snapshot-contract.md)
- [Projection snapshot decision](../adr/ADR-005-projection-snapshots.md)
- [R2 verified sync evidence](r2-verified-sync.md)
- [Execution backlog](../19-product-execution-backlog.md)
