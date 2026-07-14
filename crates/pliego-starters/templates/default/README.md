# __NAME__

The official PliegoRS starter: three routes, complete metadata, a branded 404,
local assets, and a deterministic build ledger. The first screen is deliberately
an onboarding surface so the project explains itself before you replace it.

## First run

```powershell
pliego check
pliego dev
```

Open `http://127.0.0.1:4400`. PliegoRS watches the project, rebuilds it, and
reloads the browser after each successful change. Build failures are rendered as
diagnostic pages in development.

## Project map

- `src/main.rs`: routes, document metadata, and views written in Rust.
- `assets/site.css`: design tokens, layout, and responsive behavior.
- `assets/favicon.svg`: the PliegoRS starter identity. Replace it before launch.
- `assets/site.webmanifest`: install metadata and theme colors.
- `assets/robots.txt`: crawler policy.
- `pliego.toml`: project identity, Cargo package, and output directory.

## Make the first change

Edit the `home()` view in `src/main.rs`, save it, and watch the browser reload.
Then add a `Page::new(...)` entry in `main()` to create another route.

## Production

```powershell
pliego check
pliego build
pliego inspect
pliego preview
```

The deployable site is written to `target/site`. Replace `https://example.com`
in `src/main.rs` before launch so canonical and social URLs are correct.

Documentation: https://pliegors.dev/docs/getting-started

## License

This starter is distributed under Apache-2.0; see `LICENSE`. Your original
application code and assets remain yours. Preserve notices for any third-party
dependencies or media you add.
