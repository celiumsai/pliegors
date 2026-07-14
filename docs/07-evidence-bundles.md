# PLIEGO Phase 1 evidence bundles

Phase 1 evidence is assembled locally. The bundler performs no network calls,
does not accept or move raw runs, and never derives metrics from Route Lab's
diagnostic post-load decode or Long Task observations.

## Two-phase workflow

1. Put the five reviewed candidate run JSON files in a staging directory. The
   default is `measurements/runs/staging/<case-id>/`.
2. Capture a browser trace or trace archive and a screenshot for the same five
   sessions.
3. Run the dedicated trace extractor. Its JSON must follow
   `schemas/pliego.trace-metrics.schema.json`, bind the trace SHA-256, and expose
   raw decode and main-thread task duration arrays for each staged `sessionId`.
4. Build the evidence bundle:

The extractor contract is intentionally raw and reproducible:

```json
{
  "metricsVersion": "1.0.0",
  "extractorVersion": "pliego-trace-extractor/0.1.0",
  "sourceTraceSha256": "<sha256-of-trace-or-trace-archive>",
  "runs": [
    {
      "sessionId": "<24-lowercase-hex>",
      "decodeDurationsMs": [1.2, 2.4],
      "mainThreadTaskDurationsMs": [4.8, 7.1]
    }
  ]
}
```

The real document contains exactly five run objects.

```powershell
npm run bundle:phase-1-evidence -- `
  --case-id android-site-root-universal-portrait-cold-default `
  --trace C:\evidence\trace.json.gz `
  --screenshot C:\evidence\route.png `
  --metrics C:\evidence\trace-metrics.json
```

Use `--runs <directory>` when the staged runs are elsewhere. `case-id` is an
opaque lowercase slug containing only letters, numbers, hyphens, or underscores.

The command refuses to complete unless it finds exactly five unique staged
session IDs and the metrics JSON contains the same set. Each per-run decode and
main-thread array must contain 1-20,000 finite non-negative durations. The
metrics artifact stores raw samples only: closure calculates p95 within each
run, then p95 across the five runs.

## Canonical output

The command atomically creates:

```text
measurements/evidence/<case-id>/
  bundle.json
  evidence.json
  trace.<original-extension>
  trace-metrics.json
  screenshot.<original-extension>
```

`bundle.json` records `state: "complete"`, artifact metadata, and the canonical
list of staged `{ fileName, sessionId, sha256 }` records. Existing complete
bundles are immutable; an exact rerun is idempotent and changed input is rejected.

`evidence.json` is exactly the object for `report.evidence`, including
`traceMetricsPath` and `traceMetricsSha256`. Its `rawRunsPath` points to
`measurements/runs/accepted/<case-id>/`, but the bundler does not create or
populate that directory. Publishing is a separate reviewed action: it must copy
the five staged bytes recorded in `bundle.json`, build the report, and then run
the closure gate.

Supported trace extensions are `.json`, `.json.gz`, and `.zip`. Supported
screenshot extensions are `.png`, `.jpg`, `.jpeg`, `.webp`, and `.zip`.
