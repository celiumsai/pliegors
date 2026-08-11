<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Third-party notices

PliegoRS depends on third-party Rust and JavaScript packages. Their names,
versions, sources, and resolved checksums are recorded in `Cargo.lock`, the root
`package-lock.json`, Worker lockfiles, and provider lockfiles; each remains
subject to its own license. Source and binary redistributions must preserve
notices required by those licenses.

Maintained starters that bundle fonts or media carry their attribution and
license texts inside the generated project. In particular, the `editorial` and
`cinematic` starters include `THIRD_PARTY_NOTICES.md` plus font license files
beside the bundled fonts. Crate tests fail when those files are absent.

The official documentation site bundles Instrument Serif, Instrument Sans,
and Fragment Mono under the SIL Open Font License 1.1. Their complete license
texts are stored beside the font files in `examples/pliegors-site/public/fonts`.
The original PliegoRS graphics and media under
`examples/pliegors-site/public/media` are Celiums Solutions LLC project assets,
are not third-party stock media, and fall within the project-authored
CC-BY-SA-4.0 non-software scope described in `LICENSING.md`.

The official `default` starter contains no third-party media or fonts. Generated
projects receive a neutral `LICENSE.md` placeholder; PliegoRS itself remains
AGPL-3.0-only.

This file is a distribution index, not a replacement for dependency license
texts or for notices placed beside bundled assets.
