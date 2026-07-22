// SPDX-License-Identifier: Apache-2.0

use pliego_pboc::{
    CacheDomain, CachePolicy, CacheRevalidation, FeatureRequirement, PbocFunction, PbocRoute,
    RenderMode, RouteKind, TelemetryHook, feature, runtime_contract_sha256_v1,
};
use pliego_router::{RouteGraph, RouteGraphBuilder, RouteMethod, RouteSpec};

pub const STATIC_BODY: &str = "PLIEGORS PBOC PROVIDER TCK\n";
pub const STATIC_HEADERS: &str = include_str!("../public/_headers");
pub const STREAM_BODY: &str = "frame-01\nframe-02\nframe-03\n";
pub const CLOUDFLARE_PBOC_WRAPPER: &str = r#"import Worker from "../index.js";
import pboc from "../../../pliego.pboc.json";
globalThis.__PLIEGO_PBOC_JSON = pboc;
export default Worker;
"#;

pub fn route_graph() -> RouteGraph {
    RouteGraphBuilder::new()
        .route(RouteSpec::new("home", RouteMethod::get(), "/").expect("static route is valid"))
        .route(
            RouteSpec::new("hello", RouteMethod::get(), "/api/hello/:name")
                .expect("static route is valid"),
        )
        .route(
            RouteSpec::new("asset", RouteMethod::get(), "/asset.txt")
                .expect("static route is valid"),
        )
        .route(
            RouteSpec::new("health", RouteMethod::get(), "/health").expect("static route is valid"),
        )
        .route(
            RouteSpec::new("stream", RouteMethod::get(), "/stream").expect("static route is valid"),
        )
        .seal()
        .expect("provider TCK route graph is valid")
}

pub fn route_graph_sha256() -> String {
    route_graph().digest().to_owned()
}

pub fn runtime_contract_sha256() -> String {
    runtime_contract_sha256_v1(&route_graph_sha256(), &[], &[], &[])
}

pub fn capabilities() -> Vec<FeatureRequirement> {
    vec![
        FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1),
        FeatureRequirement::required(feature::CACHE_PUBLIC, 1),
        FeatureRequirement::required(feature::DEPLOYMENT_ROLLBACK, 1),
        FeatureRequirement::required(feature::DEPLOYMENT_ROLLING, 1),
        FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
        FeatureRequirement::required(feature::HTTP_STREAM_ORDERED, 1),
        FeatureRequirement::required(feature::SECRETS_REFERENCES, 1),
        FeatureRequirement::required(feature::TELEMETRY_RECEIPTS, 1),
    ]
}

pub fn routes() -> Vec<PbocRoute> {
    vec![
        PbocRoute {
            id: "home".to_owned(),
            method: "GET".to_owned(),
            pattern: "/".to_owned(),
            kind: RouteKind::Dynamic,
            asset_path: None,
            function_id: Some("home".to_owned()),
            render_mode: RenderMode::Complete,
            cache_policy_id: None,
            required_features: vec![FeatureRequirement::required(feature::HTTP_COMPLETE, 1)],
        },
        PbocRoute {
            id: "hello".to_owned(),
            method: "GET".to_owned(),
            pattern: "/api/hello/:name".to_owned(),
            kind: RouteKind::Dynamic,
            asset_path: None,
            function_id: Some("hello".to_owned()),
            render_mode: RenderMode::Complete,
            cache_policy_id: Some("public-response".to_owned()),
            required_features: vec![FeatureRequirement::required(feature::HTTP_COMPLETE, 1)],
        },
        PbocRoute {
            id: "asset".to_owned(),
            method: "GET".to_owned(),
            pattern: "/asset.txt".to_owned(),
            kind: RouteKind::Static,
            asset_path: Some("public/asset.txt".to_owned()),
            function_id: None,
            render_mode: RenderMode::Complete,
            cache_policy_id: Some("public-immutable".to_owned()),
            required_features: vec![FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1)],
        },
        PbocRoute {
            id: "health".to_owned(),
            method: "GET".to_owned(),
            pattern: "/health".to_owned(),
            kind: RouteKind::Dynamic,
            asset_path: None,
            function_id: Some("health".to_owned()),
            render_mode: RenderMode::Resource,
            cache_policy_id: None,
            required_features: vec![FeatureRequirement::required(feature::HTTP_COMPLETE, 1)],
        },
        PbocRoute {
            id: "stream".to_owned(),
            method: "GET".to_owned(),
            pattern: "/stream".to_owned(),
            kind: RouteKind::Dynamic,
            asset_path: None,
            function_id: Some("stream".to_owned()),
            render_mode: RenderMode::Ordered,
            cache_policy_id: None,
            required_features: vec![FeatureRequirement::required(
                feature::HTTP_STREAM_ORDERED,
                1,
            )],
        },
    ]
}

pub fn functions() -> Vec<PbocFunction> {
    vec![
        function("health", RenderMode::Resource),
        function("hello", RenderMode::Complete),
        function("home", RenderMode::Complete),
        function("stream", RenderMode::Ordered),
    ]
}

fn function(id: &str, mode: RenderMode) -> PbocFunction {
    PbocFunction {
        id: id.to_owned(),
        entrypoint: id.to_owned(),
        render_modes: vec![mode],
        max_response_bytes: 65_536,
        secret_references: Vec::new(),
        permission_ids: Vec::new(),
    }
}

pub fn cache_policies() -> Vec<CachePolicy> {
    vec![
        CachePolicy {
            id: "public-immutable".to_owned(),
            domain: CacheDomain::Public,
            revalidation: CacheRevalidation::Immutable,
            max_age_seconds: Some(31_536_000),
            stale_while_revalidate_seconds: None,
            vary_headers: Vec::new(),
            tags: vec!["assets".to_owned()],
        },
        CachePolicy {
            id: "public-response".to_owned(),
            domain: CacheDomain::Public,
            revalidation: CacheRevalidation::TimeBound,
            max_age_seconds: Some(30),
            stale_while_revalidate_seconds: Some(60),
            vary_headers: vec!["accept-language".to_owned()],
            tags: vec!["hello".to_owned()],
        },
    ]
}

pub fn telemetry_hooks() -> Vec<TelemetryHook> {
    vec![TelemetryHook {
        id: "request-receipt".to_owned(),
        signal: "receipt".to_owned(),
        required: false,
        redacted_fields: vec!["authorization".to_owned(), "cookie".to_owned()],
    }]
}

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
mod cloudflare;
