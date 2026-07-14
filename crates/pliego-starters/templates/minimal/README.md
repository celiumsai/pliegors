# __NAME__

A minimal PliegoRS site with two routes, complete metadata, responsive styles,
local assets, and a deterministic build ledger.

## Start

```powershell
pliego check
pliego dev
```

## Customize

- `src/main.rs`: routes, metadata, copy, and document structure.
- `assets/site.css`: typography, color tokens, layout, and responsive behavior.
- `assets/pliego-mark.svg`: replace the starter mark with your own identity.
- `SITE_URL` in `src/main.rs`: replace `https://example.com` before launch.
- `pliego.toml`: project name, Cargo package, and output directory.

Production output is written to `target/site` by default. Run `pliego inspect`
after a build to review its route, file, and byte totals.
