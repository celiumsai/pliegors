# `pliego inspect` command contract

**Status:** Phase 1 executable contract
**Current binary:** `pliego-inspect`
**Future CLI surface:** `pliego inspect`

## Responsibilities

The inspector is a deterministic gate, not an optimizer. It reads a PLIEGO asset
manifest, verifies cross-field invariants, optionally verifies physical files,
calculates adaptive-tier totals, and evaluates route-level asset budgets.

It does not infer legal rights, fabricate provenance, transcode media, launch a
browser, or treat an estimated GPU number as a measured result.

## Commands

### Inspect

```text
pliego-inspect inspect <manifest> [--asset-root <dir>]
                       [--format human|json] [--output <file>]
                       [--enforce-budgets]
```

Without `--asset-root`, the command validates the deterministic manifest
snapshot. With it, the command also:

- hashes every declared variant;
- compares byte lengths;
- scans every configured tracked extension;
- rejects undeclared or missing tracked files.

Budget debt is reported but does not change the exit code unless
`--enforce-budgets` is present. This allows Phase 1 to capture real debt while
Phase 2 turns the same budgets into a release gate.

### Baseline

```text
pliego-inspect baseline <targets.json> [--format human|json]
                        [--output <file>] [--enforce-budgets]
```

The target set references committed manifest snapshots with relative paths.
The JSON report contains no timestamp, absolute path, host name, or random data,
so identical inputs produce byte-identical output locally and in CI.
Both `measurementPlan` and every target manifest must be normalized relative
paths. Their canonical files must remain inside the target-set directory;
absolute paths, traversal, links that escape the directory, and ambiguous
platform separators fail before hashing or inspection.

## Exit codes

| Code | Meaning |
| ---: | --- |
| `0` | Contract valid; enforced budgets, if requested, pass |
| `1` | Invariant, integrity, coverage, or enforced budget failure |
| `2` | Invalid command, unreadable input, or malformed JSON |

## Human report

The human report is optimized for a release review. It shows identity,
integrity mode, counts, total bytes, per-tier totals, every budget result,
invariant issues, and the final validity/budget state.

## JSON report

The JSON report is the CI contract. Ordered Rust structs and `BTreeMap` values
make field and tier ordering stable. Budget results remain in manifest order so
the authored route priority is preserved.

`valid` and `budgetsPass` are intentionally separate. A valid manifest can
expose an over-budget current fixture; an invalid manifest can never be released
even when its arithmetic happens to fit a budget.

## Current Phase 1 facts

The file-integrity pass on 2026-07-11 found:

| Target | Assets | Variants | Declared bytes | Budget state |
| --- | ---: | ---: | ---: | --- |
| PliegoRS site | Current manifest | Current manifest | Current manifest | Revalidated per build |

These are source-manifest totals, not network measurements. The physical-device
report remains the authority for decode, main-thread, frame-time, and Core Web
Vitals results.
