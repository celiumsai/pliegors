// SPDX-License-Identifier: Apache-2.0

use futures_util::stream;
use pliego_dom::{IntoView, el};
use pliego_router::{
    MiddlewareCapabilities, MiddlewareCapability, RouteGraphBuilder, RouteMethod, RouteScopeKind,
    RouteScopeSpec, RouteSpec,
};
use pliego_runtime::{
    AsyncBoundary, Body, BoundaryDocument, BoundaryRenderOptions, CompleteDocument,
    CompleteRenderOptions, DocumentHead, LayoutDocument, LayoutLayer, NativeRuntime,
    NativeRuntimeBuilder, OrderedDocument, OrderedRenderOptions, OrderedViewChunk, RequestContext,
    Response, StatusCode, render_boundary_document, render_complete_document,
    render_layout_document, render_ordered_document,
};
use std::error::Error;
use std::io;
use std::net::SocketAddr;

const CSS: &str = r#"
:root{font-family:Inter,ui-sans-serif,system-ui,sans-serif;color:#f3f2eb;background:#11120f;color-scheme:dark}
*{box-sizing:border-box}body{margin:0;min-width:20rem;background:#11120f}a{color:inherit}
main{width:min(72rem,calc(100% - 2rem));margin:0 auto;padding:clamp(3rem,8vw,8rem) 0}
.eyebrow{margin:0 0 1.25rem;color:#a8d087;font:700 .75rem/1.4 ui-monospace,monospace;text-transform:uppercase}
h1{max-width:12ch;margin:0;font-size:clamp(3rem,9vw,7.5rem);font-weight:620;line-height:.9;letter-spacing:0}
.lede{max-width:42rem;margin:2rem 0;color:#b9bcb2;font-size:clamp(1.05rem,2vw,1.35rem);line-height:1.6}
nav{display:flex;flex-wrap:wrap;gap:.75rem;margin-top:2.5rem}nav a{padding:.8rem 1rem;border:1px solid #3b3d36;text-decoration:none}
nav a:first-child{color:#11120f;background:#f3f2eb;border-color:#f3f2eb}
.masthead,.colophon{display:flex;justify-content:space-between;gap:1rem;width:min(72rem,calc(100% - 2rem));margin:0 auto;padding:1rem 0;color:#b9bcb2;font:700 .72rem/1.4 ui-monospace,monospace;text-transform:uppercase}
.masthead{border-bottom:1px solid #3b3d36}.colophon{border-top:1px solid #3b3d36}
.stream{display:grid;gap:1rem}.panel{padding:1.25rem;border:1px solid #3b3d36;background:#181a16}
.signal{color:#a8d087;font-family:ui-monospace,monospace}code{font-family:ui-monospace,monospace}
.error-code{display:inline-block;margin-top:1rem;padding:.35rem .5rem;color:#a8d087;border:1px solid #3b3d36;font-family:ui-monospace,monospace}
@media(prefers-reduced-motion:no-preference){nav a{transition:transform 160ms ease,border-color 160ms ease}nav a:hover{transform:translateY(-2px);border-color:#a8d087}}
"#;

type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

fn apply_response_policy(response: &mut Response<Body>) {
    response.headers_mut().insert(
        "x-content-type-options",
        "nosniff".parse().expect("static header is valid"),
    );
    response.headers_mut().insert(
        "referrer-policy",
        "no-referrer".parse().expect("static header is valid"),
    );
    response.headers_mut().insert(
        "content-security-policy",
        "default-src 'none'; style-src 'self'; base-uri 'none'; frame-ancestors 'none'"
            .parse()
            .expect("static header is valid"),
    );
}

fn route(
    id: &str,
    method: RouteMethod,
    pattern: &str,
) -> Result<RouteSpec, pliego_router::RouteError> {
    RouteSpec::new(id, method, pattern)
}

fn render_owned_page(
    context: &RequestContext,
    title: &str,
    description: &str,
    body: pliego_dom::View,
) -> Result<Response<Body>, pliego_runtime::HandlerError> {
    let layout = LayoutLayer::new("document-layout")?
        .before(
            el("header")
                .class("masthead")
                .child(el("span").child("PLIEGORS"))
                .child(el("span").child("NATIVE / G1")),
        )
        .after(
            el("footer")
                .class("colophon")
                .child(el("span").child("Explicit ownership"))
                .child(el("span").child("No JS shell")),
        )
        .wrap(el("div").class("document-frame"))
        .head(
            DocumentHead::new()
                .language("en")
                .stylesheet("/assets/site.css"),
        );
    let document = LayoutDocument::new(context.route(), body)
        .layout(layout)?
        .title(title)
        .description(description);
    render_layout_document(&document, CompleteRenderOptions::default())
}

pub fn build_runtime() -> AppResult<NativeRuntime> {
    let graph = RouteGraphBuilder::new()
        .declare_middleware(
            "canonical-entry",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::RewritePath),
        )?
        .declare_middleware(
            "response-policy",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::MutateResponseHeaders),
        )?
        .pre_route_middleware("canonical-entry")?
        .error_boundary("root-error")?
        .scope(
            RouteScopeSpec::new("site-group", RouteScopeKind::Group)?
                .middleware("response-policy")?,
        )
        .scope(
            RouteScopeSpec::new("document-layout", RouteScopeKind::Layout)?.parent("site-group")?,
        )
        .route(route("home", RouteMethod::get(), "/")?.scope("document-layout")?)
        .route(route("hello", RouteMethod::get(), "/hello/:name")?.scope("document-layout")?)
        .route(route("stream", RouteMethod::get(), "/stream")?.scope("site-group")?)
        .route(route("boundary", RouteMethod::get(), "/boundary")?.scope("site-group")?)
        .route(route("health", RouteMethod::get(), "/health")?.scope("site-group")?)
        .route(route("styles", RouteMethod::get(), "/assets/site.css")?.scope("site-group")?)
        .seal()?;

    let runtime = NativeRuntimeBuilder::new(graph, "native-pliego-preview")?
        .pre_route_middleware(
            "canonical-entry",
            MiddlewareCapabilities::none().allowing(MiddlewareCapability::RewritePath),
            |_context, mut request: pliego_runtime::Request<Body>, next: pliego_runtime::PreRouteNext| async move {
                if request.uri().path() == "/start" {
                    *request.uri_mut() = "/".parse().expect("static rewrite URI is valid");
                }
                next.run(request).await
            },
        )
        .middleware_with_capabilities(
            "response-policy",
            MiddlewareCapabilities::none()
                .allowing(MiddlewareCapability::MutateResponseHeaders),
            |_context, request, next: pliego_runtime::MiddlewareNext| async move {
                let mut response = next.run(request).await?;
                apply_response_policy(&mut response);
                Ok(response)
            },
        )
        .error_boundary(
            "root-error",
            |_context, error: pliego_runtime::PublicError| async move {
                let (title, message) = match error.class() {
                    pliego_runtime::PublicErrorClass::NotFound => (
                        "Page not found",
                        "The requested route is not part of this sealed application graph.",
                    ),
                    pliego_runtime::PublicErrorClass::UnauthorizedOrForbidden => (
                        "Access denied",
                        "This request does not have access to the selected resource.",
                    ),
                    pliego_runtime::PublicErrorClass::InvalidRequest => (
                        "Invalid request",
                        "The runtime rejected this request before application execution.",
                    ),
                    pliego_runtime::PublicErrorClass::InternalFailure => (
                        "Request failed",
                        "The runtime stopped the request and recorded a private diagnostic.",
                    ),
                };
                let body = el("main")
                    .child(el("p").class("eyebrow").child("PLIEGORS / SAFE FAILURE"))
                    .child(el("h1").child(title))
                    .child(el("p").class("lede").child(message))
                    .child(el("p").class("error-code").child(error.code().to_owned()))
                    .child(el("nav").child(el("a").attr("href", "/").child("Return home")))
                    .into_view();
                let document = CompleteDocument::new(title, body)
                    .language("en")
                    .stylesheet("/assets/site.css");
                let mut response = render_complete_document(
                    &document,
                    CompleteRenderOptions::default().status(error.status()),
                )?;
                apply_response_policy(&mut response);
                Ok(response)
            },
        )
        .handler("home", |context: RequestContext, _request| async move {
            let body = el("main")
                .child(el("p").class("eyebrow").child("PLIEGORS / NATIVE PREVIEW"))
                .child(el("h1").child("One runtime. Explicit ownership."))
                .child(
                    el("p").class("lede").child(
                        "This document crossed the portable router, bounded request lifecycle, Rust DOM renderer, and native HTTP host without a JavaScript application shell.",
                    ),
                )
                .child(
                    el("nav")
                        .attr("aria-label", "Runtime demonstrations")
                        .child(el("a").attr("href", "/stream").child("Inspect ordered SSR"))
                        .child(el("a").attr("href", "/boundary").child("Resolve async boundaries"))
                        .child(el("a").attr("href", "/hello/Pliego").child("Resolve a typed route"))
                        .child(el("a").attr("href", "/health").child("Read health")),
                )
                .into_view();
            render_owned_page(
                &context,
                "Native PliegoRS preview",
                "A dynamic reference application for the unreleased PliegoRS native runtime.",
                body,
            )
        })
        .handler("hello", |context: pliego_runtime::RequestContext, _request| async move {
            let name = context.parameter("name").unwrap_or("developer").to_owned();
            let body = el("main")
                .child(el("p").class("eyebrow").child("PORTABLE ROUTE MATCH"))
                .child(el("h1").child(format!("Hello, {name}.")))
                .child(
                    el("p")
                        .class("lede")
                        .child("The parameter was resolved by pliego-router and escaped by pliego-dom."),
                )
                .child(el("nav").child(el("a").attr("href", "/").child("Return home")))
                .into_view();
            render_owned_page(
                &context,
                "PliegoRS route",
                "A typed route rendered inside its sealed layout owner.",
                body,
            )
        })
        .handler("stream", |_context, _request| async {
            let document = OrderedDocument::new("PliegoRS ordered SSR")
                .language("en")
                .description("Sibling-granularity bounded server rendering.")
                .stylesheet("/assets/site.css");
            let chunks = stream::iter([
                OrderedViewChunk::new(|| {
                    el("main")
                        .class("stream")
                        .child(el("p").class("eyebrow").child("ORDERED SSR / 01"))
                        .child(el("h1").child("Backpressure is part of the contract."))
                        .into_view()
                }),
                OrderedViewChunk::new(|| {
                    el("section")
                        .class("panel")
                        .child(el("p").class("signal").child("FRAME 02 / RENDERED ON DEMAND"))
                        .child(el("p").child("Each sibling is constructed only when the body consumer polls its frame."))
                        .into_view()
                }),
                OrderedViewChunk::new(|| {
                    el("section")
                        .class("panel")
                        .child(el("p").class("signal").child("FRAME 03 / BOUNDED"))
                        .child(el("p").child("Shell, fragments, metadata, and total response bytes share explicit limits."))
                        .child(el("nav").child(el("a").attr("href", "/").child("Return home")))
                        .child(el("div").attr("aria-hidden", "true"))
                        .into_view()
                }),
            ]);
            render_ordered_document(&document, chunks, OrderedRenderOptions::default())
        })
        .handler("boundary", |_context, _request| async {
            let document = BoundaryDocument::new("PliegoRS async boundaries")
                .language("en")
                .description("Bounded concurrent work with document-order HTML delivery.")
                .stylesheet("/assets/site.css");
            let boundaries = [
                AsyncBoundary::map("intro", async { "ASYNC SSR / DECLARED" }, |label| {
                    el("main")
                        .class("stream")
                        .child(el("p").class("eyebrow").child(label))
                        .child(el("h1").child("Concurrency without changing the document."))
                        .into_view()
                })
                .expect("static boundary identity is valid"),
                AsyncBoundary::map(
                    "account-summary",
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(8)).await;
                        "RESOLVED / SERVER OWNED"
                    },
                    |label| {
                        el("section")
                            .class("panel")
                            .child(el("p").class("signal").child(label))
                            .child(el("p").child("The stable placeholder arrived before this bounded asynchronous result."))
                            .child(el("nav").child(el("a").attr("href", "/").child("Return home")))
                            .into_view()
                    },
                )
                .expect("static boundary identity is valid"),
            ];
            render_boundary_document(
                &document,
                boundaries,
                BoundaryRenderOptions::default(),
            )
        })
        .handler("health", |_context, _request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json; charset=utf-8")
                .header("cache-control", "no-store")
                .body(Body::from(r#"{"status":"ok","runtime":"pliegors-native-preview"}"#))
                .expect("static health response is valid"))
        })
        .handler("styles", |_context, _request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/css; charset=utf-8")
                .header("cache-control", "public, max-age=300")
                .body(Body::from(CSS))
                .expect("static stylesheet response is valid"))
        })
        .build()?;
    Ok(runtime)
}

pub fn configured_address() -> AppResult<SocketAddr> {
    let authored = std::env::var("PLIEGO_ADDR").unwrap_or_else(|_| "127.0.0.1:4310".to_owned());
    let address: SocketAddr = authored.parse()?;
    let expose = std::env::var("PLIEGO_EXPOSE").as_deref() == Ok("1");
    Ok(validate_bind_address(address, expose)?)
}

pub fn validate_bind_address(address: SocketAddr, expose: bool) -> io::Result<SocketAddr> {
    if !address.ip().is_loopback() && !expose {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "non-loopback PLIEGO_ADDR requires PLIEGO_EXPOSE=1",
        ));
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use pliego_runtime::{RenderMode, Request};
    use tower::ServiceExt;

    async fn response(target: &str) -> pliego_runtime::Response<pliego_runtime::Body> {
        build_runtime()
            .unwrap()
            .router()
            .oneshot(Request::builder().uri(target).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn complete_route_is_a_native_document() {
        let response = response("/").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.extensions().get::<RenderMode>(),
            Some(&RenderMode::Layout)
        );
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.starts_with("<!doctype html>"));
        assert!(body.contains("class=\"document-frame\""));
        assert!(body.contains("class=\"masthead\""));
        assert!(body.contains("One runtime. Explicit ownership."));
        assert!(body.contains("No JS shell"));
        assert!(body.ends_with("</body></html>"));
    }

    #[tokio::test]
    async fn pre_route_rewrite_reaches_the_canonical_entry() {
        let response = response("/start").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("One runtime. Explicit ownership.")
        );
    }

    #[tokio::test]
    async fn ordered_route_and_asset_are_served_by_the_same_runtime() {
        let streamed = response("/stream").await;
        assert_eq!(streamed.status(), StatusCode::OK);
        assert!(streamed.headers().get("content-length").is_none());
        let body = streamed.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("FRAME 02 / RENDERED ON DEMAND"));
        assert!(body.contains("FRAME 03 / BOUNDED"));

        let stylesheet = response("/assets/site.css").await;
        assert_eq!(
            stylesheet.headers()["content-type"],
            "text/css; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn boundary_route_resolves_without_a_javascript_runtime() {
        let streamed = response("/boundary").await;
        assert_eq!(streamed.status(), StatusCode::OK);
        assert!(streamed.headers().get("content-length").is_none());
        let body = streamed.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("data-pliego-boundary=\"intro\""));
        assert!(body.contains("data-pliego-boundary=\"account-summary\""));
        assert!(body.contains("RESOLVED / SERVER OWNED"));
        assert!(!body.contains("<script"));
    }

    #[tokio::test]
    async fn unknown_route_uses_the_authored_error_boundary() {
        let response = response("/missing").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.starts_with("<!doctype html>"));
        assert!(body.contains("Page not found"));
        assert!(body.contains("PLG-RTE-404"));
        assert!(!body.contains("route not found"));
    }

    #[test]
    fn bind_policy_is_loopback_by_default_and_explicit_for_lan() {
        let local: SocketAddr = "127.0.0.1:4310".parse().unwrap();
        let lan: SocketAddr = "0.0.0.0:4310".parse().unwrap();
        assert_eq!(validate_bind_address(local, false).unwrap(), local);
        assert_eq!(
            validate_bind_address(lan, false).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(validate_bind_address(lan, true).unwrap(), lan);
    }
}
