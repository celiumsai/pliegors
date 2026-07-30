# G4 engineering readiness

**Status:** In progress
**Scope:** Internal prerequisite for G4 Adoption

The public execution backlog defines G4 as an external adoption gate. PliegoRS
cannot close that gate with repository-owned tests. This contract names the
engineering work that must land before an unrelated team is asked to complete
the greenfield and migration trials.

## E1 - Verified no-op builds

A repeated `pliego build` may skip client compilation and site execution only
when all of the following remain true:

- the complete current `BuildContext` is byte-for-byte equivalent to the
  context in the published receipt;
- the output directory and every receipt-bound output pass exact-set,
  no-follow, size, and SHA-256 verification;
- the project ID, site package, framework revision, toolchains,
  configuration, source set, and external materials are unchanged; and
- inputs are revalidated after the cache decision.

A timestamp-only filesystem event is therefore a hit. A changed byte, missing
output, extra output, link, ownership mismatch, malformed ledger, or changed
toolchain is not.

## E2 - Causal artifact reuse

When a verified prior build exists, the SSG may reuse an artifact instead of
executing its producer only when:

- the prior receipt and causal graph verify together;
- the artifact path, kind, producer, route, and source dependency declaration
  are unchanged;
- framework, toolchain, configuration, material, ownership, and exclusion
  evidence remain compatible;
- every explicitly declared source retains its captured SHA-256; and
- the prior artifact is reopened without following links and its bytes match
  the receipt before reuse.

`allSources` remains deliberately conservative. A producer that omits causal
edges does not receive selective reuse after any source change. Source
declarations are a correctness contract, not a performance hint.

## E3 - Observable execution

The CLI must expose bounded machine-readable evidence for the latest build:

- outcome: `executed` or `no-op`, with rendered/reused counts distinguishing
  cold and selective execution;
- elapsed time by owned phase;
- rendered and reused artifact counts;
- before and after receipt identity; and
- the changed source set used for invalidation.

`pliego cache status` verifies that record and its referenced output.
`pliego cache clean` removes only project-owned private cache metadata. It
never removes the published site, Cargo target directory, or global caches.

## E4 - Lazy producer API

Eager `Page::new` remains compatible. A lazy page API must declare its causal
sources before accepting a renderer. On a selective hit, the renderer is not
called. Tests must prove execution counts rather than infer reuse from equal
bytes or elapsed time.

## E5 - Asset work queue

Adaptive asset plans must support a bounded status operation that classifies
each job as `pending`, `ready`, or `invalid` from the exact pinned plan and
staging artifact. Valid staged work is reusable; invalid staged work fails
closed and is never silently relabelled or deleted. Final publication retains
content-addressed names, budget enforcement, and all existing path and resource
limits.

## E6 - Measurement

The maintained harness records independent raw samples for:

1. cold build;
2. warm no-op build;
3. one explicitly scoped content change;
4. one asset-only change; and
5. corrupt-cache rejection and recovery.

Reports identify the exact revision, OS, architecture, Rust toolchain, fixture,
sample vector, route/artifact counts, and peak memory when available. Cargo
fresh-process timing is not presented as an incremental framework result, and
local evidence is not generalized to Linux ARM64 or low-end devices.

Run a dirty one-sample harness smoke test with:

```sh
npm run measure:g4-engineering -- --samples 1
```

An accepted report additionally requires `--accept`, at least five independent
samples, and a clean exact Git revision. The release CLI is built outside every
timed region; each cold sample receives its own empty Cargo target.

## Exit boundary

Engineering readiness is complete only when the implementation, negative
tests, measurement harness, accepted evidence, public documentation, starter,
site, capability manifest, and changelog agree. G4 Adoption remains open until
an unaffiliated team completes its public-resource-only greenfield application
and partial migration.
