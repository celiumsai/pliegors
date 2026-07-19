# G1 complete-render foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** In progress; complete mode only

**Toolchain:** Rust 1.86.0 under Debian WSL2

## Contract under test

The unreleased `pliego-runtime` source now renders a `pliego-dom` body into a
typed complete HTML document. The contract includes:

- one output budget covering doctype, metadata shell, body, and closing tags;
- escaped title, description, canonical, language, asset paths, and body;
- local absolute stylesheet and module-script paths;
- relative or `http`/`https` canonical URLs;
- explicit plain versus adoptable SSR seeds;
- rejection of bodyless HTTP statuses;
- DOM parser-repair and resource-limit errors before response commitment;
- `Content-Type` and exact `Content-Length`; and
- `renderMode: "complete"` in the exactly-once runtime receipt.

## Reproduction

```bash
CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo test -p pliego-runtime --locked

CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo clippy \
  -p pliego-runtime --all-targets --locked -- -D warnings
```

Observed result:

- 11 runtime unit tests passed;
- 10 native runtime integration tests passed;
- doc tests passed; and
- Clippy with warnings denied passed.

The complete-render integration test dispatches through the real Axum service,
consumes the owned response body, verifies escaping, and inspects the emitted
runtime receipt.

## Evidence boundary

This slice does not implement ordered streaming or asynchronous boundaries. It
does not close G1, does not publish `pliego-runtime`, and does not change
`dynamic-ssr` from `not-released`.
