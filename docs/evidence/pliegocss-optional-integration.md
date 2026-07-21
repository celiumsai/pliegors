# Optional PliegoCSS companion evidence

**State:** Active experimental integration; never required by PliegoRS

**Observed:** 2026-07-21 on Windows x86-64 with Rust 1.86

## Ownership boundary

PliegoRS accepts ordinary CSS and does not link PliegoCSS crates into its
browser or server runtimes. `pliego css check` is an explicit local delegation
to a separately installed `pliego-cssc` process. It performs no installation,
download, or network call. The default starter and every core framework gate
remain valid without PliegoCSS.

The validated companion is `pliego-cssc 0.1.0-rc.2`. Compilation, watch mode,
manifests, and route/island bundles remain direct PliegoCSS workflows. Their
output is standards-compliant static CSS plus versioned evidence artifacts;
PliegoRS consumes the selected CSS as ordinary verified assets.

## Cross-repository replay

The PliegoCSS repository commit `b920974` pins its schema-2 integration
contract to PliegoRS commit `b031222` and covered-source SHA-256
`5b7e09211e80715c25f33196ee6cb1ef1de48109ca2dfdb417b15293dc7c991b`.
The replay command was:

```powershell
$env:PLIEGORS_ROOT='<pliegors-checkout>'
pnpm integration:pliegors
```

The gate passed with:

- three Cargo targets: native site library, SSG binary, and browser WASM;
- 12 exact Rust source units and five typed style sites;
- two deterministic SSG routes;
- shared, route, island, and fully pruned unreachable bundles;
- exact CSS and manifest hashes plus one selected theme-bearing preload;
- resumable island state from 15 to 20; and
- a 31,485-byte raw WASM payload inside a 40,024-byte raw browser payload.

The same gate rejects a dirty or different PliegoRS contract before Cargo runs
and verifies that the checkout remains unchanged after the replay.

## Claim boundary

This evidence proves the exact current pair and its framework-neutral artifact
boundary. It does not make PliegoCSS part of the PliegoRS support requirement,
promise compatibility with an arbitrary future PliegoCSS version, or replace
hosted multi-platform integration evidence. The delegated command remains
experimental until that compatibility matrix and release-lifecycle evidence
exist.
