# R4 DOM lifecycle acceptance evidence

**Gate:** R4 - DOM lifecycle

**Status:** Accepted

**Recorded:** 2026-07-16

**Platform boundary:** Debian WSL2 for native Rust tests; Windows 11 and Chrome
150 for browser execution

## Acceptance matrix

| ID | Requirement | Evidence | Result |
|---|---|---|---|
| R4-A01 | One owner for nodes, effects, listeners, and dynamic children | `MountScope`, exact range/node registries, owner tests | PASS |
| R4-A02 | Adapters are causally bound to mounted scopes | synchronous `pliego:scope-dispose`, Node and Chrome gates | PASS |
| R4-A03 | No forgotten WASM listener ownership | `check-wasm-lifetimes.mjs` scans maintained Rust/WASM surfaces | PASS |
| R4-A04 | Cleanup is LIFO and idempotent before DOM removal | Chromium order test and repeated dispose tests | PASS |
| R4-A05 | Tags, attributes, namespaces, URLs, and parser topology are validated | native and Chromium adversarial construction suites | PASS |
| R4-A06 | Keyed updates preserve identity and minimize moves | typed-key tests, retained listener/focus checks, LIS reorder plan | PASS |
| R4-A07 | Duplicate, oversized, and hostile keyed updates fail closed | keyed native/browser limits and foreign-gap tests | PASS |
| R4-A08 | SSR adoption reuses exact authored nodes | static, dynamic, nested, SVG, and keyed Chromium cases | PASS |
| R4-A09 | Adoption mismatch is diagnostic and non-mutating | structured preflight mismatch and complete commit rollback tests | PASS |
| R4-A10 | 10,000 mount/dispose cycles plateau | no connected DOM residue after each cycle | PASS |
| R4-A11 | Pending plugin work cannot postpone registered cleanup | never-settling mount/update Node cases and Chrome update case | PASS |
| R4-A12 | Reduced motion, lazy scheduling, and page lifecycle clean automatically | live policy, schedule cancellation, DOM removal, pagehide tests | PASS |

## Implementation chain

- `1b501aec70017d774409b118ed0de5fe3dc7f88f` - explicit reactive owners
- `8fbdb78c1fb3a5d3275287fc9101881a60386d49` - exact DOM lifecycle ownership
- `52fccbae2e301feef1a86a0e6a76636c7d1ad227` - retained keyed reconciliation
- `feb5395ab4d8abed0adfe623cc20404e09cc4e20` - versioned SSR adoption
- `5d6cddb10b69992bc8146d2f509911586f055f9c` - scope cleanup/adapter bridge
- `f25c75ab2bbe696498890ffa4c8459e02fbb9d1a` - official client lifetime ownership
- `e5e7076750f779c130756c7aef285a76d59ad2bf` - adapter cancellation and browser gates
- `f31f8913c53c139c106f7407ce17d720357d5753` - explicit scope LIFO proof
- `786fd7fab49065cd9df10901954517568c0986f7` - normative contract and initial R4 record
- `37a9d90dc4d9729327e20131a5e69b11c52b6632` - retained spike root found by the strict WASM replay

## Verified behavior

### Exact ownership and teardown

Mount ranges freeze their authored top-level node identities while detached.
Cleanup removes only those nodes plus identities registered by nested scopes.
Foreign nodes inserted between boundaries survive. Custom-element callbacks that
move or reinsert owned nodes are drained with a finite pass bound; failure to
converge is terminal and remains attached to the nearest live owner.

The scope event and two registered callbacks were observed in this exact order
while the adapter element remained connected:

```text
pliego:scope-dispose
cleanup-second
cleanup-first
DOM removal
```

Repeated `dispose()` did not repeat any callback or removal.

### Keyed reconciliation

The browser suite covers prepend, append, delete, reverse, rotate, mixed
reorder, and newly inserted keys. Retained rows keep node identity and listener
state; the focused input is restored after browser moves. New row builders run
only for new key lifetimes. Duplicate keys, key budgets, unsupported parents,
foreign gaps, moved descendants, reentry, and custom-element topology attacks
fail without claiming ownership of foreign DOM.

### SSR adoption

Plain `render_html` output did not change. Adoptable rendering emits an explicit
`pliego:ssr:v1` root and bounded internal markers. Browser preflight checks
complete structure, exact attributes and namespaces, text, dynamic first reads,
and keyed identities before installing resources. Static, dynamic text,
dynamic subtree, keyed, nested dynamic/keyed, empty text, RCDATA, and qualified
SVG attribute cases passed. Forged namespace and content mismatches returned
bounded paths without mutation. A mismatch triggered after preflight retired the
complete seed and its installed listener.

### Adapter and client lifetime

Adapter runtime `1.1.0` installs a provisional lifecycle before calling plugin
`mount`. Scope disposal aborts it and drains registered resources immediately,
even when `mount` or `update` never resolves. A late returned cleanup executes
once without mounting the obsolete generation. The real Chrome gate proves
capture-phase scope disposal, a blocked update, and MutationObserver removal.

The official site client now owns all event closures, intersection observers,
and carousel intervals through a page `MountScope`. Its carousel responds to
live reduced-motion changes. CI rejects `Closure::forget`, `.forget()`, and
`mem::forget` in maintained browser surfaces.

## Gate results

The accepted replay used
`PLIEGORS_SOURCE_REV=37a9d90dc4d9729327e20131a5e69b11c52b6632`.
Its gate families and focused counts are:

- Debian `pliego-dom` native: `27` passed, `0` failed.
- Chrome `pliego-dom` lifecycle: `54` passed, `0` failed, including the 10,000
  cycle case.
- Adapter Node runtime: `21` passed, `0` failed.
- Adapter real-browser gate: scope, pending update, and DOM removal PASS.
- WASM lifetime source gate: `9` Rust source files, no ownership escape.
- Official site: `47` routes and `78` files built through `pliego build`.
- Phase 1 tooling: `86` Node tests passed, `0` failed.

## Environment

- Rust `1.85.0`, Cargo `1.85.0`
- Node `24.16.0`, npm `11.13.0`
- Debian WSL2, Linux `6.18.33.1-microsoft-standard-WSL2`, x86_64
- Chrome and ChromeDriver `150.0.7871.124`

Native Windows test executables remain subject to local Windows Application
Control. Browser/WASM execution is real Chrome on Windows; native certification
uses Linux binaries and a Linux target directory under Debian WSL2.

## Residual boundaries

- The plateau gate proves zero connected DOM residue across 10,000 browser
  mount/dispose cycles. Reactive arena slot reuse is independently certified by
  R0; browser heap size is not claimed from this test.
- JavaScript cannot forcibly terminate an arbitrary promise. The adapter
  contract aborts and cleans registered resources immediately; a plugin must
  observe `context.signal` to stop its own non-registered work.
- SSR adoption is strict complete-seed reuse, not streaming SSR or heuristic
  adoption of third-party markup.

No unresolved R4 P0 or P1 finding remains within these boundaries.

## Final replay

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| Debian `cargo test --workspace --all-targets --locked` | PASS |
| Debian `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| Debian `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | PASS, 20 workspace documentation roots |
| Debian `cargo test -p pliego-dom --lib --locked` | PASS, 27/27 |
| Debian `cargo test -p pliego-adapters --all-targets --locked` | PASS, 8/8 |
| WASM Clippy for `pliego-dom`, official client, and spike | PASS |
| Real Chrome `browser_lifecycle` | PASS, 54/54 |
| Adapter Node adversarial suite | PASS, 21/21 |
| Adapter real-Chrome gate | PASS, scope, pending update, DOM removal |
| WASM lifetime source audit | PASS, 9 Rust source files |
| Documentation links | PASS, 48 Markdown files |
| Distribution policy | PASS, 15 source crates and five private targets |
| Official site contract | PASS, canonical SEO and bilingual alternates |
| Two exact official-site builds | PASS, 47 routes, 78 files, identical ledger |
| Phase 1 test suite | PASS, 86/86 |
| Phase 1 deterministic gate | PASS |
| Site deployment package | PASS, Wrangler dry-run only |
| `git diff --check` | PASS |

The two official-site builds produced ledger SHA-256
`9d854fb2321cc729e5d44fdf8ac918d14ef597011f45651d464d1f94087b723e`.
No deploy or release was performed.

The first strict WASM replay rejected `examples/spike` because it discarded the
`MountedRoot` result. Under the R4 ownership contract that would immediately
drop and dispose the application. Commit `37a9d90` stores the root in an explicit
page owner; the complete accepted replay above ran after that fix. A later focal
Linux retry initially exhausted the temporary target filesystem after multiple
fresh builds. Only verified `/tmp/pliegors-r4-*` targets were removed, and the
same focal suites then passed without changing code or relaxing a gate.
