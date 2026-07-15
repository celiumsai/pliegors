# PLIEGO assets

**Status:** Phase 2 adaptive pipeline contract complete
**Crate and CLI:** `crates/pliego-assets`

`pliego-assets` is the deterministic orchestration layer for adaptive media. It
does not reimplement codecs or hide encoder versions. Rust owns recipes,
validation, plans, ordering, tier policies, device budgets, hashes,
content-addressed publication, runtime delivery directives, and manifests.
Pinned external toolchains own encoding.

The contract covers:

- AVIF, WebP, and JPEG responsive images;
- H.264/AV1 MP4 and VP9 WebM video ladders;
- WOFF2 font subsetting with bounded glyph and Unicode ranges;
- GLB LODs with Meshopt or Draco geometry and ETC1S/UASTC KTX2 textures;
- Universal, Lite, Balanced, and Signature tiers;
- modest-mobile, tablet, and desktop byte/VRAM/geometry/preload budgets;
- initial, deferred, and interaction-only loading;
- Save-Data and reduced-motion suppression.

The machine-readable contracts are:

- `schemas/pliego.adaptive-asset-recipe.schema.json`;
- `schemas/pliego.adaptive-asset-plan.schema.json`;
- `schemas/pliego.adaptive-asset-manifest.schema.json`.

## Plan and finalize

An adaptive recipe never contains a shell command. Each transform becomes a
typed job with an exact staging path and the pinned capabilities it requires.

```powershell
cargo run -p pliego-assets -- plan assets.recipe.json `
  --source-root public\sources `
  --out target\assets\plan.json
```

An external runner reads each job's structured `operation`, invokes the pinned
tool without a shell, and writes only to the job's `stagingPath`. PliegoRS then
validates and publishes those outputs:

```powershell
cargo run -p pliego-assets -- finalize target\assets\plan.json `
  --output-root target\site `
  --manifest target\site\pliego.adaptive-assets.json
```

Finalization checks the declared container signature, streams SHA-256 without
unbounded allocation, evaluates every applicable device/tier budget before the
first publication, and emits names such as
`assets/hero/mobile.7f83b1657ff1fc53.avif`. A budget failure leaves every
staged artifact in place and publishes nothing.

Raster codecs, video codecs, and scene LODs belonging to one source carry a
deterministic `selectionGroup`: budgets charge the largest selectable transfer,
decode, or geometry cost instead of incorrectly summing mutually exclusive
fallbacks. Font subsets remain cumulative because more than one range can be
needed on the same page.

The Rust `AdaptiveManifest::delivery` method converts one manifest plus a tier,
Save-Data flag, and reduced-motion flag into explicit `eager`, `lazy`, or
`interaction` directives. Codec capability selection remains with the browser
adapter because support is runtime evidence, not a build-time guess.

## Runtime bridge

The manifest and plugin contracts are intentionally separate but composable.
Rust selects the permitted manifest entries with `AdaptiveManifest::delivery`,
serializes only those content-addressed URLs into adapter props, and declares
the same minimum tier and capabilities on `AdapterIsland`. The v1 runtime then
re-evaluates Save-Data, reduced motion, tier, and trigger policy before it
imports Three.js, GSAP, Lenis, or another native ESM module.

An interaction-only GLB therefore stays absent from the initial request graph,
a Save-Data or reduced-motion visitor keeps useful authored fallback markup,
and removing the island aborts pending work and runs the plugin cleanup stack.
The asset manifest never imports JavaScript and the adapter never chooses an
artifact outside the validated manifest.

## Safety invariants

- Absolute paths, `..`, backslashes, Windows device names, alternate data
  streams, non-UTF-8 names, symlinks, and forged staging paths fail closed.
- Recipes allow at most 512 sources, 4,096 jobs, 2 GiB per source/artifact, and
  8 GiB each of source material and staged artifacts per plan. Aggregate staged
  bytes are checked before any artifact validation or hashing.
- These are processing-plan ceilings, not the static-site publication budget.
  Receipt v2 publication is intentionally narrower: at most 512 MiB per output
  file and 4 GiB for the complete payload set. Larger processed media must be
  split or delivered through a separately verified external origin.
- Unknown JSON fields, duplicate IDs, duplicate toolchain pins, unpinned
  capabilities, malformed hashes, incompatible operations, and forged decoded
  memory or geometry estimates are rejected.
- Video and 3D cannot target Universal and must opt into reduced-motion
  suppression. Deferred/on-demand assets cannot preload or use high priority.
- Publication uses create-new semantics, verifies existing content-addresses,
  rejects linked destinations, and preflights collisions before writing.
- The manifest contains no timestamp, absolute path, random identifier, or
  host-specific value. Identical recipe/source bytes produce identical plans.

## Raster command

```powershell
cargo run -p pliego-assets -- raster path\to\hero.png `
  --out path\to\generated `
  --id hero `
  --widths 480,960,1440 `
  --formats avif,webp,jpeg `
  --quality 78
```

Requested widths above the source width collapse to the source width. Repeated
widths and formats are deduplicated and sorted. Each encoded file is named from
its asset ID, actual width, and the first 16 hexadecimal characters of its full
SHA-256. The ledger stores the full hash and contains no timestamp, absolute
path, random identifier, or machine-specific field.

The backend runs single-threaded, strips source metadata, and requests FFmpeg's
bit-exact mode. Reproducibility is scoped to the recorded backend version; a
toolchain change is visible in the ledger and must be reviewed before publishing
new variants.

## Public reference

The official PliegoRS site is the maintained public manifest target. Its
content-addressed images, fonts, icons, and downloadable artifacts are checked
against the Universal first-viewport budget by `pliego-inspect`. Private
acceptance applications are intentionally excluded from this repository.

## Toolchain boundary

PliegoRS ships the stable orchestration and verification contract, not Blender,
FFmpeg, Fonttools, glTF Transform, or a KTX2 encoder. CI and studio runners pin
those binaries and may add stronger codec-specific probes without changing the
recipe or manifest API. Missing tools prevent execution; they never cause a
silent copy or a falsely labelled output.

The original `raster` command remains available as the first bundled FFmpeg
runner. Its outputs now share the same symlink rejection and no-clobber
publication guarantees as the adaptive finalizer. The compatibility pipeline
streams fingerprints and rejects sources or variants above 256 MiB, more than
1 GiB of aggregate output, more than 32 widths or 96 variants, dimensions above
16,384, and decoded raster surfaces above 64 million pixels.
