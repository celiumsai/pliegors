# Reproducible benchmarks

PliegoRS P8 measures its own build and browser runtime paths with versioned,
machine-readable reports. The measurements are diagnostic evidence for one
revision and environment. They are not performance guarantees, budgets, or
competitor comparisons.

## Covered work

The build harness copies the maintained `content-collections-pliegors` example
into a fresh standalone workspace for every sample. It resolves dependencies
before timing and then executes the same `pliego build` command five times in a
fixed order:

| Observation | Project state |
| --- | --- |
| clean cold | fresh project and empty Cargo target; host registry/Git caches retained |
| no-change warm | same source, output, and target after the cold build |
| content-only | one real Markdown entry changed |
| CSS-only | the stylesheet embedded by the site package changed |
| Rust view | a Rust component source file changed |

The browser harness compiles a dedicated Rust `cdylib` against the local
`pliego-dom` and `pliego-reactive` crates. In a fresh headless Chrome profile it
mounts a dynamic PliegoRS view, warms the signal, times synchronous updates,
and verifies the final DOM text. It then records WebAssembly linear-memory size
and DOM child residue across repeated `mount`/`dispose` batches.

A memory plateau means only that the last three page-granular WASM memory
values are equal and every observed host has zero residual children. It does
not prove that every heap allocation was reclaimed.

## Prerequisites

- Rust `1.85.0` with `wasm32-unknown-unknown`;
- `wasm-bindgen-cli 0.2.126`;
- Node.js 20 or newer and the repository dependencies;
- Chrome, or `CHROME_BIN` pointing to Chrome/Chromium;
- a clean Git worktree for evidence runs.

Declare the storage class when known. Power and thermal state remain explicitly
uncontrolled unless a future collector can measure them:

```sh
export PLIEGORS_BENCH_STORAGE_CLASS="NVMe SSD"
```

## Capture

On Linux, macOS, or WSL2, capture at least five complete build samples. Ten is
the maintained default:

```sh
node scripts/measure-p8-builds.mjs
```

Each completed build sample is checkpointed with the revision, requested sample
count, and exact environment record. After an external interruption, resume
only that matching experiment:

```sh
PLIEGORS_BENCH_RESUME=1 node scripts/measure-p8-builds.mjs
```

The command refuses a checkpoint from another commit, sample count, or
environment. A successful final report removes the checkpoint.

Build the browser fixture on a Unix-like host:

```sh
sh scripts/build-browser-benchmark.sh
```

Then run at least ten browser samples. The default is 20 samples, 1,000 signal
updates per sample, 250 warmup updates, and six 500-cycle lifecycle batches:

```sh
node scripts/measure-browser-benchmark.mjs
```

The browser and build reports must name the same clean commit. Merge them and
validate the final schema:

```sh
node scripts/merge-p8-benchmark-report.mjs
npm run check:benchmarks
```

The default outputs are:

- `target/benchmarks/p8-build.json`;
- `target/benchmarks/p8-browser.json`;
- `target/benchmarks/p8-report.json`.

Generated evidence is not committed by default. A release candidate may copy
the final report into its sealed evidence bundle after independent review.

## Statistical contract

Every report retains raw observations. p50 and p95 use the nearest-rank method:
sort ascending and select rank `ceil(p * n)`, starting at rank one. No outlier
is discarded. CLI compilation, dependency resolution, WASM packaging, and
Chrome startup occur outside their respective timed regions and are declared
in the report.

`PLIEGORS_ALLOW_DIRTY_BENCH=1` exists only for harness smoke tests. Such reports
carry `sourceTreeDirty: true` and the merger rejects them, so they cannot become
P8 release evidence.

## Interpretation

Read raw observations before percentiles. Different operating systems may be
used for the build and browser sections, and each section records its own CPU,
memory, storage declaration, runtime versions, cache policy, and limitations.
Results from another commit, machine, browser, power state, or cache policy are
a different experiment and must not be combined with this report.

## Published local baseline

The first complete [P8 baseline](../benchmarks/baselines/p8-888b892.json) is
bound to clean revision `888b8929951c724b3b5146073897918779c539d1`. Build
samples ran in Debian 13 on WSL2 with Rust 1.85.0, Node 20.19.2, an Intel Core
Ultra 9 285H, 16 logical CPUs, 16 GB exposed memory, and an NVMe SSD through the
WSL2 filesystem. Browser samples ran on Windows x64 with Chrome 150.0.7871.128,
Node 24.16.0, the same CPU, 32 GB host memory, and an NVMe SSD.

| Observation | p50 | p95 |
| --- | ---: | ---: |
| clean cold build | 11,455.060 ms | 14,946.898 ms |
| no-change warm build | 1,715.485 ms | 1,979.736 ms |
| content-only build | 1,690.088 ms | 2,473.570 ms |
| CSS-only build | 2,185.710 ms | 2,727.206 ms |
| Rust-view build | 2,173.469 ms | 2,808.250 ms |
| browser apply per update | 1.300 us | 2.300 us |

Across the recorded 500 through 3,500 lifecycle cycles, WASM linear memory
remained at 1,179,648 bytes and every observation had zero residual DOM child
nodes. Power and thermal state were not controlled or measured. These values
are a local baseline for regression detection, not a release-candidate matrix
or a claim about other machines.
