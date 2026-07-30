# Golden developer experience contract

**Status:** R5 accepted; G4 engineering-readiness extensions in progress

R5 makes the shortest PliegoRS path a replayable application, then keeps every
development rebuild causal and explainable. It does not turn PliegoRS into a
generic JavaScript bundler.

## First replayable application

`pliego new <path>` selects default starter revision 3. Its `src/domain.rs`
contains one complete vertical:

```text
Action::CreateNote
  -> NoteCreated v1 / EventSchema
  -> ReactiveLog::append_typed
  -> sealed EventCatalog
  -> identified transactional Reducer
  -> Projection<NotesProjection, AppEvent>
  -> rendered Rust route
```

The generated project owns three tests: live projection equals genesis replay,
a verified snapshot restores and folds exactly its tail, and a rejected action
does not append history. `pliego check` materializes the lockfile before `cargo
test --locked`.

## Causal build graph

Every SSG build reserves and emits `pliego.graph.json` version 1. The file is a
normal output in `pliego.build.json`; missing, extra, modified, linked, or
oversized graph bytes fail normal artifact verification.

`Page::source(path)` and `Asset::source(path)` accept only canonical captured
project sources. Pages create source-to-route and route-to-artifact edges.
Assets create source-to-artifact edges. Producers without declarations use the
explicit `allSources` compatibility edge. The graph is sorted, bounded to the
artifact limits, and bound to the receipt's project ID, source-set digest,
source hashes, output paths, producers, kinds, and hashes.

The graph is now also a verified reuse contract. When the current build context
and the prior receipt match exactly, `pliego build` returns a no-op without
starting the client or site compiler. When inputs changed, `Page::lazy` renders
only routes whose declared sources changed and copies every other artifact from
the verified prior publication into a fresh stage. The complete new site is
still committed atomically.

`Page::new` remains eager for source compatibility and is always rendered.
`Page::lazy` is an explicit correctness promise: every byte consulted by its
renderer must appear in `.source(...)`. A producer with no declarations uses
`allSources` and is conservatively invalidated by any captured source change.

## Native watcher and HMR

`pliego dev` uses `notify` 8.2 with symlink following disabled. Its recommended
backend maps to inotify, FSEvents/kqueue, or ReadDirectoryChangesW by platform.
The CLI debounces event batches for 120 ms, ignores generated roots, and never
rescans the entire project merely to discover a write. The verified build
context still hashes every accepted source byte.

After a successful rebuild, the CLI compares prior and current graphs and emits
one SSE payload:

```json
{
  "generation": 4,
  "kind": "css",
  "paths": ["/assets/site.css"],
  "routes": []
}
```

| Kind | Browser action |
| --- | --- |
| `css` | Cache-bust matching stylesheet links without replacing the document. |
| `content` | Fetch the current route, dispatch scope disposal, replace its body, and reconnect. |
| `adapter` | Dispatch a cancelable adapter HMR event; runtime v1.2 remounts from a cache-busted ESM specifier. |
| `reload` | Reload for mixed output classes or an unhandled adapter event. |
| `none` | Preserve the document and advance the generation. |

Build failure advances the error surface without terminating the watcher. The
last valid output remains on disk. Development responses are `no-store`; preview
and production output contain no HMR script.

## Why commands

```powershell
pliego why artifact /
pliego why artifact assets/site.css
pliego why-rebuilt
pliego cache status
pliego cache status --format json
pliego cache clean
```

`why artifact` first verifies the current receipt and graph, then prints the
source, optional route, producer, and artifact hash. `why-rebuilt` reads
`target/.pliego/last-rebuild.json`, a bounded strict record written after a
successful development rebuild. It reports changed sources, causal invalidation,
actual byte changes, HMR class, and before/after receipts. It is not a durable
build history and is intentionally excluded from publication inputs.

`cache status` verifies the current receipt and graph before reporting the last
production build outcome, causal source changes, rendered/reused artifact
counts, and bounded phase timings. The strict private record lives at
`target/.pliego/last-build.json`; `cache clean` removes only that record and the
development rebuild record. It never removes Cargo outputs or a published site.

## Diagnostics

All failure categories retain stable codes and exit statuses. Human output adds
`at:` lines and `fix:` suggestions when available. JSON output always includes:

```json
{
  "spans": [{ "file": "src/main.rs", "line": 12, "column": 7, "label": "compiler primary" }],
  "fixes": [{ "message": "...", "applicability": "manual" }]
}
```

Compiler locations are parsed from rendered Rust primary spans, including
Windows drive-letter paths. Manifest parse locations use TOML line and column.
The CLI retains at most 16 spans and fixes, sanitizes controls, and bounds each
suggestion to 512 bytes.

## Measurement

`scripts/measure-r5-first-app.sh` builds the release CLI once outside the timed
region. Each independent sample begins by installing a copy of that binary and
ends only after default scaffolding, `pliego check`, all replay tests, `pliego
build`, `pliego inspect`, and `pliego why artifact /` succeed. The report uses
nearest-rank p50/p95 and records its exact revision, platform, Rust version,
sample vector, and measured steps.

The accepted measurements and command replay are in
[`evidence/r5-golden-developer-experience.md`](evidence/r5-golden-developer-experience.md).
