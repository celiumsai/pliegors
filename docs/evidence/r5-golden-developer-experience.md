# R5 golden developer experience acceptance evidence

**Gate:** R5 - Golden developer experience

**Status:** Accepted

**Recorded:** 2026-07-16

**Implementation revision:** `d91a3b95216c48eb8fc7af773c1d8d3d38325bca`

**Platform boundary:** Debian WSL2 for native Rust execution; Windows 11 for
documentation and Node.js adapter verification

## Acceptance matrix

| ID | Requirement | Evidence | Result |
| --- | --- | --- | --- |
| R5-A01 | Scaffold actions, events, projections, and replay tests | Default starter revision 2 and the standalone first-use integration gate | PASS |
| R5-A02 | Watch through operating-system events | `notify` recommended watcher, ignored generated roots, symlinks disabled, and native watcher E2E | PASS |
| R5-A03 | Emit source-to-route-to-artifact causality | Versioned receipt-bound `pliego.graph.json`, precise and conservative edge tests | PASS |
| R5-A04 | Explain artifacts and rebuilds | Verified `pliego why artifact` and bounded `pliego why-rebuilt` records | PASS |
| R5-A05 | Hot-update CSS, content, and adapters | Real CSS/content dev-server E2E plus 22 adapter runtime cases | PASS |
| R5-A06 | Report spans and fix suggestions | Human and strict JSON diagnostic contract with bounded compiler/TOML extraction | PASS |
| R5-A07 | Measure install-to-first-replayable-app p50/p95 | 20 complete independent samples on the exact implementation revision | PASS |
| R5-A08 | Preserve artifact trust and framework regressions | Complete workspace tests, Clippy, Rustdoc, docs, and distribution policy | PASS |

## Implementation chain

- `84f9cef` - emit a versioned receipt-bound causal build graph;
- `c14dc94` - replace polling with native events and typed causal HMR;
- `37ff663` - make the default starter a complete replayable vertical;
- `fa9ee99` - expose bounded diagnostic spans and manual fixes;
- `97bf420` - prove native CSS HMR against a generated application;
- `6ae7026` - add the repeatable first-replayable-app measurement;
- `d91a3b9` - prove content and adapter HMR classification and content E2E.

The R5 base is R4 evidence commit `d8b5ee6`. This document records the final
implementation revision above; its own documentation commit is intentionally
not part of the measured binary.

## Verified behavior

### Replayable first use

`pliego new` defaults to starter revision 2. The generated `src/domain.rs`
contains a typed action, versioned event schema, sealed catalog, identified
reducer, transactional projection, and rendered state. Its three owned tests
prove live/genesis replay equality, verified snapshot-tail restore, and no log
append after action rejection. The CLI integration gate scaffolds outside the
workspace and runs `pliego check`, `cargo test --locked`, `pliego build`, graph
verification, and `pliego why artifact /`.

### Causal graph and explanations

Every SSG publication reserves `pliego.graph.json` and includes its exact bytes
in `pliego.build.json`. Graph validation is strict, bounded, deterministic, and
binds project, source set, output namespace, producer, kind, route, and hashes.
Declared page sources produce source-to-route-to-artifact chains; declared
asset sources produce source-to-artifact chains. A legacy producer without
declarations receives an explicit `allSources` edge rather than false precision.

`pliego why artifact` verifies the current receipt and graph before explaining
an artifact or route. `pliego why-rebuilt` reads only the latest bounded private
development record and reports the changed sources, invalidated routes, affected
and byte-changing artifacts, HMR decision, and receipt transition.

### Native watcher and typed HMR

`pliego dev` uses the platform-recommended `notify` watcher, a 120 ms debounce,
and generated-root exclusion. Failed rebuilds preserve the last valid output
and keep watching. Successful builds compare verified graphs and emit typed SSE.

The end-to-end generated-project test observed a stylesheet edit as `css`,
updated the matching URL without requiring a document replacement protocol,
and explained `assets/site.css`. It then edited Rust page content, observed
`content` for route `/`, fetched the newly compiled document, and explained the
content rebuild. Adapter runtime 1.2 passed 22 Node.js cases, including abort,
cleanup, serialized updates, policy changes, and cache-busted HMR remount.

### Diagnostics

Human diagnostics retain stable codes and add bounded `at:` and `fix:` lines.
JSON diagnostics always expose `spans[]` and `fixes[]`. Compiler primary spans,
Windows drive-letter paths, and TOML line/column locations are parsed without
turning suggestions into automatic edits. At most 16 spans and fixes are kept;
fix text is control-sanitized and bounded to 512 bytes.

## First replayable application measurement

The executable measurement built the release CLI once outside the timed region.
Each sample then copied that binary into a fresh install root and performed:

```text
install release CLI copy
pliego new default
pliego check
cargo test --locked
pliego build
pliego inspect
pliego why artifact /
```

All 20 samples succeeded. Durations in execution order, milliseconds:

```text
14471, 16541, 15368, 15907, 15138, 14806, 14369, 14987, 15750, 16899,
16078, 16833, 16149, 15958, 15552, 15776, 15019, 15525, 14919, 16071
```

| Statistic | Result |
| --- | ---: |
| Samples | 20 |
| p50, nearest rank | 15,552 ms |
| p95, nearest rank | 16,833 ms |

With 20 observations, nearest-rank p95 is the 19th sorted observation. The
machine-readable report was generated at
`target/evidence/r5-first-replayable-app.json`; `target` remains intentionally
untracked, while the complete vector and environment are committed here.

## Verification replay

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| Debian `cargo test --workspace --locked` | PASS, including 46 CLI unit, 14 CLI contract, and native HMR E2E cases |
| Debian `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| Debian `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | PASS, 20 documentation roots |
| `npm run test:adapters` | PASS, 22/22 |
| `npm run check:distribution` | PASS, 15 source-only crates and five private candidate targets |
| `npm run check:docs` | PASS after this evidence file was added |
| `git diff --check` | PASS |
| `scripts/measure-r5-first-app.sh 20` | PASS, 20/20 complete samples |

## Environment and interpretation

- Debian WSL2, `Linux x86_64`;
- Rust `1.85.0` (`4d91de4e4 2025-02-17`);
- Windows Node.js `24.16.0`, npm `11.13.0`;
- exact source revision `d91a3b95216c48eb8fc7af773c1d8d3d38325bca`.

Native Windows test executables are blocked by local Windows Application
Control with OS error 4551. Native certification therefore uses Linux binaries
and a Linux target directory in Debian WSL2. Node.js adapter tests and docs run
on Windows. No Rust test was skipped or weakened for that host policy.

The timing is checkout-specific evidence with dependency caches available. It
is not a universal performance promise. Every sample used a new project and
target, while the release CLI build was excluded as documented. R6 must measure
the golden path from candidate distribution artifacts rather than a framework
checkout.

## Residual boundaries

- The causal graph provides precise invalidation and explanation. Cargo still
  performs normal incremental compilation and the SSG atomically stages the
  complete valid site; R5 does not claim a partial route compiler.
- Content HMR replaces the authored body after synchronous scope disposal. It
  is not state-preserving component HMR.
- Adapter HMR can clean registered resources immediately, but a plugin must
  observe its abort signal to stop arbitrary unregistered asynchronous work.
- `why-rebuilt` is the latest private development explanation, not durable
  provenance or a build-history database.
- Candidate binary authenticity, five-target reproducibility, installer
  lifecycle, and distribution-only onboarding remain R6.

No unresolved R5 P0 or P1 finding remains within these boundaries. No deploy,
release, or repository publication was performed.
