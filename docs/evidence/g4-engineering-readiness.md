# G4 incremental engineering evidence

**Gate:** G4 engineering prerequisite
**Status:** Accepted on the named revision; external adoption remains open
**Date:** 2026-07-30
**Contract:** `pliegors-g4-incremental-measurement/1`
**Revision:** `5e655a06f70b825fd69e6bbf74c85c825e6cb38a`

## Acceptance result

The maintained harness completed five independent official-default projects on
Linux. The release CLI was built once from the clean named revision outside
the timed region. Every cold sample used an empty Cargo target. Every sample
then proved:

- a cold executed build;
- a verified warm no-op with the publication receipt unchanged;
- selective execution after changing only `src/domain.rs`;
- selective execution after changing only `assets/site.css`;
- fail-closed rejection of a malformed private build record;
- verified no-op recovery after removing only the malformed record; and
- private cache cleanup without deleting the publication or Cargo outputs.

The fixture contained three routes and seven receipt-bound publication
artifacts. Content changes rendered five and reused two artifacts in every
sample. Asset changes rendered four and reused three.

## Observed results

Durations are wall-clock milliseconds. Peak RSS is the nearest-rank percentile
of 10 ms `/proc` samples summed across the command process tree.

| Scenario | Duration p50 | Duration p95 | Peak RSS p50 | Peak RSS p95 |
| --- | ---: | ---: | ---: | ---: |
| Cold, empty Cargo target | 7,481.08 ms | 8,986.76 ms | 59,516 KiB | 73,200 KiB |
| Verified no-op | 201.83 ms | 239.11 ms | 50,636 KiB | 52,476 KiB |
| `src/domain.rs` change | 662.40 ms | 796.81 ms | 52,212 KiB | 52,764 KiB |
| `assets/site.css` change | 696.50 ms | 820.09 ms | 52,336 KiB | 52,656 KiB |
| Post-rejection recovery | 184.53 ms | 248.28 ms | 52,088 KiB | 52,552 KiB |

The complete raw vectors, per-phase microseconds, changed source sets, artifact
counts, receipt transitions, host details, and release CLI digest are retained
in
[`g4-engineering-readiness.json`](g4-engineering-readiness.json).

## Bound environment

- Linux `6.18.33.1-microsoft-standard-WSL2`, x86-64
- Intel Core Ultra 9 285H, 16 logical CPUs
- 16,502,128,640 bytes of reported host memory
- Rust `1.86.0`
- Cargo `1.86.0`
- Node.js `24.16.0`
- release CLI SHA-256
  `707d3e48db6828f3374c0f846fb10ee6048efe56e44d57aa419f9c8363ef59af`

The source commit was copied into a clean ext4 clone before measurement because
Git for Windows and Git in WSL applied different line-ending policies to the
NTFS working tree. The report names and verifies the same exact source
revision; measurements and Cargo targets remained on ext4.

## Maintained commands

```sh
npm run check:g4-engineering
npm run measure:g4-engineering -- --samples 5 --accept \
  --output docs/evidence/g4-engineering-readiness.json
```

## Limits

This evidence applies only to the named revision, host, toolchain, and official
default fixture. It is not a competitor comparison, Linux ARM64 result,
low-end-device guarantee, hosted-CI result, distributed-cache claim, or
external adoption proof. G4 Adoption closes only after an unaffiliated team
completes the public-resource-only greenfield and partial-migration trials.
