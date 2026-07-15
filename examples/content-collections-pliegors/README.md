# Native Content Ledger

This executable reference turns a neutral Markdown/YAML corpus into authored
Rust routes. It exercises typed collection loading, deterministic ordering,
safe CommonMark rendering, metadata, canonical URLs, static assets, and the
PliegoRS build ledger.

```sh
cargo build -p pliego-cli --locked
cd examples/content-collections-pliegors
../../target/debug/pliego build
../../target/debug/pliego inspect
```

The direct site binary does not publish without the bounded build invocation
created by `pliego build`.

The source corpus lives at `fixtures/content/reference`. The example is
self-contained and contains no customer or private product material.
