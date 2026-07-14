# Native Content Ledger

This executable reference turns a neutral Markdown/YAML corpus into authored
Rust routes. It exercises typed collection loading, deterministic ordering,
safe CommonMark rendering, metadata, canonical URLs, static assets, and the
PliegoRS build ledger.

```powershell
cargo run -p content-collections-pliegors -- examples/content-collections-pliegors/target/site
```

The source corpus lives at `fixtures/content/reference`. The example is
self-contained and contains no customer or private product material.
