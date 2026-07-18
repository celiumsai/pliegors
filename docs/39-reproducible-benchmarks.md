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
