# PliegoRS Next Phase 0 smoke

**Status:** bounded smoke; not accepted baseline evidence
**Captured:** 2026-08-09
**Source reference:** `bdc285fbe208f09c47ffdfbf1081e923884a9cf7` plus a dirty Next worktree
**Environment:** DigitalOcean `nyc3`, 4 vCPU DO Premium AMD, 8 GiB RAM, NVMe,
Ubuntu 24.04, Rust 1.86.0, Node 20.20.2, Chrome 151.0.7922.108

This record preserves the first complete build/dev/HMR phase split for the
executable `minimal` and `stress-dashboard` fixtures. The raw JSON and rendered
Markdown remain outside the repository because the source tree was dirty:

```text
baseline.json sha256 c915b68cf589422428772957d8278c4cef4123197d2316dc11da8cbfdf4e83bf
baseline.md   sha256 8527852682024d61156804e532d05389e01ef33cb04b12dd179d1a7a90bcb64c
```

## Summary

| p50 metric | Minimal | Stress dashboard |
| --- | ---: | ---: |
| Cold build | 49,464.66 ms | 48,879.96 ms |
| Warm verified build | 425.37 ms | 448.01 ms |
| Cold Rust/WASM phase | 35,133.47 ms | 34,837.74 ms |
| Cold site phase | 13,728.87 ms | 13,466.37 ms |
| CSS change to SSE | 2,712.29 ms | 2,562.46 ms |
| CSS change to visible browser | 2,722.08 ms | 2,664.84 ms |
| HMR discovery | 372.79 ms | 341.25 ms |
| HMR Rust/WASM | 535.73 ms | 486.42 ms |
| HMR site | 1,370.04 ms | 1,440.84 ms |
| HMR verification | 76.31 ms | 71.84 ms |
| HMR host/transport remainder | 213.51 ms | 239.30 ms |
| DOM mutation records | 1 | 1 |
| Dev-session RSS | 57,552,896 bytes | 61,960,192 bytes |

## Findings

1. Site execution is the largest CSS-HMR phase for both fixtures.
2. Rebuilding Rust/WASM for a CSS-only source change is the second largest
   measured phase and demonstrates the absence of a CSS fast path.
3. Discovery is material but smaller. Output verification is not a primary
   bottleneck.
4. The residual host/transport interval is consistent with watcher debounce,
   SSE polling, scheduling, and delivery. This report does not attribute that
   remainder more narrowly.
5. Browser settlement adds about 10 ms for the minimal fixture and about 102 ms
   for the 1,536-row dashboard at p50. The browser is not the dominant HMR cost.
6. Both CSS paths mutate one DOM record: the stylesheet link URL.

## Limits

- Dirty source means these observations cannot close Phase 0.
- Five samples make nearest-rank p95 and p99 equal to the maximum observation.
- The VM is suitable for reproducible framework comparison, not Hyphae G7's
  dedicated non-virtualized hardware requirement.
- Cache lookup attempts/hits/misses, internal owner/resource census, and typed
  remount/reload receipts remain unavailable.
- At capture time, Hyphae Console was deferred behind Native G7, G8, a public
  release, and the PliegoRS Native integration decision. Hyphae `v1.0.1` later
  satisfied the release prerequisite; ADR-011 now permits sidecar-adapter work
  while G7/G8 continue to block performance claims and final fixture acceptance.
