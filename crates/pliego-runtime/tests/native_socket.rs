// SPDX-License-Identifier: Apache-2.0

use http::Response;
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteSpec};
use pliego_runtime::{
    Body, CancelReason, InMemoryReceiptSink, NativeRuntimeBuilder, RequestLimits,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
