# G0 product truth acceptance evidence

**Gate:** G0 - Product truth  
**Status:** Accepted  
**Recorded:** 2026-07-19  
**Implementation revision:** `7814b7e9fef06318d011e20b05b5e049d81663a2`

## Acceptance matrix

| ID | Requirement | Evidence | Result |
| --- | --- | --- | --- |
| G0-A01 | One machine-readable current-surface authority | `product.capabilities.json` validates against the strict Draft 2020-12 schema and negative stability cases fail | PASS |
| G0-A02 | Release, workspace, OpenSDK, MSRV, target, crate, and browser truth agree | `npm run check:product-truth` checks Cargo, npm, toolchain, support, documentation, and site source | PASS |
| G0-A03 | The official site publishes the authority without drift | Linux-built `/capabilities.json` is byte-identical to the repository file; the 75-route site contract passes | PASS |
| G0-A04 | Runtime, route, data/cache, deployment, and security boundaries are designed before implementation | RFC-007 through RFC-010 plus the full-stack threat model define the G1-G3 ownership and rejection rules | PASS |
| G0-A05 | Public repository documentation has one vocabulary | README, framework contract, constitution, capability guide, and backlog link current availability and stability terms | PASS |
| G0-A06 | Current registry and release claims match external state | GitHub release `v0.0.2` is public; all 15 released crates report `0.0.2`; `pliego-sdk` returns crates.io HTTP 404 | PASS |
| G0-A07 | The new website and documentation contracts execute on the production-oriented toolchain | Debian WSL2 Rust 1.86.0 builds the site and all 13 `pliegors-site` tests pass | PASS |
| G0-A08 | Local competitive research is not published as product documentation | `.gitignore` excludes `docs/research/`, the preliminary audit, and generated dossier PDFs | PASS |

## Canonical surface

The accepted manifest records 17 PliegoRS surfaces:

- released static generation, Rust/WASM UI, content, assets, and browser
  lifecycle;
- bounded source previews for OpenSDK build, browser, and tooling planes;
- explicit partial status for the optional Hyphae client boundary; and
- `not-released` status for native HTTP, dynamic SSR, full-stack routing,
  data/actions/cache, production observability, server OpenSDK, PBOC execution,
  native/OCI application deployment, and Cloudflare application runtime.

The public manifest contains no roadmap entry for separate future products. It
describes only capabilities owned by the PliegoRS repository.

## Local and WSL evidence

### Product-truth gate

```text
Product truth: PASS
release 0.0.2
workspace 0.0.2
OpenSDK 0.1.0-preview.1
15 released crates
17 surfaces
5 release targets
```

### Documentation

```text
documentation links PASS: 76 Markdown files
```

### Linux site build

Environment:

- Debian WSL2;
- `rustc 1.86.0`;
- `cargo 1.86.0`;
- `wasm-bindgen 0.2.126`; and
- persistent isolated Cargo target outside the repository.

```text
PliegoRS site: 75 routes and 114 files -> target/site
PLIEGO build: PliegoRS -> target/site [e363fa236b6c]
PliegoRS site contract passed: 75 routes, canonical SEO,
bilingual alternates, product examples absent.
```

`scripts/check-pliegors-site.mjs` compares the published capability bytes with
the root authority, so a copied or stale manifest fails the site contract.

### Site tests

```text
running 13 tests
test result: ok. 13 passed; 0 failed
```

The tests cover route language twins, documentation registry uniqueness,
canonical metadata, sitemap safety, security disclosure schema, localized
routes, and authored asset dimensions.

## External state snapshot

Verified on 2026-07-19:

```text
GitHub release: v0.0.2
draft: false
prerelease: false
published: 2026-07-18T23:09:27Z
```

The crates.io API returned `0.0.2` as `max_version` for:

```text
pliego-adapters, pliego-artifact, pliego-assets, pliego-cli,
pliego-content, pliego-dom, pliego-fold, pliego-hyphae,
pliego-inspect, pliego-log, pliego-macros, pliego-reactive,
pliego-resume, pliego-ssg, pliego-starters
```

`https://crates.io/api/v1/crates/pliego-sdk` returned HTTP 404. This external
snapshot is time-sensitive; the repository checker proves internal consistency,
while release automation must verify registry state again at promotion time.

## Failed-path evidence

The initial native Windows site execution was blocked by the machine's
Application Control policy after compilation (`os error 4551`). That run is not
counted as runtime evidence. The gate moved to Debian WSL2 and rebuilt with the
pinned toolchain.

The first site check then rejected a localized `/es/capabilities.json` link.
The site now uses the canonical absolute URL, rebuilds from the root bytes, and
passes the bilingual link corpus.

## Gate boundary

G0 establishes truth and design authority. It does not claim implementation of
the G1 runtime. `tiny_http` remains a static development/preview server, the
current OpenSDK HTTP world remains buffered and experimental, and no dynamic
SSR or production server surface is promoted by this acceptance record.
