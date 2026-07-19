# G1 ordered-render foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** In progress; ordered sibling mode

**Toolchain:** Rust 1.86.0 under Debian WSL2

## Contract under test

The unreleased `pliego-runtime` source now emits an ordered HTML document as:

1. one validated and escaped document prefix;
2. zero or more independently validated sibling `pliego-dom` views; and
3. one document suffix.

Each `OrderedViewChunk` stores a `Send` factory rather than a `View`. The body
poll creates, renders, and drops one non-`Send` view at a time. This preserves
Hyper backpressure and PliegoRS's existing `Rc`-based DOM ownership without
pre-rendering the complete response.

The implementation also demonstrates:

- no `Content-Length` on ordered responses;
- one aggregate output budget including shell and every sibling;
- per-sibling DOM depth, node, and output enforcement;
- bounded chunk count and document metadata;
- panic isolation around stream polling and chunk rendering;
- post-commit failure without status replacement;
- cancellation wakeups through the runtime-owned response body; and
- `renderMode: "ordered"` in the exactly-once receipt.

## Reproduction

```bash
CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo test -p pliego-runtime --locked

CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo clippy \
  -p pliego-runtime --all-targets --locked -- -D warnings
```

Observed result:

- 13 runtime unit tests passed;
- 11 native runtime integration tests passed;
- doc tests passed; and
- Clippy with warnings denied passed.

The backpressure test consumes the prefix frame first and proves that no view
factory ran. It then consumes one frame at a time and observes exactly one new
factory invocation per sibling. The integration test dispatches through Axum,
consumes the stream, verifies sibling order, and inspects the receipt.

## Evidence boundary

Ordered mode streams complete sibling views. It does not incrementally walk one
view, expose an async placeholder boundary, or claim partial-prerendering
semantics. G1 remains open and `dynamic-ssr` remains `not-released`.
