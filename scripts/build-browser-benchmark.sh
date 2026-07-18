#!/usr/bin/env sh
# SPDX-License-Identifier: Apache-2.0
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
manifest="$root/benchmarks/browser-apply/Cargo.toml"
output="$root/target/benchmarks/browser-apply"
target_dir=${PLIEGORS_BROWSER_BENCH_TARGET:-"$root/target/benchmarks/browser-cargo"}
toolchain=${PLIEGORS_BENCH_TOOLCHAIN:-1.86.0}

[ "$#" -eq 0 ] || {
  echo "usage: sh scripts/build-browser-benchmark.sh" >&2
  exit 2
}

runner=$(command -v wasm-bindgen || true)
[ -n "$runner" ] || {
  echo "wasm-bindgen 0.2.126 is required" >&2
  exit 1
}
"$runner" --version | grep -Eq 'wasm-bindgen 0\.2\.126$' || {
  echo "wasm-bindgen 0.2.126 is required" >&2
  exit 1
}

CARGO_TARGET_DIR="$target_dir" cargo "+$toolchain" build \
  --manifest-path "$manifest" \
  --target wasm32-unknown-unknown \
  --release \
  --locked

# The output is a fixed path inside this checkout; it is never supplied by a caller.
rm -rf "$output"
mkdir -p "$output"
[ -f "$target_dir/wasm32-unknown-unknown/release/pliegors_browser_apply_benchmark.wasm" ] || {
  echo "compiled browser benchmark WASM is missing" >&2
  exit 1
}
"$runner" \
  --target web \
  --out-dir "$output" \
  --out-name pliegors_browser_benchmark \
  "$target_dir/wasm32-unknown-unknown/release/pliegors_browser_apply_benchmark.wasm"

printf 'browser benchmark package: %s\n' "$output"
