// SPDX-License-Identifier: Apache-2.0

use crate::{STATIC_BODY, STREAM_BODY, route_graph};
use bytes::Bytes;
use futures_util::stream;
use pliego_runtime::{
    Body, HandlerError, NativeRuntime, NativeRuntimeBuilder, RequestContext, Response,
    RuntimeDiagnostic, StatusCode,
};
use std::sync::Arc;

type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn build_runtime() -> AppResult<NativeRuntime> {
    build_runtime_with_identity("unbound", &"0".repeat(64))
}

#[derive(Clone)]
struct ResponseIdentity {
    release_id: Arc<str>,
    pboc_sha256: Arc<str>,
}

pub fn build_runtime_with_identity(
    release_id: &str,
    pboc_sha256: &str,
) -> AppResult<NativeRuntime> {
    let identity = ResponseIdentity {
        release_id: Arc::from(release_id),
        pboc_sha256: Arc::from(pboc_sha256),
    };
    let home_identity = identity.clone();
    let hello_identity = identity.clone();
    let asset_identity = identity.clone();
    let health_identity = identity.clone();
    let stream_identity = identity;
    let runtime = NativeRuntimeBuilder::new(route_graph(), "provider-tck")?
        .handler("home", move |context: RequestContext, _request| {
            let identity = home_identity.clone();
            async move {
                response(
                    &context,
                    &identity,
                    "text/html; charset=utf-8",
                    "no-store",
                    Body::from(
                        "<!doctype html><title>PliegoRS Provider TCK</title><h1>same PBOC</h1>",
                    ),
                )
            }
        })
        .handler("hello", move |context: RequestContext, _request| {
            let identity = hello_identity.clone();
            async move {
                let name = context.parameter("name").unwrap_or("developer");
                response(
                    &context,
                    &identity,
                    "application/json; charset=utf-8",
                    "public, max-age=30, stale-while-revalidate=60",
                    Body::from(format!(
                        "{{\"hello\":\"{name}\",\"provider\":\"portable\"}}"
                    )),
                )
            }
        })
        .handler("asset", move |context: RequestContext, _request| {
            let identity = asset_identity.clone();
            async move {
                response(
                    &context,
                    &identity,
                    "text/plain; charset=utf-8",
                    "public, max-age=31536000, immutable",
                    Body::from(STATIC_BODY),
                )
            }
        })
        .handler("health", move |context: RequestContext, _request| {
            let identity = health_identity.clone();
            async move {
                response(
                    &context,
                    &identity,
                    "application/json; charset=utf-8",
                    "no-store",
                    Body::from("{\"status\":\"ok\",\"contract\":\"pboc\"}"),
                )
            }
        })
        .handler("stream", move |context: RequestContext, _request| {
            let identity = stream_identity.clone();
            async move {
                let chunks = STREAM_BODY
                    .lines()
                    .map(|line| Ok::<_, std::convert::Infallible>(Bytes::from(format!("{line}\n"))))
                    .collect::<Vec<_>>();
                response(
                    &context,
                    &identity,
                    "text/plain; charset=utf-8",
                    "no-store",
                    Body::from_stream(stream::iter(chunks)),
                )
            }
        })
        .build()?;
    Ok(runtime)
}

fn response(
    context: &RequestContext,
    identity: &ResponseIdentity,
    content_type: &str,
    cache_control: &str,
    body: Body,
) -> Result<Response<Body>, HandlerError> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("cache-control", cache_control)
        .header("x-content-type-options", "nosniff")
        .header("x-pliego-route", context.route().route_id())
        .header("x-pliego-release", identity.release_id.as_ref())
        .header("x-pliego-pboc", identity.pboc_sha256.as_ref())
        .body(body)
        .map_err(|error| {
            HandlerError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                RuntimeDiagnostic::new("PLG-TCK-500", error.to_string())
                    .expect("bounded response diagnostic is valid"),
            )
        })
}
