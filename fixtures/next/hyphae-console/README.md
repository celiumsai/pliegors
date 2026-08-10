# Reduced Hyphae Console Next fixture

This fixture is specified against Hyphae Native `v1.0.1` at commit
`84161cf067141b60f4847b965ef77c5b749749c0`. That release closed the bounded G6
functional profile for the embedded product, local daemon, HTTP `/v2`, and
cross-surface conformance. The fixture does not freeze the older PliegoRS
append/page seam as the Console architecture.

Acceptance requires navigation, simulated authentication, dynamic and streamed
SSR, bounded queries, optional PliegoCSS output, and persistent state with
verified replay. No production Hyphae gateway claim is implied.

The selected boundary is a separately installed and verified Hyphae sidecar
with HTTP `/v2` bound to loopback. PliegoRS owns the server-side adapter,
authentication policy, process supervision, request bounds, and translation to
application types. Browsers never receive Hyphae credentials or call the
sidecar directly. No `hyphae-*` crate enters the PliegoRS Cargo graph, so
Hyphae's Rust `1.89` floor does not raise the PliegoRS `1.86` MSRV.

The `v1.0.1` sidecar is the frozen learning target, not an accepted production
dependency. HTTP `/v2` still identifies itself as `2.0.0-alpha`, and the formal
Hyphae source status leaves G7 and G8 open. G7 blocks performance and scale
claims. G8 blocks final fixture acceptance until Hyphae records durable
source-level closure. The pending Native V3/G7 work does not change the `/v2`
route set or local protocol, so it does not block adapter design.

The fixture becomes executable only after the PliegoRS adapter verifies the
exact sidecar release, probes capabilities, enforces loopback, handles unknown
error codes, and passes bounded startup, shutdown, cancellation, persistence,
and browser-isolation tests. A later released Hyphae version requires a fresh
compatibility decision and new fixture identity.
