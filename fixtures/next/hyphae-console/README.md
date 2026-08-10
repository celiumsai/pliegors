# Reduced Hyphae Console Next fixture

This fixture is deliberately deferred while Hyphae Native closes G7 and G8 and
publishes `1.0.0`. It will target the released Native contracts rather than
freezing the older PliegoRS append/page seam as the Console architecture.

Acceptance requires navigation, simulated authentication, dynamic and streamed
SSR, bounded queries, optional PliegoCSS output, and persistent state with
verified replay. No production Hyphae gateway claim is implied.

Implementation starts only after the Native release identifies the exact
public integration surface: HTTP `/v2`, the local daemon protocol, an updated
`hyphae-pliegors` adapter, or an explicitly accepted embedded boundary. The
first preference remains a sidecar so Hyphae's Rust `1.89` floor does not
silently raise the PliegoRS `1.86` MSRV.
