# G1 OpenTelemetry foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** In progress; operator-enabled request traces and HTTP metrics

**Base revision:** `963513c4db743a945cbf96a2d953f5d9a0979abf`

**Toolchain:** Rust and Cargo 1.86.0 on Windows x86-64 and Debian/WSL2 x86-64

This is a historical foundation snapshot. G1 was later completed and promoted
through the [`0.1.0-preview.1` transport evidence](g1-transport-load-security.md)
and public component prerelease.

## Contract under test

At this base revision, the then-unreleased native runtime remained silent by default. Calling
`NativeRuntimeBuilder::open_telemetry` captures global OpenTelemetry providers
that the operator configured before runtime construction. PliegoRS does not
select an exporter, collector, endpoint, credentials, processor, sampling
policy, or retention policy.

An enabled request produces one `SERVER` span from pre-admission through the
last response-body frame and three standard instruments:

- `http.server.request.duration`;
- `http.server.active_requests`; and
- `http.server.response.body.size`.

The attribute contract admits only the exact known-method set, operator-trusted
HTTP scheme, protocol version, sealed route template and ID, HTTP status, response size, render mode,
runtime outcome, receipt contract, and finite framework error-code allowlist.
Unknown methods and application-defined diagnostic codes map to `_OTHER`.
Concrete paths, query strings, addresses, headers, cookies, request/response
bodies, user/request/deployment identifiers, and diagnostic messages are not
read into telemetry. The privacy profile deliberately omits `url.path` rather
than publishing a route template with incorrect semantics.

Remote `traceparent` is ignored by default. The explicit
`RemoteTracePolicy::AcceptW3c` policy uses the W3C propagator for that parent
alone. Inbound `tracestate`, baggage, and alternate propagation formats are
discarded so a peer cannot inject provider state into exported telemetry.
Runtime receipts remain independent from exporters and record a coarse
duration bucket.

## Reproduction

```powershell
cargo test -p pliego-runtime --lib --locked
cargo test -p pliego-runtime --test native_runtime --locked
cargo test -p pliego-runtime --test opentelemetry --locked
cargo clippy -p pliego-runtime --all-targets --locked -- -D warnings
cargo doc -p pliego-runtime --no-deps --locked
```

Linux cross-check:

```sh
CARGO_TARGET_DIR=/tmp/pliegors-otel-target \
  cargo +1.86.0 test -p pliego-runtime --test opentelemetry --locked
```

Observed targeted result:

- 21 runtime unit tests passed;
- 27 native lifecycle integration tests passed;
- the in-memory OpenTelemetry integration test passed on Debian/WSL2;
- the full workspace passed `cargo test --workspace --all-targets --locked`
  on Debian/WSL2;
- runtime Clippy and Rustdoc passed with warnings denied; and
- `cargo audit` found no vulnerability and retained the existing allowed
  `RUSTSEC-2026-0173` unmaintained warning through `rstml`.

The in-memory SDK test sends three instrumented requests and one request through
an uninstrumented builder. Their dynamic path parameters, queries,
authorization, cookies, private headers, and bodies contain unique sentinels.
It proves:

- the uninstrumented request emits nothing and the three span names remain
  `POST /users/:id`, never the concrete path;
- exactly one span accepts the known remote parent: the default policy ignores
  the same header and explicit W3C mode rejects a malformed header;
- every span contains the exact ten-key allowlist and no diagnostic events;
- every span and metric series carries the operator-trusted `https` scheme,
  and the duration histogram exposes the recommended HTTP boundaries;
- three instrumented dynamic parameter values collapse to one time series per
  instrument, with exactly three observations per histogram;
- active-request increment and decrement use the same attributes and return the
  exported sum to zero; and
- no sentinel, including an inbound `tracestate` value, appears anywhere in
  exported spans or metrics.

The signal names, units, required scheme/method attributes, route semantics,
unknown-method naming, status handling, and duration boundaries were checked
against the official OpenTelemetry 1.43.0
[HTTP metrics](https://opentelemetry.io/docs/specs/semconv/http/http-metrics/)
and [HTTP spans](https://opentelemetry.io/docs/specs/semconv/http/http-spans/)
contracts. The documented `url.path` omission remains PliegoRS's deliberate
privacy deviation from the server-span attribute set.

An earlier exporter-test revision passed on Windows before Application Control
denied a subsequently recompiled test executable. The final strengthened test,
full workspace suite, and `cargo fmt --check` passed under Debian/WSL2 with
Rust 1.86.0. This host-policy limitation is recorded rather than converted
into a runtime exception; the protected GitHub Linux workflow remains the
merge authority.

## Evidence boundary

This slice does not provide an OTLP exporter, collector deployment, structured
runtime logs, raw concrete-path telemetry, HTTP/2 correlation, fixed-load
cardinality/RSS measurements, or operator-specific sampling and retention
guidance. It does not close G1 or promote `production-observability` from
`not-released`.
