# __NAME__

An editorial PliegoRS starter for journals, archives, magazines, research
studios, and independent publishers.

## Start

```powershell
pliego check
pliego dev
```

## Customize

- `src/main.rs`: publication identity, route structure, copy, metadata, and the
  `SITE_URL` constant.
- `assets/site.css`: color tokens, typography, grids, dark mode, and responsive
  composition.
- `assets/images`: replace all three demonstration photographs and update their
  alternative text in `src/main.rs`.
- `assets/fonts`: replace or remove the local type family files and their
  matching `@font-face` declarations. Keep the matching license files while
  those fonts remain in the project.
- `assets/site.webmanifest`, `robots.txt`, and `sitemap.xml`: replace the demo
  identity and production domain before launch.

The starter intentionally contains authored demonstration content. Treat it as
a structural and visual system, not final client copy.

Third-party font attribution and license locations are recorded in
`THIRD_PARTY_NOTICES.md`.
