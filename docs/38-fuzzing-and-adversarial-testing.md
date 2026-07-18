# Fuzzing and adversarial testing

PliegoRS maintains six `cargo-fuzz` targets for inputs that cross a trust
boundary. The harness uses `cargo-fuzz 0.13.2`, `libfuzzer-sys 0.4.13`, and the
pinned `nightly-2026-06-26` toolchain on x64 or ARM64 Unix-like hosts. Native
Windows is covered by deterministic tests; run libFuzzer through WSL2.

| Target | Product boundary | Principal invariant |
| --- | --- | --- |
| `portable_path` | routes and artifact paths | every admitted path round-trips without traversal or platform ambiguity |
| `project_manifest` | `pliego.toml` | the fuzzer calls the same parser and semantic validator as the CLI |
| `event_schema` | untrusted event JSON | accepted JSON is duplicate-free, bounded, canonical, and idempotent |
| `snapshot` | projection snapshot wire bytes | valid envelopes round-trip and every one-byte mutation is rejected |
| `dom_adoption` | adoptable SSR construction | parser-sensitive trees fail closed and successful output stays bounded |
| `adapter_manifest` | declarative plugin admission | only safe modules, identifiers, props, capabilities, and motion policy render |

## Deterministic smoke

Install `cargo-fuzz 0.13.2`, then run on Linux, macOS, or WSL2:

```sh
cargo install cargo-fuzz --version 0.13.2 --locked
sh scripts/run-fuzz.sh
```

The runner copies the reviewed corpus to a temporary directory, fixes the seed,
caps each input at 64 KiB, applies a five-second per-input timeout, and executes
512 cases per target. CI runs the same command on every relevant pull request,
main update, manual dispatch, and weekly schedule.

## Maintainer run

Increase the deterministic run budget without changing the harness:

```sh
FUZZ_RUNS=1000000 FUZZ_SEED=20260718 sh scripts/run-fuzz.sh
```

One target can be isolated by name:

```sh
FUZZ_RUNS=100000 sh scripts/run-fuzz.sh snapshot
```

Crashes are written under `fuzz/artifacts/<target>/`. Reproduce and minimize a
failure before review:

```sh
cargo +nightly-2026-06-26 fuzz run snapshot fuzz/artifacts/snapshot/<case>
cargo +nightly-2026-06-26 fuzz tmin snapshot \
  fuzz/artifacts/snapshot/<case> fuzz/corpus/snapshot/regression-<issue>
```

Only a minimized, named regression enters the committed corpus. Generated
coverage corpus entries and local artifacts remain ignored. A bounded smoke is
evidence that the checked executions did not fail; it is not proof that the
input space is exhausted.
