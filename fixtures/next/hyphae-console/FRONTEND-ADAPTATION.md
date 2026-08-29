<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# Hyphae Console frontend adaptation

## Source finding

Hyphae does not currently ship a browser frontend to port. Current `main` has a Rust Ratatui operator console under `crates/hyphae-cli/src/tui.rs`; its information architecture can inform this fixture, but its widgets are not web components and current `main` targets Native 2.2.0 rather than the adapter's pinned Native `v1.0.1` contract.

The frontend will therefore be a PliegoRS application over Pliego-owned routes and DTOs, not a browser build of the Hyphae SDK and not a generic `/v2` proxy.

## Product boundary

```text
Browser UI
  -> same-origin Pliego route, loader, or action
  -> authentication, CSRF, tenant scope, authorization, and bounds
  -> pliego-hyphae-native application API
  -> loopback-only Hyphae /v2
```

The browser must not learn the sidecar address, bearer value, `X-Hyphae-Session-Id`, product media types, raw operation envelopes, or unrestricted engine error details. Tenant and principal are derived from the admitted server session rather than accepted from browser fields.

## First frontend slice

The first useful UI should adapt the safe parts of Hyphae's terminal information architecture:

| View | Pliego route | Server capability | Browser result |
| --- | --- | --- | --- |
| Overview | `GET /console` | Sidecar admission plus tenant state summary | Bounded compatibility and state status |
| Catalog | `GET /console/catalog` | Documents and stored chunks | Paginated stored citations |
| Ingest | `POST /console/catalog/ingest` | `dev.celiums.hyphae.ingest/v1` action | Durable receipt or explicit unknown outcome |
| Search | `POST /console/search` | `dev.celiums.hyphae.retrieve/v1` loader | Stored citation hits plus proof state |
| Verify | `POST /console/proofs/verify` | `dev.celiums.hyphae.verify/v1` action | Verified or rejected proof result |
| Activity | `GET /console/activity` | Pliego-owned bounded history | Ordered SSR stream |

Only Overview and the existing tenant activity/counter demonstration can operate before the corresponding adapter work packages exist. Catalog, ingest, search, and proof routes must not be mounted as successful mocks.

## Interaction contract

- Complete SSR is the baseline; critical login, navigation, ingest, and verification flows work without JavaScript.
- Optional islands may improve filtering or progressive input, but can call only same-origin Pliego routes.
- Ingest presents `editing`, `validating`, `committing`, `durable`, `rolled back`, and `outcome unknown` states. Pending work is never styled as durable.
- Search presents exact stored citation text. It never substitutes a model paraphrase.
- A hit whose proof fails verification is withheld rather than shown with a weak warning.
- Sidecar unavailability preserves the last bounded page state where applicable and explains staleness without exposing infrastructure diagnostics.
- Keyboard operation, narrow layouts, bounded tables, one active mutation, and explicit destructive confirmation carry over from the Hyphae TUI's safety model.

## Visual direction

Use a dense research-instrument visual language rather than a generic administration template: high-contrast document surfaces, monospaced identifiers, restrained status color, visible provenance anchors, and compact evidence panels. The layout must collapse to one column on mobile without horizontal page scrolling. Hyphae trademark assets are not copied unless their separate asset terms and intended use are confirmed.

## Explicit exclusions

The adaptation does not include arbitrary SQL, administration, doctor, telemetry, ANN tuning, backup controls, chat, model selection, an embedder, user-directory management, OIDC, or an OpenAI-compatible proxy. Those surfaces are outside the pinned workplan even though some appear in the upstream TUI.

## Delivery order

1. Complete adapter WP1-WP6.
2. Cut the fixture over in WP7 and replace the counter-only markup with the Overview shell while preserving simulated authentication, no-script operation, tenant isolation, and browser-isolation tests.
3. Expose Catalog and Ingest only after the application types, transactions, and `pliego-data` action are real.
4. Expose Search and Verify only after exact `v1.0.1` codecs and live proof tests pass.
5. Add mobile, keyboard, hostile-response, stale-state, and lost-ack browser tests in WP8 before promoting the fixture.
