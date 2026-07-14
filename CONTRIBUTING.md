<!-- SPDX-License-Identifier: Apache-2.0 -->

# Contributing to PliegoRS

PliegoRS accepts focused, reviewable contributions once the repository is
public. Start with an issue for behavior changes, new public APIs, protocol
changes, dependencies, or work that spans more than one crate. Small fixes and
documentation corrections may go directly to a pull request.

## Development setup

Install the Rust toolchain declared in `rust-toolchain.toml`, Node.js for the
repository verification scripts, and `wasm-bindgen-cli` when changing a browser
client.

```sh
cargo build -p pliego-cli
cargo test --workspace --all-targets --locked
npm ci
npm run check:docs
npm run check:site
```

The complete quality-gate list lives in `README.md`. Run the narrowest relevant
tests while developing and the full applicable gates before requesting review.

## Change contract

- Keep public API changes explicit and document their compatibility impact.
- Add regression tests for bug fixes and contract tests for serialized formats.
- Preserve deterministic output, safe HTML escaping, bounded inputs, and
  cleanup semantics.
- Do not add a production dependency without explaining its purpose, license,
  maintenance status, and size impact.
- Do not include generated build output, credentials, private datasets, or
  third-party media without redistribution rights.
- Keep source, docs, examples, and error diagnostics in sync.

## Pull requests

Use a descriptive title, explain the problem and the chosen behavior, list the
commands used for verification, and call out compatibility or security effects.
Keep unrelated refactors out of the same change.

By submitting a contribution, you represent that you have the right to submit
it and agree that it is licensed under Apache-2.0, the repository license.

Participation is governed by `CODE_OF_CONDUCT.md`. Security reports follow
`SECURITY.md`, not the public issue tracker.
