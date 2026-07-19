# G1 native socket foundation evidence

**Gate:** G1 Native runtime and dynamic rendering

**State:** HTTP/1.1 loopback conformance slice; gate remains open

**Toolchain:** Rust 1.86.0 under Debian WSL2

## Demonstrated path

The test corpus opens a real Tokio `TcpListener`, launches
`NativeRuntime::serve`, and sends raw HTTP/1.1 bytes over a loopback
`TcpStream`. This is transport evidence, not an in-process Tower
`ServiceExt::oneshot` substitute.

The corpus demonstrates that:

- a parameterized route receives a real socket request and returns a valid
  `200 OK` response with the expected body;
- the request registry returns to zero and emits one bounded receipt;
- graceful shutdown closes admission and completes a clean server stop;
- a pending streamed body is woken and cancelled by shutdown;
- registered application cleanup runs before the request leaves the registry;
  and
- the terminal receipt records `CancelReason::Shutdown`.

The shutdown case uses a 250 ms drain policy and a two-second external test
timeout. The observed run completed both socket tests in 0.02 seconds.

## Reproduction

From the repository root on Linux with Rust 1.86.0:

```bash
CARGO_TARGET_DIR=/tmp/pliegors-g1 cargo test \
  -p pliego-runtime --test native_socket --locked -- --nocapture
```

Observed result:

```text
running 2 tests
test serves_real_http11_request_and_shuts_down_cleanly ... ok
test shutdown_cancels_pending_socket_stream_and_runs_cleanup ... ok

test result: ok. 2 passed; 0 failed; 0 ignored
```

## Evidence boundary

This slice does not establish TLS, HTTP/2, proxy-header trust, public-network
behavior, slowloris resistance, slow-reader behavior, or fixed-load latency
and memory bounds. It does not close G1 or promote `native-http-runtime` from
`not-released`.
