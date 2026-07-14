# __NAME__

A full-bleed cinematic PliegoRS starter for films, festivals, visual studios,
artists, and narrative launches.

## Start

```powershell
pliego check
pliego dev
```

## Customize

- `src/main.rs`: title, credits, story, metadata, JSON-LD, production URL, and
  screening details.
- `assets/site.css`: scene framing, palette, motion, responsive layouts, and
  reduced-motion behavior.
- `assets/afterlight-scene.jpg`: replace the demonstration key art and update
  image dimensions and alternative text in `src/main.rs`.
- `assets/fonts`: replace or remove the local type family files and matching
  `@font-face` declarations. Keep the matching license files while those fonts
  remain in the project.
- Inline constants `FAVICON`, `MANIFEST`, `ROBOTS`, and `SITEMAP` in
  `src/main.rs`: replace the demonstration identity and domain before launch.

Motion is CSS-only in this starter. A production project can add a declared
PliegoRS adapter for GSAP, Three.js, WebGL, or another external motion engine.

Third-party font attribution and license locations are recorded in
`THIRD_PARTY_NOTICES.md`.
