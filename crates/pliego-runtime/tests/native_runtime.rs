// SPDX-License-Identifier: Apache-2.0

use axum::body::to_bytes;
use http::{Request, Response, StatusCode};
use pliego_router::{
    MiddlewareCapabilities, MiddlewareCapability, RouteGraphBuilder, RouteMethod, RouteScopeKind,
    RouteScopeSpec, RouteSpec,
};
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
async fn middleware_unwinds_before_commit_and_records_entered_layers() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware(
            "outer",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
        )
        .unwrap()
        .declare_middleware(
            "inner",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
        )
        .unwrap()
        .route(
            route("home", RouteMethod::get(), "/")
                .middleware("outer")
                .unwrap()
                .middleware("inner")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "middleware-order")
        .unwrap()
        .middleware_with_capabilities(
            "outer",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
            {
                let order = order.clone();
                move |_context, request, next: pliego_runtime::MiddlewareNext| {
                    let order = order.clone();
                    async move {
                        order.lock().unwrap().push("outer-before");
                        let mut response = next.run(request).await?;
                        order.lock().unwrap().push("outer-after");
                        response
                            .headers_mut()
                            .insert("x-outer", http::HeaderValue::from_static("set"));
                        Ok(response)
                    }
                }
            },
        )
        .middleware_with_capabilities(
            "inner",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
            {
                let order = order.clone();
                move |_context, request, next: pliego_runtime::MiddlewareNext| {
                    let order = order.clone();
                    async move {
                        order.lock().unwrap().push("inner-before");
                        let mut response = next.run(request).await?;
                        order.lock().unwrap().push("inner-after");
                        response
                            .headers_mut()
                            .insert("x-inner", http::HeaderValue::from_static("set"));
                        Ok(response)
                    }
                }
            },
        )
        .handler("home", {
            let order = order.clone();
            move |_context, _request| {
                let order = order.clone();
                async move {
                    order.lock().unwrap().push("handler");
                    Ok(Response::new(Body::from("ok")))
                }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["x-outer"], "set");
    assert_eq!(response.headers()["x-inner"], "set");
    assert_eq!(to_bytes(response.into_body(), 16).await.unwrap(), "ok");
    assert_eq!(
        *order.lock().unwrap(),
        vec![
            "outer-before",
            "inner-before",
            "handler",
            "inner-after",
            "outer-after"
        ]
    );
    assert_eq!(
        sink.receipts()[0].middleware,
        vec!["outer".to_owned(), "inner".to_owned()]
    );
}

#[tokio::test]
async fn group_and_layout_middleware_and_errors_inherit_in_sealed_order() {
    let group = RouteScopeSpec::new("app-group", RouteScopeKind::Group)
        .unwrap()
        .middleware("group-policy")
        .unwrap();
    let layout = RouteScopeSpec::new("account-layout", RouteScopeKind::Layout)
        .unwrap()
        .parent("app-group")
        .unwrap()
        .middleware("layout-policy")
        .unwrap()
        .error_boundary("layout-error")
        .unwrap();
    let graph = RouteGraphBuilder::new()
        .declare_middleware("group-policy", MiddlewareCapabilities::none())
        .unwrap()
        .declare_middleware("layout-policy", MiddlewareCapabilities::none())
        .unwrap()
        .declare_middleware("route-policy", MiddlewareCapabilities::none())
        .unwrap()
        .scope(group)
        .scope(layout)
        .route(
            route("account", RouteMethod::get(), "/account")
                .scope("account-layout")
                .unwrap()
                .middleware("route-policy")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "scope-inheritance")
        .unwrap()
        .middleware("group-policy", {
            let order = order.clone();
            move |_context, request, next: pliego_runtime::MiddlewareNext| {
                let order = order.clone();
                async move {
                    order.lock().unwrap().push("group-before");
                    let response = next.run(request).await?;
                    order.lock().unwrap().push("group-after");
                    Ok(response)
                }
            }
        })
        .middleware("layout-policy", {
            let order = order.clone();
            move |_context, request, next: pliego_runtime::MiddlewareNext| {
                let order = order.clone();
                async move {
                    order.lock().unwrap().push("layout-before");
                    let response = next.run(request).await?;
                    order.lock().unwrap().push("layout-after");
                    Ok(response)
                }
            }
        })
        .middleware("route-policy", {
            let order = order.clone();
            move |_context, request, next: pliego_runtime::MiddlewareNext| {
                let order = order.clone();
                async move {
                    order.lock().unwrap().push("route-before");
                    let response = next.run(request).await?;
                    order.lock().unwrap().push("route-after");
                    Ok(response)
                }
            }
        })
        .error_boundary(
            "layout-error",
            |_context, error: pliego_runtime::PublicError| async move {
                Ok(Response::builder()
                    .status(error.status())
                    .body(Body::from("layout failure"))
                    .unwrap())
            },
        )
        .handler("account", {
            let order = order.clone();
            move |context: pliego_runtime::RequestContext, _request| {
                let order = order.clone();
                assert_eq!(
                    context.route().scope_ids(),
                    &["app-group".to_owned(), "account-layout".to_owned()]
                );
                async move {
                    order.lock().unwrap().push("handler");
                    Err(pliego_runtime::HandlerError::internal("private failure"))
                }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .uri("/account")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "layout failure"
    );
    assert_eq!(
        *order.lock().unwrap(),
        vec![
            "group-before",
            "layout-before",
            "route-before",
            "handler",
            "route-after",
            "layout-after",
            "group-after"
        ]
    );
    let receipt = &sink.receipts()[0];
    assert_eq!(
        receipt.route_scopes,
        vec!["app-group".to_owned(), "account-layout".to_owned()]
    );
    assert_eq!(receipt.error_boundary.as_deref(), Some("layout-error"));
}

#[tokio::test]
async fn middleware_can_short_circuit_without_entering_later_layers() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware(
            "guard",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::Reject),
        )
        .unwrap()
        .declare_middleware("never", MiddlewareCapabilities::none())
        .unwrap()
        .route(
            route("private", RouteMethod::get(), "/private")
                .middleware("guard")
                .unwrap()
                .middleware("never")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let handler_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "middleware-short-circuit")
        .unwrap()
        .middleware_with_capabilities(
            "guard",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::Reject),
            |_context, _request, _next| async {
                Ok(Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(Body::from("unauthorized"))
                    .unwrap())
            },
        )
        .middleware(
            "never",
            |_context, request, next: pliego_runtime::MiddlewareNext| async {
                next.run(request).await
            },
        )
        .handler("private", {
            let handler_ran = handler_ran.clone();
            move |_context, _request| {
                let handler_ran = handler_ran.clone();
                async move {
                    handler_ran.store(true, std::sync::atomic::Ordering::Release);
                    Ok(Response::new(Body::empty()))
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
                .uri("/private")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "unauthorized"
    );
    assert!(!handler_ran.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(sink.receipts()[0].middleware, vec!["guard".to_owned()]);
}

#[tokio::test]
async fn pre_route_middleware_rewrites_before_resolution_and_unwinds_before_commit() {
    let capabilities = MiddlewareCapabilities::none()
        .allowing(MiddlewareCapability::RewritePath)
        .allowing(MiddlewareCapability::MutateResponseHeaders);
    let graph = RouteGraphBuilder::new()
        .declare_middleware("canonicalize", capabilities.clone())
        .unwrap()
        .pre_route_middleware("canonicalize")
        .unwrap()
        .route(route("hello", RouteMethod::get(), "/hello"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "pre-route-rewrite")
        .unwrap()
        .pre_route_middleware(
            "canonicalize",
            capabilities,
            |_context, mut request: http::Request<Body>, next: pliego_runtime::PreRouteNext| async move {
                *request.uri_mut() = "/hello".parse().unwrap();
                let mut response = next.run(request).await?;
                response.headers_mut().insert(
                    "x-pre-route",
                    http::HeaderValue::from_static("canonicalized"),
                );
                Ok(response)
            },
        )
        .handler("hello", |context: pliego_runtime::RequestContext, request: http::Request<Body>| async move {
            assert_eq!(context.route().route_id(), "hello");
            assert_eq!(request.uri().path(), "/hello");
            Ok(Response::new(Body::from("rewritten")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .uri("/alias")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-pre-route"], "canonicalized");
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "rewritten"
    );
    let receipt = &sink.receipts()[0];
    assert_eq!(receipt.route_id.as_deref(), Some("hello"));
    assert_eq!(receipt.middleware, vec!["canonicalize".to_owned()]);
}

#[tokio::test]
async fn pre_route_middleware_can_redirect_without_resolving_a_route() {
    let capabilities = MiddlewareCapabilities::none().allowing(MiddlewareCapability::Redirect);
    let graph = RouteGraphBuilder::new()
        .declare_middleware("legacy-redirect", capabilities.clone())
        .unwrap()
        .pre_route_middleware("legacy-redirect")
        .unwrap()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let handler_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "pre-route-redirect")
        .unwrap()
        .pre_route_middleware(
            "legacy-redirect",
            capabilities,
            |_context, _request, _next| async {
                Ok(Response::builder()
                    .status(StatusCode::PERMANENT_REDIRECT)
                    .header(http::header::LOCATION, "/")
                    .body(Body::empty())
                    .unwrap())
            },
        )
        .handler("home", {
            let handler_ran = handler_ran.clone();
            move |_context, _request| {
                handler_ran.store(true, std::sync::atomic::Ordering::Release);
                async { Ok(Response::new(Body::empty())) }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .uri("/legacy")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(response.headers()[http::header::LOCATION], "/");
    to_bytes(response.into_body(), 1).await.unwrap();
    assert!(!handler_ran.load(std::sync::atomic::Ordering::Acquire));
    let receipt = &sink.receipts()[0];
    assert_eq!(receipt.route_id, None);
    assert_eq!(receipt.middleware, vec!["legacy-redirect".to_owned()]);
}

#[tokio::test]
async fn undeclared_pre_route_rewrite_fails_closed_before_resolution() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware("rewrite", MiddlewareCapabilities::none())
        .unwrap()
        .pre_route_middleware("rewrite")
        .unwrap()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let handler_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "undeclared-rewrite")
        .unwrap()
        .pre_route_middleware(
            "rewrite",
            MiddlewareCapabilities::none(),
            |_context, mut request: http::Request<Body>, next: pliego_runtime::PreRouteNext| async move {
                *request.uri_mut() = "/".parse().unwrap();
                next.run(request).await
            },
        )
        .handler("home", {
            let handler_ran = handler_ran.clone();
            move |_context, _request| {
                handler_ran.store(true, std::sync::atomic::Ordering::Release);
                async { Ok(Response::new(Body::empty())) }
            }
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .uri("/alias")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "PLG-RUN-507\n"
    );
    assert!(!handler_ran.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(sink.receipts()[0].diagnostics[0].code, "PLG-RUN-507");
}

#[tokio::test]
async fn undeclared_response_header_mutation_is_replaced_before_commit() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware("headers", MiddlewareCapabilities::none())
        .unwrap()
        .route(
            route("home", RouteMethod::get(), "/")
                .middleware("headers")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "undeclared-headers")
        .unwrap()
        .middleware(
            "headers",
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                let mut response = next.run(request).await?;
                response
                    .headers_mut()
                    .insert("x-hidden", http::HeaderValue::from_static("denied"));
                Ok(response)
            },
        )
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::from("must not escape")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .uri("/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.headers().get("x-hidden").is_none());
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "PLG-RUN-507\n"
    );
    assert_eq!(sink.receipts()[0].diagnostics[0].code, "PLG-RUN-507");
}

#[tokio::test]
async fn request_body_reads_require_the_sealed_capability() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware("body-reader", MiddlewareCapabilities::none())
        .unwrap()
        .route(
            route("submit", RouteMethod::post(), "/submit")
                .middleware("body-reader")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "undeclared-body-read")
        .unwrap()
        .middleware(
            "body-reader",
            |_context, request: http::Request<Body>, next: pliego_runtime::MiddlewareNext| async move {
                let (parts, body) = request.into_parts();
                let bytes = to_bytes(body, 64)
                    .await
                    .map_err(|_| pliego_runtime::HandlerError::internal("body read failed"))?;
                next.run(http::Request::from_parts(parts, Body::from(bytes)))
                    .await
            },
        )
        .handler("submit", |_context, _request| async {
            Ok(Response::new(Body::from("must not run")))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .method("POST")
                .uri("/submit")
                .body(Body::from("payload"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "PLG-RUN-507\n"
    );
    assert_eq!(sink.receipts()[0].diagnostics[0].code, "PLG-RUN-507");

    let capabilities = MiddlewareCapabilities::none().allowing(MiddlewareCapability::ReadBody);
    let graph = RouteGraphBuilder::new()
        .declare_middleware("body-reader", capabilities.clone())
        .unwrap()
        .route(
            route("submit", RouteMethod::post(), "/submit")
                .middleware("body-reader")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let runtime = NativeRuntimeBuilder::new(graph, "declared-body-read")
        .unwrap()
        .middleware_with_capabilities(
            "body-reader",
            capabilities,
            |_context, request: http::Request<Body>, next: pliego_runtime::MiddlewareNext| async move {
                let (parts, body) = request.into_parts();
                let bytes = to_bytes(body, 64)
                    .await
                    .map_err(|_| pliego_runtime::HandlerError::internal("body read failed"))?;
                next.run(http::Request::from_parts(parts, Body::from(bytes)))
                    .await
            },
        )
        .handler("submit", |_context, request: http::Request<Body>| async move {
            let body = to_bytes(request.into_body(), 64)
                .await
                .map_err(|_| pliego_runtime::HandlerError::internal("handler body read failed"))?;
            Ok(Response::new(Body::from(body)))
        })
        .build()
        .unwrap();
    let response = runtime
        .router()
        .oneshot(
            http::Request::builder()
                .method("POST")
                .uri("/submit")
                .body(Body::from("payload"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 32).await.unwrap(), "payload");
}

#[tokio::test]
async fn redirects_and_rejections_require_their_sealed_capabilities() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware("policy", MiddlewareCapabilities::none())
        .unwrap()
        .route(
            route("redirect", RouteMethod::get(), "/redirect")
                .middleware("policy")
                .unwrap(),
        )
        .route(
            route("reject", RouteMethod::get(), "/reject")
                .middleware("policy")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let runtime = NativeRuntimeBuilder::new(graph, "undeclared-status-effects")
        .unwrap()
        .middleware(
            "policy",
            |_context, request: http::Request<Body>, _next| async move {
                let status = if request.uri().path() == "/redirect" {
                    StatusCode::TEMPORARY_REDIRECT
                } else {
                    StatusCode::FORBIDDEN
                };
                Ok(Response::builder()
                    .status(status)
                    .body(Body::empty())
                    .unwrap())
            },
        )
        .handler("redirect", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .handler("reject", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build()
        .unwrap();

    for path in ["/redirect", "/reject"] {
        let response = runtime
            .router()
            .oneshot(
                http::Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            to_bytes(response.into_body(), 32).await.unwrap(),
            "PLG-RUN-507\n"
        );
    }
}

#[tokio::test]
async fn error_boundaries_walk_outward_without_receiving_internal_messages() {
    let graph = RouteGraphBuilder::new()
        .declare_middleware(
            "outer",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
        )
        .unwrap()
        .error_boundary("root-error")
        .unwrap()
        .route(
            route("fail", RouteMethod::get(), "/fail")
                .middleware("outer")
                .unwrap()
                .error_boundary("route-error")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "boundary-walk")
        .unwrap()
        .middleware_with_capabilities(
            "outer",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                let mut response = next.run(request).await?;
                response
                    .headers_mut()
                    .insert("x-error-policy", http::HeaderValue::from_static("applied"));
                Ok(response)
            },
        )
        .error_boundary("route-error", {
            let order = order.clone();
            move |_context, error: pliego_runtime::PublicError| {
                order.lock().unwrap().push("route");
                async move {
                    assert_eq!(
                        error.class(),
                        pliego_runtime::PublicErrorClass::InternalFailure
                    );
                    Ok(Response::new(Body::from("wrong status")))
                }
            }
        })
        .error_boundary("root-error", {
            let order = order.clone();
            move |context: pliego_runtime::ErrorBoundaryContext,
                  error: pliego_runtime::PublicError| {
                order.lock().unwrap().push("root");
                async move {
                    assert_eq!(context.route_id(), Some("fail"));
                    assert_eq!(error.code(), "PLG-RUN-500");
                    Ok(Response::builder()
                        .status(error.status())
                        .body(Body::from("public failure"))
                        .unwrap())
                }
            }
        })
        .handler("fail", |_context, _request| async {
            Err(pliego_runtime::HandlerError::internal(
                "database password must remain private",
            ))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(Request::builder().uri("/fail").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()["x-error-policy"], "applied");
    let body = to_bytes(response.into_body(), 64).await.unwrap();
    assert_eq!(body, "public failure");
    assert!(!String::from_utf8_lossy(&body).contains("password"));
    assert_eq!(*order.lock().unwrap(), vec!["route", "root"]);
    let receipt = &sink.receipts()[0];
    assert_eq!(receipt.error_boundary.as_deref(), Some("root-error"));
    assert_eq!(receipt.middleware, vec!["outer".to_owned()]);
    assert!(
        receipt
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PLG-RUN-505")
    );
}

#[tokio::test]
async fn boundary_errors_and_panics_reach_the_builtin_fallback_without_leaks() {
    let graph = RouteGraphBuilder::new()
        .error_boundary("failing-root")
        .unwrap()
        .route(
            route("fail", RouteMethod::get(), "/fail")
                .error_boundary("panicking-route")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "boundary-fallback")
        .unwrap()
        .error_boundary("panicking-route", |_context, _error| async move {
            panic!("boundary secret must not escape");
            #[allow(unreachable_code)]
            Ok(Response::new(Body::empty()))
        })
        .error_boundary("failing-root", |_context, _error| async move {
            Err(pliego_runtime::HandlerError::internal(
                "secondary secret must remain receipt-only",
            ))
        })
        .handler("fail", |_context, _request| async {
            Err(pliego_runtime::HandlerError::internal(
                "primary secret must remain receipt-only",
            ))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(Request::builder().uri("/fail").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), 64).await.unwrap();
    assert_eq!(body, "PLG-RUN-500\n");
    let public = String::from_utf8_lossy(&body);
    assert!(!public.contains("secret"));
    let receipt = &sink.receipts()[0];
    assert_eq!(receipt.error_boundary, None);
    assert!(
        receipt
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "PLG-RUN-504")
    );
}

#[tokio::test]
async fn root_boundary_handles_not_found_without_route_context() {
    let graph = RouteGraphBuilder::new()
        .error_boundary("not-found")
        .unwrap()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let runtime =
        NativeRuntimeBuilder::new(graph, "root-boundary")
            .unwrap()
            .error_boundary(
                "not-found",
                |context: pliego_runtime::ErrorBoundaryContext,
                 error: pliego_runtime::PublicError| async move {
                    assert_eq!(context.route_id(), None);
                    assert_eq!(error.class(), pliego_runtime::PublicErrorClass::NotFound);
                    Ok(Response::builder()
                        .status(error.status())
                        .body(Body::from("authored 404"))
                        .unwrap())
                },
            )
            .handler("home", |_context, _request| async {
                Ok(Response::new(Body::from("home")))
            })
            .build()
            .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        to_bytes(response.into_body(), 32).await.unwrap(),
        "authored 404"
    );
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

#[test]
fn runtime_rejects_incomplete_or_extra_behavior_registries() {
    let pre_route_graph = RouteGraphBuilder::new()
        .declare_middleware(
            "canonicalize",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::RewritePath),
        )
        .unwrap()
        .pre_route_middleware("canonicalize")
        .unwrap()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let missing_pre_route = NativeRuntimeBuilder::new(pre_route_graph, "missing-pre-route")
        .unwrap()
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        missing_pre_route,
        Err(pliego_runtime::RuntimeBuildError::MissingPreRouteMiddleware(id))
            if id == "canonicalize"
    ));

    let middleware_graph = RouteGraphBuilder::new()
        .declare_middleware("security", MiddlewareCapabilities::none())
        .unwrap()
        .route(
            route("home", RouteMethod::get(), "/")
                .middleware("security")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let missing = NativeRuntimeBuilder::new(middleware_graph, "missing-middleware")
        .unwrap()
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        missing,
        Err(pliego_runtime::RuntimeBuildError::MissingMiddleware(id)) if id == "security"
    ));

    let capability_graph = RouteGraphBuilder::new()
        .declare_middleware(
            "security",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::Reject),
        )
        .unwrap()
        .route(
            route("home", RouteMethod::get(), "/")
                .middleware("security")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let mismatch = NativeRuntimeBuilder::new(capability_graph, "capability-mismatch")
        .unwrap()
        .middleware(
            "security",
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                next.run(request).await
            },
        )
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        mismatch,
        Err(pliego_runtime::RuntimeBuildError::MiddlewareCapabilityMismatch { id, .. })
            if id == "security"
    ));

    let plain_graph = RouteGraphBuilder::new()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let extra = NativeRuntimeBuilder::new(plain_graph, "extra-middleware")
        .unwrap()
        .middleware(
            "ghost",
            |_context, request, next: pliego_runtime::MiddlewareNext| async {
                next.run(request).await
            },
        )
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        extra,
        Err(pliego_runtime::RuntimeBuildError::UnknownMiddleware(id)) if id == "ghost"
    ));

    let boundary_graph = RouteGraphBuilder::new()
        .error_boundary("root-error")
        .unwrap()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let missing = NativeRuntimeBuilder::new(boundary_graph, "missing-boundary")
        .unwrap()
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        missing,
        Err(pliego_runtime::RuntimeBuildError::MissingErrorBoundary(id)) if id == "root-error"
    ));

    let duplicate_middleware_graph = RouteGraphBuilder::new()
        .declare_middleware("security", MiddlewareCapabilities::none())
        .unwrap()
        .route(
            route("home", RouteMethod::get(), "/")
                .middleware("security")
                .unwrap(),
        )
        .seal()
        .unwrap();
    let duplicate = NativeRuntimeBuilder::new(duplicate_middleware_graph, "duplicate-middleware")
        .unwrap()
        .middleware(
            "security",
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                next.run(request).await
            },
        )
        .middleware(
            "security",
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                next.run(request).await
            },
        )
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        duplicate,
        Err(pliego_runtime::RuntimeBuildError::DuplicateMiddlewareRegistration(id))
            if id == "security"
    ));

    let duplicate_boundary_graph = RouteGraphBuilder::new()
        .error_boundary("root-error")
        .unwrap()
        .route(route("home", RouteMethod::get(), "/"))
        .seal()
        .unwrap();
    let duplicate = NativeRuntimeBuilder::new(duplicate_boundary_graph, "duplicate-boundary")
        .unwrap()
        .error_boundary("root-error", |_context, _error| async {
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap())
        })
        .error_boundary("root-error", |_context, _error| async {
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap())
        })
        .handler("home", |_context, _request| async {
            Ok(Response::new(Body::empty()))
        })
        .build();
    assert!(matches!(
        duplicate,
        Err(pliego_runtime::RuntimeBuildError::DuplicateErrorBoundaryRegistration(id))
            if id == "root-error"
    ));
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

#[tokio::test]
async fn complete_server_render_is_escaped_and_bound_to_the_receipt() {
    use pliego_dom::{IntoView, el};
    use pliego_runtime::{
        CompleteDocument, CompleteRenderOptions, RenderMode, render_complete_document,
    };

    let graph = RouteGraphBuilder::new()
        .route(route("document", RouteMethod::get(), "/document"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "render-test")
        .unwrap()
        .handler("document", |_context, _request| {
            let document = CompleteDocument::new(
                "Pliego & Rust",
                el("main").child("<trusted by construction>").into_view(),
            );
            std::future::ready(render_complete_document(
                &document,
                CompleteRenderOptions::default(),
            ))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/document")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(body.starts_with("<!doctype html><html lang=\"en\">"));
    assert!(body.contains("Pliego &amp; Rust"));
    assert!(body.contains("&lt;trusted by construction&gt;"));
    assert_eq!(sink.receipts()[0].render_mode, Some(RenderMode::Complete));
}

#[tokio::test]
async fn ordered_server_render_streams_siblings_and_binds_the_receipt() {
    use pliego_dom::{IntoView, el};
    use pliego_runtime::{
        OrderedDocument, OrderedRenderOptions, OrderedViewChunk, RenderMode,
        render_ordered_document,
    };

    let graph = RouteGraphBuilder::new()
        .route(route("ordered", RouteMethod::get(), "/ordered"))
        .seal()
        .unwrap();
    let sink = InMemoryReceiptSink::default();
    let runtime = NativeRuntimeBuilder::new(graph, "ordered-render-test")
        .unwrap()
        .handler("ordered", |_context, _request| {
            let document = OrderedDocument::new("Ordered");
            let chunks = futures_util::stream::iter([
                OrderedViewChunk::new(|| el("h1").child("First").into_view()),
                OrderedViewChunk::new(|| el("p").child("Second").into_view()),
            ]);
            std::future::ready(render_ordered_document(
                &document,
                chunks,
                OrderedRenderOptions::default(),
            ))
        })
        .receipt_sink(sink.clone())
        .build()
        .unwrap();

    let response = runtime
        .router()
        .oneshot(
            Request::builder()
                .uri("/ordered")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response
            .headers()
            .get(http::header::CONTENT_LENGTH)
            .is_none()
    );
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(body.contains("</head><body><h1>First</h1><p>Second</p></body></html>"));
    assert_eq!(sink.receipts()[0].render_mode, Some(RenderMode::Ordered));
}
