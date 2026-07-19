// SPDX-License-Identifier: Apache-2.0

use axum::body::to_bytes;
use http::{Request, Response, StatusCode};
use pliego_router::{RouteGraphBuilder, RouteMethod, RouteSpec};
use pliego_runtime::{Body, InMemoryReceiptSink, NativeRuntimeBuilder, RequestLimits};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tower::ServiceExt;

fn route(id: &str, method: RouteMethod, pattern: &str) -> RouteSpec {
    RouteSpec::new(id, method, pattern).unwrap()
}

#[tokio::test]
async fn dispatches_real_axum_request_and_records_receipt() {
    let graph = RouteGraphBuilder::new()
        .route(route("hello", RouteMethod::get(), "/hello/:name"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let cleanup = Arc::new(Mutex::new(Vec::new()));
    let runtime = NativeRuntimeBuilder::new(graph, "test-deployment")
        .unwrap()
        .handler("hello", {
            let cleanup = cleanup.clone();
            move |context: pliego_runtime::RequestContext, _request| {
                let cleanup = cleanup.clone();
                async move {
                    context
                        .scope()
                        .register_cleanup(move |_| {
                            cleanup.lock().unwrap().push("closed");
                            Ok(())
                        })
                        .unwrap();
                    Ok(Response::new(Body::from(format!(
                        "hello {}",
                        context.parameter("name").unwrap()
                    ))))
                }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/hello/Pliego")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "hello Pliego"
    );
    assert_eq!(*cleanup.lock().unwrap(), vec!["closed"]);
    let receipts = sink.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].route_id.as_deref(), Some("hello"));
    assert_eq!(receipts[0].response_status, Some(200));
    assert_eq!(receipts[0].response_bytes, 12);
}

#[tokio::test]
async fn rejects_oversized_declared_body_before_handler() {
    let graph = RouteGraphBuilder::new()
        .route(route("upload", RouteMethod::post(), "/upload"))
        .seal()
        .unwrap();
    let runtime = NativeRuntimeBuilder::new(graph, "body-limit")
        .unwrap()
        .limits(RequestLimits {
            max_body_bytes: 4,
            ..RequestLimits::default()
        })
        .unwrap()
        .handler("upload", |_context, _request| async {
            panic!("handler ran after failed body preflight");
            #[allow(unreachable_code)]
            Ok(Response::new(Body::empty()))
        })
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .header("content-length", "5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn reports_method_not_allowed_with_allow_header() {
    let graph = RouteGraphBuilder::new()
        .route(route("read", RouteMethod::get(), "/items"))
        .seal()
        .unwrap();
    let runtime = NativeRuntimeBuilder::new(graph, "method-test")
        .unwrap()
        .handler("read", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build()
        .unwrap();
    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[http::header::ALLOW], "GET");
}

#[tokio::test]
async fn deadline_cancels_handler_and_returns_gateway_timeout() {
    let graph = RouteGraphBuilder::new()
        .route(route("slow", RouteMethod::get(), "/slow"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "deadline-test")
        .unwrap()
        .limits(RequestLimits {
            deadline_ms: 15,
            ..RequestLimits::default()
        })
        .unwrap()
        .handler("slow", |_context, _request| async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(Response::new(Body::from("late")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();
    let response = runtime
        .router()
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let _ = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(sink.receipts().len(), 1);
}

#[test]
fn runtime_rejects_missing_or_unknown_handlers() {
    let graph = RouteGraphBuilder::new()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    assert!(
        NativeRuntimeBuilder::new(graph.clone(), "missing")
            .unwrap()
            .build()
            .is_err()
    );
    assert!(
        NativeRuntimeBuilder::new(graph, "unknown")
            .unwrap()
            .handler("home", |_context, _request| async {
                Ok(Response::new(Body::empty()))
            })
            .handler("ghost", |_context, _request| async {
                Ok(Response::new(Body::empty()))
            })
            .build()
            .is_err()
    );
}

#[tokio::test]
async fn isolates_handler_panics_and_records_failure() {
    let graph = RouteGraphBuilder::new()
        .route(route("panic", RouteMethod::get(), "/panic"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "panic-test")
        .unwrap()
        .handler("panic", |_context, _request| async move {
            panic!("handler panic must not escape the runtime");
            #[allow(unreachable_code)]
            Ok(Response::new(Body::empty()))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/panic")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "PLG-RUN-502\n"
    );
    let receipts = sink.receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].outcome, pliego_runtime::RequestOutcome::Failed);
    assert_eq!(receipts[0].diagnostics[0].code, "PLG-RUN-502");
}

#[tokio::test]
async fn rejects_overload_without_running_a_second_handler() {
    let graph = RouteGraphBuilder::new()
        .route(route("work", RouteMethod::get(), "/work"))
        .seal()
        .unwrap();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let runtime = NativeRuntimeBuilder::new(graph, "overload-test")
        .unwrap()
        .limits(RequestLimits {
            max_concurrent_requests: 1,
            ..RequestLimits::default()
        })
        .unwrap()
        .handler("work", {
            let started = started.clone();
            let release = release.clone();
            let calls = calls.clone();
            move |_context, _request| {
                let started = started.clone();
                let release = release.clone();
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    started.notify_one();
                    release.notified().await;
                    Ok(Response::new(Body::from("done")))
                }
            }
        })
        .build()
        .unwrap();

    let started_wait = started.notified();
    tokio::pin!(started_wait);
    let first_router = runtime.router();
    let first = tokio::spawn(async move {
        first_router
            .oneshot(Request::builder().uri("/work").body(Body::empty()).unwrap())
            .await
            .unwrap()
    });
    started_wait.await;
    assert_eq!(runtime.active_request_count(), 1);

    let second = runtime
        .router()
        .oneshot(Request::builder().uri("/work").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        to_bytes(second.into_body(), 1024).await.unwrap(),
        "PLG-RUN-107\n"
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 1);

    release.notify_one();
    let first = first.await.unwrap();
    assert_eq!(to_bytes(first.into_body(), 1024).await.unwrap(), "done");
    assert_eq!(runtime.active_request_count(), 0);
}

#[tokio::test]
async fn shutdown_wakes_pending_stream_and_rejects_new_requests() {
    let graph = RouteGraphBuilder::new()
        .route(route("stream", RouteMethod::get(), "/stream"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "shutdown-test")
        .unwrap()
        .handler("stream", |_context, _request| async move {
            let stream = futures_util::stream::pending::<Result<bytes::Bytes, std::io::Error>>();
            Ok(Response::new(Body::from_stream(stream)))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime.active_request_count(), 1);
    runtime.begin_shutdown();
    assert!(to_bytes(response.into_body(), 1024).await.is_err());
    assert_eq!(runtime.active_request_count(), 0);

    let rejected = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        to_bytes(rejected.into_body(), 1024).await.unwrap(),
        "PLG-RUN-503\n"
    );
    let receipts = sink.receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(
        receipts[0].cancel_reason,
        Some(pliego_runtime::CancelReason::Shutdown)
    );
}

#[tokio::test]
async fn dropping_response_body_is_a_client_disconnect() {
    let graph = RouteGraphBuilder::new()
        .route(route("drop", RouteMethod::get(), "/drop"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "disconnect-test")
        .unwrap()
        .handler("drop", |_context, _request| async move {
            Ok(Response::new(Body::from("unread")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(Request::builder().uri("/drop").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(runtime.active_request_count(), 1);
    drop(response);
    assert_eq!(runtime.active_request_count(), 0);
    assert_eq!(
        sink.receipts()[0].cancel_reason,
        Some(pliego_runtime::CancelReason::ClientDisconnect)
    );
}
