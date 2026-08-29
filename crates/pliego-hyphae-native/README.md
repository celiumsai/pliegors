<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# PliegoRS Hyphae Native adapter

`pliego-hyphae-native` is an experimental, application-facing server adapter for running a separately installed Hyphae Native process as a bounded durable store. Version `0.4.0-beta.1` is published on crates.io as part of the PliegoRS beta family. It preserves the PliegoRS Rust 1.86 dependency graph by communicating with Hyphae over loopback HTTP instead of linking a `hyphae-*` crate, and it keeps Hyphae protocol values out of other `pliego-*` APIs.

## Topology

```text
Browser
  |
  | Pliego-owned pages, forms, loaders, and actions
  v
Pliego server
  |
  | bounded product envelopes; credentials remain here
  v
127.0.0.1 /v2
  |
  v
Hyphae Native sidecar -> Hyphae-owned data directory
```

The browser never receives Hyphae credentials, product envelopes, sidecar addresses, or a `/v2` proxy route.

Loopback limits network exposure; it is not an authorization boundary against hostile processes running as the same local user. Deploy the sidecar within the application's trusted host boundary.

## Pinned identity

The first adapter targets only Hyphae `v1.0.1` at commit `84161cf067141b60f4847b965ef77c5b749749c0`. `HyphaeInstallation::admit` checks the reviewed executable digest, `HyphaeSidecar::start` checks the process version and product capabilities, and `NativeHttpClient` enforces the loopback address and the `application/vnd.hyphae.product-v1` and `application/vnd.hyphae.error-v1` media types. A later Hyphae release requires a written compatibility review; current Hyphae `main` is not an implicit upgrade target.

Reviewed executable admission currently supports `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`. Other PliegoRS targets fail admission until their exact Hyphae artifacts are reviewed.

## Native HTTP operations

WP1 implements these exact `v1.0.1` operations:

- OpenAPI `nativeCapabilitiesV2`: `GET /v2/capabilities`.
- OpenAPI `executeNativeOperationV2`: `POST /v2/execute`, carrying exact product operation `structure_get` (`5`).
- OpenAPI `executeNativeOperationV2`: `POST /v2/execute`, carrying exact product operation `structure_set` (`6`).
- OpenAPI `executeNativeOperationV2`: `POST /v2/execute`, carrying exact product operation `transaction_status_by_idempotency` (`39`).

`NativeHttpClient::set` never retries an uncertain mutation as a second semantic write. A timeout, lost response, released `unknown_commit`, or `OutcomeUnknown` commit is resolved with `transaction_status_by_idempotency` and the original nonzero idempotency token; only a matching strict committed receipt is success.

Later work packages may add the exact documented product operations `transaction_begin`, `transaction_stage_structure`, `transaction_commit`, `transaction_rollback`, `explicit_transaction_status`, `search_ingest`, `search_collection`, `proof_generate`, and `proof_verify` through OpenAPI `executeNativeOperationV2` at `POST /v2/execute`. Those operations are not implemented or simulated by this first extraction.

## Local installation

Set `HYPHAE_BIN` to the reviewed `v1.0.1` executable and `HYPHAE_DATA_DIR` to the sidecar-owned directory:

```sh
HYPHAE_BIN=/opt/hyphae/1.0.1/hyphae \
HYPHAE_DATA_DIR=/var/lib/my-pliego-app/hyphae \
cargo run -p my-pliego-app
```

Applications load `SidecarAuthority`, admit `HyphaeInstallation::from_env`, and start `HyphaeSidecar::start_from_env`. Merely setting the variables does not bypass executable or capability admission.

## Non-goals

This crate is not a browser SDK, identity provider, embedder, arbitrary SQL or administration console, telemetry dashboard, backup UI, framework-wide default store, replacement for `mycelium-do`, or implementation of `pliego-hyphae` verified-sync v2. It makes no production cutover, scale, or comparative performance claim.
