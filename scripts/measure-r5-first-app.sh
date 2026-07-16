#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

samples="${1:-20}"
output="${2:-target/evidence/r5-first-replayable-app.json}"

if ! [[ "$samples" =~ ^[0-9]+$ ]] || (( samples < 1 || samples > 100 )); then
  echo "samples must be an integer between 1 and 100" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${PLIEGORS_SOURCE_REV:-}" ]]; then
  revision="$PLIEGORS_SOURCE_REV"
else
  revision="$("${PLIEGO_GIT:-git}" -C "$root" rev-parse --verify HEAD)"
fi
if ! [[ "$revision" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "PLIEGORS_SOURCE_REV must resolve to a full Git commit SHA" >&2
  exit 2
fi
target="${PLIEGO_R5_CLI_TARGET:-/tmp/pliegors-r5-first-app-cli}"
work="$(mktemp -d "${TMPDIR:-/tmp}/pliegors-r5-first-app.XXXXXX")"
trap 'rm -rf "$work"' EXIT

CARGO_TARGET_DIR="$target" \
  cargo build --manifest-path "$root/Cargo.toml" -p pliego-cli --release --locked
source_cli="$target/release/pliego"
if [[ ! -x "$source_cli" ]]; then
  echo "release CLI was not produced at $source_cli" >&2
  exit 1
fi

durations=()
for ((sample = 1; sample <= samples; sample += 1)); do
  sample_root="$work/sample-$sample"
  install_root="$sample_root/install/bin"
  project="$sample_root/application"
  mkdir -p "$install_root"

  start_ns="$(date +%s%N)"
  install -m 0755 "$source_cli" "$install_root/pliego"
  cli="$install_root/pliego"
  "$cli" new "$project" --framework-path "$root" >/dev/null
  (cd "$project" && "$cli" check >/dev/null)
  (cd "$project" && cargo test --locked --quiet)
  (cd "$project" && "$cli" build >/dev/null)
  (cd "$project" && "$cli" inspect >/dev/null)
  (cd "$project" && "$cli" why artifact / >/dev/null)
  end_ns="$(date +%s%N)"

  elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))
  durations+=("$elapsed_ms")
  printf 'sample %02d/%02d: %d ms\n' "$sample" "$samples" "$elapsed_ms"
done

mapfile -t sorted < <(printf '%s\n' "${durations[@]}" | sort -n)
p50_index=$(( (samples * 50 + 99) / 100 - 1 ))
p95_index=$(( (samples * 95 + 99) / 100 - 1 ))
p50="${sorted[$p50_index]}"
p95="${sorted[$p95_index]}"

output_path="$output"
if [[ "$output_path" != /* ]]; then
  output_path="$root/$output_path"
fi
mkdir -p "$(dirname "$output_path")"
sample_json="$(printf '%s,' "${durations[@]}")"
sample_json="[${sample_json%,}]"

cat >"$output_path" <<JSON
{
  "contract": "pliegors-r5-install-to-first-replayable-app/1",
  "revision": "$revision",
  "platform": "$(uname -sm)",
  "rustc": "$(rustc --version)",
  "samples": $samples,
  "durationsMs": $sample_json,
  "p50Ms": $p50,
  "p95Ms": $p95,
  "nearestRank": true,
  "measuredWork": [
    "install release CLI copy",
    "pliego new default",
    "pliego check",
    "cargo test --locked",
    "pliego build",
    "pliego inspect",
    "pliego why artifact /"
  ]
}
JSON

printf 'p50: %d ms\np95: %d ms\nevidence: %s\n' "$p50" "$p95" "$output_path"
