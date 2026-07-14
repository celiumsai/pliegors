# Official starters and CLI diagnostics

**Status:** DX-05 and DX-06 accepted; official first-use contract revised on 2026-07-14

## Architecture

Official starters belong to `pliego-starters`, not `pliego-cli`. Each starter
owns its Cargo manifest template, PliegoRS manifest template, ignore policy,
Rust source, assets, README, revision, and capability declaration. The CLI links
the crate and embeds its bytes, so the distributed executable remains
standalone without acquiring project identity or asset ownership.

Scaffolding is transactional:

1. Parse the requested starter and explicit dependency source.
2. Reject non-portable paths, reserved paths, case-insensitive duplicates,
   file/directory collisions, symbolic-link destinations, and unresolved tokens.
3. Render typed plain-text or JSON tokens into a sibling staging directory.
4. Create every file with `create_new` semantics.
5. Rename the complete staged tree into the final empty destination.

The dependency order is explicit and reproducible:

1. `--framework-path <checkout>`;
2. `PLIEGO_FRAMEWORK_PATH`;
3. the canonical PliegoRS Git repository at the exact source revision embedded
   in the CLI.

The CLI does not silently switch to local path dependencies because it happens
to run inside a PliegoRS checkout.

## Catalog

| ID | Revision | Intended use | Capabilities |
| --- | ---: | --- | --- |
| `default` | 1 | first PliegoRS project and framework onboarding | SSG, SEO, local guide, branded errors |
| `minimal` | 1 | studios, portfolios, small authored sites | SSG, SEO, responsive |
| `editorial` | 1 | journals, archives, research, publishing | SSG, SEO, local media, dark mode |
| `cinematic` | 1 | films, festivals, visual launches | SSG, SEO, local media, adaptive motion |

```powershell
pliego templates
pliego new my-project
pliego new my-journal --template editorial
pliego new my-film --template cinematic
```

`default` is the explicit CLI default. It ships `/`, `/guide/`, an authored
`/404.html`, the PliegoRS favicon, complete metadata, a web manifest, crawler
policy, README, and Apache-2.0 license. The other entries remain opt-in design
starters rather than silently defining the framework's first-run experience.

Each generated README identifies the source, identity, domain, metadata, image,
font, copy, and style files that must be changed before launch. Demonstration
brands and domains are deliberately visible rather than hidden in framework
internals.

The editorial and cinematic source trees also include the complete SIL OFL
license texts and a `THIRD_PARTY_NOTICES.md` file. A crate test requires those
files whenever a starter embeds local font binaries. Every maintained starter
ships the framework `LICENSE` file.

## Diagnostics

Human diagnostics include a stable identifier, category, message, and next
action. Machine consumers can request one JSON object on standard error:

```powershell
pliego build --diagnostic-format json
```

| Exit | Identifier family | Meaning |
| ---: | --- | --- |
| `0` | none | success |
| `2` | `PLG-ARG-*` | command, option, port, or starter selection |
| `3` | `PLG-PRJ-*`, `PLG-NEW-*` | project discovery/configuration or scaffold transaction |
| `4` | `PLG-ENV-*` | package target, Rust target, or required tool |
| `5` | `PLG-BLD-*` | compilation or site build |
| `6` | `PLG-ART-*` | missing or invalid build artifact/ledger |
| `7` | `PLG-SRV-*` | preview/development server |

`pliego dev` keeps serving when the initial build or a rebuild fails. Document
requests receive a branded HTTP 500 diagnostic surface containing
`PLG-BLD-001`, escaped compiler output, a concrete recovery instruction, and a
live-reload channel. Successful recompilation clears the failure and reloads
the valid document. Missing routes use the project's authored `/404.html`; when
one is absent, the server returns a branded `PLG-HTTP-404` fallback instead of
an untyped text response.

## Acceptance evidence

The original three design starters were generated from the standalone test binary outside the
framework workspace with `--name "Acceptance <template>"`, then ran
`pliego check`, two identical `pliego build` passes, and `pliego inspect`.

| Starter | Routes | Files | Bytes | Ledger SHA-256 |
| --- | ---: | ---: | ---: | --- |
| default | 3 | 7 | 15,429 | `CF07AF0A3E7A69AAE54A247744A3B818A7F3EFF8A6DCD9BDA717D9C1CB0C19B2` |
| minimal | 2 | 5 | 7,667 | `240BFA68604A6BFE054ED55DC041A5A4AACA239A2BA339294E84537BC0D29D07` |
| editorial | 2 | 14 | 1,267,285 | `064B5F1CFB89092E5097FB4EF7A297014D9A41C656064288945916E9C599811E` |
| cinematic | 2 | 12 | 405,284 | `FBBCAE6AE41CC92DB174BE022DA2880608EBDC6977F0FC1393A69FBA7DA85967` |

The default starter was scaffolded without `--template`, checked, built twice,
inspected, and produced the identical ledger hash shown above. Ledger hashes
matched across both builds. Every emitted file matches its ledger,
every local HTML/CSS reference resolves, former-framework runtime markers are
absent, canonical routes return HTTP 200, unknown routes return the authored 404,
and preview output contains no development reload hook.

Browser acceptance used 1440x1000 desktop and 390x844 mobile viewports. All six
cases had zero horizontal overflow, zero clipped text, and no failed visible
images. Editorial lazy images were additionally loaded after a 7,117-pixel
scroll and all reported their real 1536-pixel intrinsic width.

`pliego-starters` also passes Cargo's packaged-tarball verification: 49 files,
1.6 MiB compressed. Framework crates explicitly reject registry publication;
the distributed CLI instead generates `git + rev` dependencies that resolve
the same canonical source commit on every machine once the repository opens.

| Minimal | Editorial | Cinematic |
| --- | --- | --- |
| ![Minimal desktop](baselines/starters/minimal-desktop-viewport.png) | ![Editorial desktop](baselines/starters/editorial-desktop-viewport.png) | ![Cinematic desktop](baselines/starters/cinematic-desktop-viewport.png) |
| ![Minimal mobile](baselines/starters/minimal-mobile-viewport.png) | ![Editorial mobile](baselines/starters/editorial-mobile-viewport.png) | ![Cinematic mobile](baselines/starters/cinematic-mobile-viewport.png) |
