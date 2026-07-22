// SPDX-License-Identifier: Apache-2.0

use http::Response;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http2;
use hyper_util::rt::{TokioExecutor, TokioIo};
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteSpec};
use pliego_runtime::{
    Body, CancelReason, InMemoryReceiptSink, NativeRuntime, NativeRuntimeBuilder, RequestLimits,
    TransportLimits,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

fn route(id: &str, method: RouteMethod, pattern: &str) -> RouteSpec {
    RouteSpec::new(id, method, pattern).unwrap()
}

async fn connect_and_send(address: std::net::SocketAddr, target: &str) -> TcpStream {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    stream
}

async fn read_headers(stream: &mut TcpStream) -> Vec<u8> {
    let mut response = Vec::new();
    let mut buffer = [0_u8; 512];
    while !response.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
            .await
            .expect("server did not produce HTTP headers")
            .unwrap();
        assert!(read > 0, "connection closed before HTTP headers");
        response.extend_from_slice(&buffer[..read]);
    }
    response
}

async fn send_raw(address: std::net::SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut response))
        .await
        .expect("raw request did not terminate");
    response
}

async fn wait_for(
    maximum: Duration,
    mut condition: impl FnMut() -> bool,
) -> Result<(), &'static str> {
    tokio::time::timeout(maximum, async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| "condition timed out")
}

async fn spawn_server(
    runtime: NativeRuntime,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<std::io::Result<()>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        runtime
            .serve(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    (address, shutdown_tx, server)
}

#[tokio::test]
async fn serves_real_http11_request_and_shuts_down_cleanly() {
    let graph = RouteGraphBuilder::new()
        .route(route("hello", RouteMethod::get(), "/hello/:name"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "socket-test")
        .unwrap()
        .handler(
            "hello",
            |context: pliego_runtime::RequestContext, _request| async move {
                Ok(Response::new(Body::from(format!(
                    "hello {}",
                    context.parameter("name").unwrap()
                ))))
            },
        )
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_runtime = runtime.clone();
    let server = tokio::spawn(async move {
        server_runtime
            .serve(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let mut stream = connect_and_send(address, "/hello/socket").await;
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .expect("HTTP/1.1 response timed out")
        .unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.ends_with("hello socket"));
    assert_eq!(runtime.active_request_count(), 0);
    assert_eq!(sink.receipts().len(), 1);

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("server did not stop")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn shutdown_cancels_pending_socket_stream_and_runs_cleanup() {
    let graph = RouteGraphBuilder::new()
        .route(route("pending", RouteMethod::get(), "/pending"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let cleaned = Arc::new(AtomicBool::new(false));
    let runtime = NativeRuntimeBuilder::new(graph, "socket-shutdown-test")
        .unwrap()
        .limits(RequestLimits {
            graceful_shutdown_ms: 250,
            ..RequestLimits::default()
        })
        .unwrap()
        .handler("pending", {
            let cleaned = cleaned.clone();
            move |context: pliego_runtime::RequestContext, _request| {
                let cleaned = cleaned.clone();
                async move {
                    context
                        .scope()
                        .register_cleanup(move |_| {
                            cleaned.store(true, Ordering::Release);
                            Ok(())
                        })
                        .unwrap();
                    let stream =
                        futures_util::stream::pending::<Result<bytes::Bytes, std::io::Error>>();
                    Ok(Response::new(Body::from_stream(stream)))
                }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_runtime = runtime.clone();
    let server = tokio::spawn(async move {
        server_runtime
            .serve(listener, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let mut stream = connect_and_send(address, "/pending").await;
    let headers = read_headers(&mut stream).await;
    assert!(headers.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert_eq!(runtime.active_request_count(), 1);
    shutdown_tx.send(()).unwrap();

    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("graceful shutdown did not finish")
        .unwrap()
        .unwrap();
    assert!(cleaned.load(Ordering::Acquire));
    assert_eq!(runtime.active_request_count(), 0);
    let receipts = sink.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].cancel_reason, Some(CancelReason::Shutdown));
}

#[tokio::test]
async fn serves_real_http2_prior_knowledge_connection() {
    let graph = RouteGraphBuilder::new()
        .route(route("hello", RouteMethod::get(), "/hello/:name"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "http2-socket-test")
        .unwrap()
        .handler(
            "hello",
            |context: pliego_runtime::RequestContext, _request| async move {
                Ok(Response::new(Body::from(format!(
                    "hello {}",
                    context.parameter("name").unwrap()
                ))))
            },
        )
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let server_runtime = runtime.clone();
    let (address, shutdown_tx, server) = spawn_server(server_runtime).await;
    let stream = TcpStream::connect(address).await.unwrap();
    let (mut sender, connection) = http2::Builder::new(TokioExecutor::new())
        .handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let client = tokio::spawn(connection);
    let response = sender
        .send_request(
            http::Request::builder()
                .version(http::Version::HTTP_2)
                .uri("http://localhost/hello/h2")
                .body(Empty::<bytes::Bytes>::new())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.version(), http::Version::HTTP_2);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, "hello h2");
    drop(sender);
    client.await.unwrap().unwrap();
    wait_for(Duration::from_secs(1), || {
        runtime.active_connection_count() == 0
    })
    .await
    .unwrap();
    assert_eq!(sink.receipts().len(), 1);

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn http2_multiplexing_preserves_global_request_admission() {
    let graph = RouteGraphBuilder::new()
        .route(route("work", RouteMethod::get(), "/work"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "http2-overload-test")
        .unwrap()
        .limits(RequestLimits {
            max_concurrent_requests: 4,
            ..RequestLimits::default()
        })
        .unwrap()
        .handler("work", |_context, _request| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(Response::new(Body::from("done")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let (address, shutdown_tx, server) = spawn_server(runtime.clone()).await;
    let stream = TcpStream::connect(address).await.unwrap();
    let (sender, connection) = http2::Builder::new(TokioExecutor::new())
        .handshake(TokioIo::new(stream))
        .await
        .unwrap();
    let client = tokio::spawn(connection);
    let mut requests = Vec::new();
    for _ in 0..16 {
        let mut sender = sender.clone();
        requests.push(tokio::spawn(async move {
            let response = sender
                .send_request(
                    http::Request::builder()
                        .version(http::Version::HTTP_2)
                        .uri("http://localhost/work")
                        .body(Empty::<bytes::Bytes>::new())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            response.into_body().collect().await.unwrap();
            status
        }));
    }
    drop(sender);
    let mut success = 0;
    let mut overloaded = 0;
    for request in requests {
        match request.await.unwrap() {
            http::StatusCode::OK => success += 1,
            http::StatusCode::SERVICE_UNAVAILABLE => overloaded += 1,
            status => panic!("unexpected overload status {status}"),
        }
    }
    assert_eq!(success, 4);
    assert_eq!(overloaded, 12);
    client.await.unwrap().unwrap();
    assert_eq!(runtime.active_request_count(), 0);
    assert_eq!(sink.receipts().len(), 16);

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn absolute_http1_header_deadline_stops_trickling_peer() {
    let graph = RouteGraphBuilder::new()
        .route(route("hello", RouteMethod::get(), "/hello"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "slow-head-test")
        .unwrap()
        .transport_limits(TransportLimits {
            http1_header_read_timeout_ms: 60,
            read_idle_timeout_ms: 500,
            write_idle_timeout_ms: 500,
            ..TransportLimits::default()
        })
        .unwrap()
        .handler("hello", |_context, _request| async {
            Ok(Response::new(Body::from("never")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let server_runtime = runtime.clone();
    let (address, shutdown_tx, server) = spawn_server(server_runtime).await;
    let mut stream = TcpStream::connect(address).await.unwrap();
    for chunk in [b"G".as_slice(), b"E", b"T", b" ", b"/"] {
        let _ = stream.write_all(chunk).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut response))
        .await
        .expect("slow request head was retained");
    wait_for(Duration::from_secs(1), || {
        runtime.active_connection_count() == 0
    })
    .await
    .unwrap();
    assert!(sink.receipts().is_empty());

    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn connection_admission_is_bounded_before_request_parsing() {
    let graph = RouteGraphBuilder::new()
        .route(route("hello", RouteMethod::get(), "/hello"))
        .seal()
        .unwrap();
    let runtime = NativeRuntimeBuilder::new(graph, "connection-cap-test")
        .unwrap()
        .transport_limits(TransportLimits {
            max_connections: 1,
            http1_header_read_timeout_ms: 500,
            read_idle_timeout_ms: 500,
            ..TransportLimits::default()
        })
        .unwrap()
        .handler("hello", |_context, _request| async {
            Ok(Response::new(Body::from("hello")))
        })
        .build()
        .unwrap();
    let server_runtime = runtime.clone();
    let (address, shutdown_tx, server) = spawn_server(server_runtime).await;
    let mut first = TcpStream::connect(address).await.unwrap();
    first.write_all(b"G").await.unwrap();
    wait_for(Duration::from_secs(1), || {
        runtime.active_connection_count() == 1
    })
    .await
    .unwrap();

    let mut second = TcpStream::connect(address).await.unwrap();
    let _ = second
        .write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await;
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(1), second.read_to_end(&mut response))
        .await
        .expect("rejected connection stayed open");
    wait_for(Duration::from_secs(1), || {
        runtime.rejected_connection_count() == 1
    })
    .await
    .unwrap();
    assert!(response.is_empty());

    drop(first);
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn stalled_response_reader_releases_request_and_connection() {
    let graph = RouteGraphBuilder::new()
        .route(route("large", RouteMethod::get(), "/large"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "slow-reader-test")
        .unwrap()
        .transport_limits(TransportLimits {
            http1_header_read_timeout_ms: 500,
            read_idle_timeout_ms: 2_000,
            write_idle_timeout_ms: 75,
            ..TransportLimits::default()
        })
        .unwrap()
        .handler("large", |_context, _request| async {
            let chunk = bytes::Bytes::from(vec![b'x'; 64 * 1_024]);
            let stream = futures_util::stream::iter(
                (0..256).map(move |_| Ok::<_, std::io::Error>(chunk.clone())),
            );
            Ok(Response::new(Body::from_stream(stream)))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let server_runtime = runtime.clone();
    let (address, shutdown_tx, server) = spawn_server(server_runtime).await;
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /large HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    wait_for(Duration::from_secs(1), || {
        runtime.active_request_count() == 1
    })
    .await
    .unwrap();
    wait_for(Duration::from_secs(3), || {
        runtime.active_request_count() == 0
    })
    .await
    .unwrap();
    wait_for(Duration::from_secs(1), || {
        runtime.active_connection_count() == 0
    })
    .await
    .unwrap();
    let receipts = sink.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0].cancel_reason,
        Some(CancelReason::ClientDisconnect)
    );

    drop(stream);
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn socket_security_corpus_bounds_ambiguous_or_unsupported_bodies() {
    let graph = RouteGraphBuilder::new()
        .route(route("upload", RouteMethod::post(), "/upload"))
        .seal()
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let saw_content_length = Arc::new(AtomicBool::new(false));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "socket-security-test")
        .unwrap()
        .handler("upload", {
            let calls = calls.clone();
            let saw_content_length = saw_content_length.clone();
            move |_context: pliego_runtime::RequestContext, request: http::Request<Body>| {
                calls.fetch_add(1, Ordering::AcqRel);
                saw_content_length.store(
                    request.headers().contains_key(http::header::CONTENT_LENGTH),
                    Ordering::Release,
                );
                async { Ok(Response::new(Body::from("unsafe"))) }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let (address, shutdown_tx, server) = spawn_server(runtime.clone()).await;

    let compressed = send_raw(
        address,
        b"POST /upload HTTP/1.1\r\nHost: localhost\r\nContent-Encoding: gzip\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata",
    )
    .await;
    assert!(compressed.starts_with(b"HTTP/1.1 415 Unsupported Media Type\r\n"));
    assert!(compressed.ends_with(b"PLG-RUN-108\n"));

    let multipart = send_raw(
        address,
        b"POST /upload HTTP/1.1\r\nHost: localhost\r\nContent-Type: multipart/form-data; boundary=x\r\nContent-Length: 3\r\nConnection: close\r\n\r\n--x",
    )
    .await;
    assert!(multipart.starts_with(b"HTTP/1.1 415 Unsupported Media Type\r\n"));
    assert!(multipart.ends_with(b"PLG-RUN-109\n"));

    let ambiguous = send_raw(
        address,
        b"POST /upload HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n",
    )
    .await;
    assert!(
        ambiguous.starts_with(b"HTTP/1.1 400 Bad Request\r\n"),
        "ambiguous framing response: {}",
        String::from_utf8_lossy(&ambiguous)
    );
    assert!(!saw_content_length.load(Ordering::Acquire));

    let mut oversized_head =
        b"POST /upload HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n".to_vec();
    for index in 0..101 {
        oversized_head.extend_from_slice(format!("X-Bounded-{index}: x\r\n").as_bytes());
    }
    oversized_head.extend_from_slice(b"Content-Length: 0\r\n\r\n");
    let oversized = send_raw(address, &oversized_head).await;
    assert!(oversized.starts_with(b"HTTP/1.1 431 Request Header Fields Too Large\r\n"));

    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert_eq!(sink.receipts().len(), 3);
    shutdown_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}
