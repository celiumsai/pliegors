#!/usr/bin/env sh
# SPDX-License-Identifier: Apache-2.0
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
toolchain=${PLIEGORS_FUZZ_TOOLCHAIN:-nightly-2026-06-26}
runs=${FUZZ_RUNS:-512}
seed=${FUZZ_SEED:-20260718}
max_len=${FUZZ_MAX_LEN:-65536}

for value in "$runs" "$seed" "$max_len"; do
  case "$value" in
    ''|*[!0-9]*) echo "fuzz limits must be positive integers" >&2; exit 2 ;;
  esac
  [ "$value" -gt 0 ] || {
    echo "fuzz limits must be positive integers" >&2
    exit 2
  }
done

if [ "$#" -eq 0 ]; then
  set -- portable_path project_manifest event_schema snapshot dom_adoption adapter_manifest
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT INT TERM
for target in "$@"; do
  case "$target" in
    portable_path) seeds="valid.txt traversal.txt" ;;
    project_manifest) seeds="valid.toml overlap.toml" ;;
    event_schema) seeds="valid.json duplicate.json" ;;
    snapshot) seeds="raw.txt" ;;
    dom_adoption) seeds="parser-sensitive.txt" ;;
    adapter_manifest) seeds="valid-shape.txt" ;;
    *) echo "unknown PliegoRS fuzz target: $target" >&2; exit 2 ;;
  esac
  corpus="$scratch/$target"
  mkdir -p "$corpus"
  for seed_file in $seeds; do
    cp "$root/fuzz/corpus/$target/$seed_file" "$corpus/$seed_file"
  done
  (
    cd "$root"
    cargo "+$toolchain" fuzz run "$target" "$corpus" -- \
      -runs="$runs" \
      -seed="$seed" \
      -max_len="$max_len" \
      -timeout=5
  )
done
