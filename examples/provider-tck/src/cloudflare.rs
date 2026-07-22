// SPDX-License-Identifier: Apache-2.0

use crate::{STREAM_BODY, route_graph_sha256, runtime_contract_sha256};
use futures_util::stream;
use pliego_cloudflare::{Application, RequestContext};
use pliego_pboc::decode_manifest;
use worker::{Context, Env, Headers, Request, Response, Result, event};

#[event(fetch)]
pub async fn fetch(request: Request, env: Env, context: Context) -> Result<Response> {
    match dispatch(request, env, context).await {
        Ok(response) => Ok(response),
        Err(error) => {
            worker::console_error!("PLG-CF-500 {}", error);
            Response::error("PLG-CF-500\n", 500)
        }
    }
}

async fn dispatch(request: Request, env: Env, context: Context) -> Result<Response> {
    let source = manifest_source(&env)?;
    let manifest = decode_manifest(source.as_bytes())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let application = Application::new(
        manifest,
        &route_graph_sha256(),
        &runtime_contract_sha256(),
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(worker::Error::RustError)?
    .handler("health", |context, _request, _env, _execution| async move {
        response(
            &context,
            "application/json; charset=utf-8",
            "no-store",
            "{\"status\":\"ok\",\"contract\":\"pboc\"}",
        )
    })
    .handler(
        "hello",
        |context: RequestContext, _request, _env, _execution| async move {
            let name = context
                .parameters
                .get("name")
                .map(String::as_str)
                .unwrap_or("developer");
            response(
                &context,
                "application/json; charset=utf-8",
                "public, max-age=30, stale-while-revalidate=60",
                &format!("{{\"hello\":\"{name}\",\"provider\":\"portable\"}}"),
            )
        },
    )
    .handler("home", |context, _request, _env, _execution| async move {
        response(
            &context,
            "text/html; charset=utf-8",
            "no-store",
            "<!doctype html><title>PliegoRS Provider TCK</title><h1>same PBOC</h1>",
        )
    })
    .handler("stream", |context, _request, _env, _execution| async move {
        let frames = STREAM_BODY
            .lines()
            .map(|line| Ok::<_, worker::Error>(format!("{line}\n").into_bytes()))
            .collect::<Vec<_>>();
        let headers = headers(&context, "text/plain; charset=utf-8", "no-store")?;
        Ok(Response::from_stream(stream::iter(frames))?.with_headers(headers))
    })
    .seal()
    .map_err(worker::Error::RustError)?;
    application.dispatch(request, env, context).await
}

fn manifest_source(env: &Env) -> Result<String> {
    let global = js_sys::global();
    let key = worker::wasm_bindgen::JsValue::from_str("__PLIEGO_PBOC_JSON");
    if let Some(source) = js_sys::Reflect::get(&global, &key)
        .ok()
        .and_then(|value| value.as_string())
        .filter(|source| !source.is_empty())
    {
        return Ok(source);
    }
    env.var("PLIEGO_PBOC_JSON").map(|value| value.to_string())
}

fn response(
    context: &RequestContext,
    content_type: &str,
    cache_control: &str,
    body: &str,
) -> Result<Response> {
    Ok(Response::ok(body)?.with_headers(headers(context, content_type, cache_control)?))
}

fn headers(context: &RequestContext, content_type: &str, cache_control: &str) -> Result<Headers> {
    let headers = Headers::new();
    headers.set("content-type", content_type)?;
    headers.set("cache-control", cache_control)?;
    headers.set("x-content-type-options", "nosniff")?;
    headers.set("x-pliego-route", &context.route_id)?;
    headers.set("x-pliego-release", &context.release_id)?;
    headers.set("x-pliego-pboc", &context.manifest_sha256)?;
    Ok(headers)
}
