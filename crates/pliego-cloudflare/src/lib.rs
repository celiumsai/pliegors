// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![forbid(unsafe_code)]

//! Cloudflare Workers adapter for the Pliego Build Output Contract.

use pliego_pboc::{FeatureRequirement, HostKind, HostProfile, feature};

pub const TARGET_ID: &str = "cloudflare-workers";

pub fn host_profile(host_version: impl Into<String>) -> HostProfile {
    HostProfile {
        host_id: "pliego.cloudflare".to_owned(),
        host_version: host_version.into(),
        target_id: TARGET_ID.to_owned(),
        host_kind: HostKind::CloudflareWorkers,
        features: vec![
            FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1),
            FeatureRequirement::required(feature::CACHE_PUBLIC, 1),
            FeatureRequirement::required(feature::DEPLOYMENT_ROLLBACK, 1),
            FeatureRequirement::required(feature::DEPLOYMENT_ROLLING, 1),
            FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
            FeatureRequirement::required(feature::HTTP_STREAM_ORDERED, 1),
            FeatureRequirement::required(feature::SECRETS_REFERENCES, 1),
            FeatureRequirement::required(feature::TELEMETRY_RECEIPTS, 1),
        ],
        max_artifact_bytes: 25 * 1024 * 1024,
        max_bundle_bytes: 100 * 1024 * 1024,
    }
}

#[cfg(target_arch = "wasm32")]
mod runtime {
    use pliego_pboc::{HostAdmission, PbocManifest, PbocRouteMatch, PbocRouter, RouteKind};
    use std::collections::{BTreeMap, BTreeSet};
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use worker::{Context, Env, Headers, Request, Response, Result};

    pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<Response>> + 'static>>;

    #[derive(Clone, Debug)]
    pub struct CloudflareRequestContext {
        pub manifest_sha256: String,
        pub release_id: String,
        pub route_id: String,
        pub parameters: BTreeMap<String, String>,
    }

    pub trait CloudflareHandler: 'static {
        fn call(
            &self,
            context: CloudflareRequestContext,
            request: Request,
            env: Env,
            execution: Context,
        ) -> HandlerFuture;
    }

    impl<F, Fut> CloudflareHandler for F
    where
        F: Fn(CloudflareRequestContext, Request, Env, Context) -> Fut + 'static,
        Fut: Future<Output = Result<Response>> + 'static,
    {
        fn call(
            &self,
            context: CloudflareRequestContext,
            request: Request,
            env: Env,
            execution: Context,
        ) -> HandlerFuture {
            Box::pin(self(context, request, env, execution))
        }
    }

    pub struct CloudflareApplication {
        manifest: PbocManifest,
        admission: HostAdmission,
        router: PbocRouter,
        handlers: BTreeMap<String, Rc<dyn CloudflareHandler>>,
        secret_bindings: BTreeMap<String, String>,
        asset_binding: String,
        sealed: bool,
    }

    impl CloudflareApplication {
        pub fn new(
            manifest: PbocManifest,
            route_graph_sha256: &str,
            runtime_contract_sha256: &str,
            host_version: impl Into<String>,
        ) -> std::result::Result<Self, String> {
            if manifest.build.route_graph_sha256 != route_graph_sha256 {
                return Err(format!(
                    "PLG-CF-005 route graph differs: manifest={} application={route_graph_sha256}",
                    manifest.build.route_graph_sha256
                ));
            }
            if manifest.build.runtime_contract_sha256 != runtime_contract_sha256 {
                return Err(format!(
                    "PLG-CF-006 runtime contract differs: manifest={} application={runtime_contract_sha256}",
                    manifest.build.runtime_contract_sha256
                ));
            }
            let admission = manifest
                .admit(&super::host_profile(host_version))
                .map_err(|error| error.to_string())?;
            let router = PbocRouter::new(&manifest).map_err(|error| error.to_string())?;
            Ok(Self {
                manifest,
                admission,
                router,
                handlers: BTreeMap::new(),
                secret_bindings: BTreeMap::new(),
                asset_binding: "ASSETS".to_owned(),
                sealed: false,
            })
        }

        pub fn handler<H>(mut self, function_id: impl Into<String>, handler: H) -> Self
        where
            H: CloudflareHandler,
        {
            self.sealed = false;
            self.handlers.insert(function_id.into(), Rc::new(handler));
            self
        }

        pub fn secret_binding(
            mut self,
            secret_reference: impl Into<String>,
            binding: impl Into<String>,
        ) -> Self {
            self.sealed = false;
            self.secret_bindings
                .insert(secret_reference.into(), binding.into());
            self
        }

        pub fn asset_binding(mut self, binding: impl Into<String>) -> Self {
            self.sealed = false;
            self.asset_binding = binding.into();
            self
        }

        pub fn seal(mut self) -> std::result::Result<Self, String> {
            self.validate_registration()?;
            self.sealed = true;
            Ok(self)
        }

        fn validate_registration(&self) -> std::result::Result<(), String> {
            let required: BTreeSet<_> = self
                .manifest
                .functions
                .iter()
                .map(|function| function.id.as_str())
                .collect();
            let registered: BTreeSet<_> = self.handlers.keys().map(String::as_str).collect();
            if required != registered {
                let missing = required
                    .difference(&registered)
                    .copied()
                    .collect::<Vec<_>>();
                let unknown = registered
                    .difference(&required)
                    .copied()
                    .collect::<Vec<_>>();
                return Err(format!(
                    "PLG-CF-002 function registry differs; missing=[{}] unknown=[{}]",
                    missing.join(","),
                    unknown.join(",")
                ));
            }
            let declared_secrets: BTreeSet<_> = self
                .manifest
                .secret_references
                .iter()
                .map(|secret| secret.id.as_str())
                .collect();
            let mapped_secrets: BTreeSet<_> =
                self.secret_bindings.keys().map(String::as_str).collect();
            let unknown_secrets = mapped_secrets
                .difference(&declared_secrets)
                .copied()
                .collect::<Vec<_>>();
            if !unknown_secrets.is_empty() {
                return Err(format!(
                    "PLG-CF-003 unknown secret binding maps=[{}]",
                    unknown_secrets.join(",")
                ));
            }
            for secret in &self.manifest.secret_references {
                if secret.required && !self.secret_bindings.contains_key(&secret.id) {
                    return Err(format!(
                        "PLG-CF-003 missing binding map for secret {}",
                        secret.id
                    ));
                }
            }
            if !is_binding_name(&self.asset_binding)
                || self
                    .secret_bindings
                    .values()
                    .any(|name| !is_binding_name(name))
            {
                return Err(
                    "PLG-CF-003 binding names must use Cloudflare identifier syntax".into(),
                );
            }
            Ok(())
        }

        pub fn admission(&self) -> &HostAdmission {
            &self.admission
        }

        pub async fn dispatch(
            &self,
            request: Request,
            env: Env,
            execution: Context,
        ) -> Result<Response> {
            if !self.sealed {
                return Response::error("PLG-CF-001 application registry is not sealed", 500);
            }
            let method = request.method().to_string();
            let path = request.path();
            let matched = match self.router.resolve(&method, &path) {
                Ok(matched) => matched,
                Err(error) => {
                    let headers = Headers::new();
                    headers.set("content-type", "text/plain; charset=utf-8")?;
                    if let Some(allow) = error.allow_header() {
                        headers.set("allow", &allow)?;
                    }
                    return Ok(Response::error(
                        format!("{}\n", error.code()),
                        error.status_code(),
                    )?
                    .with_headers(headers));
                }
            };
            match matched.route.kind {
                RouteKind::Static => self.static_asset(request, env).await,
                RouteKind::Dynamic => self.dynamic(matched, request, env, execution).await,
            }
        }

        async fn static_asset(&self, request: Request, env: Env) -> Result<Response> {
            env.assets(&self.asset_binding)?
                .fetch_request(request)
                .await
        }

        async fn dynamic(
            &self,
            matched: PbocRouteMatch,
            request: Request,
            env: Env,
            execution: Context,
        ) -> Result<Response> {
            let function_id = matched
                .route
                .function_id
                .as_deref()
                .expect("validated dynamic PBOC routes have a function");
            let function = self
                .manifest
                .functions
                .iter()
                .find(|function| function.id == function_id)
                .expect("validated PBOC functions are present");
            for secret in &function.secret_references {
                let Some(binding) = self.secret_bindings.get(secret) else {
                    return Response::error("PLG-CF-003 secret binding is not mapped", 500);
                };
                if env.secret(binding).is_err() {
                    return Response::error(
                        "PLG-CF-004 required secret binding is unavailable",
                        500,
                    );
                }
            }
            let handler = self
                .handlers
                .get(function_id)
                .expect("sealed Cloudflare registries are exact");
            handler
                .call(
                    CloudflareRequestContext {
                        manifest_sha256: self.admission.manifest_sha256.clone(),
                        release_id: self.manifest.build.release_id.clone(),
                        route_id: matched.route.id,
                        parameters: matched.parameters,
                    },
                    request,
                    env,
                    execution,
                )
                .await
        }
    }

    fn is_binding_name(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 128
            && value.bytes().enumerate().all(|(index, byte)| {
                byte == b'_'
                    || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit())
            })
    }

    pub use CloudflareApplication as Application;
    pub use CloudflareHandler as Handler;
    pub use CloudflareRequestContext as RequestContext;
}

#[cfg(target_arch = "wasm32")]
pub use runtime::{Application, Handler, HandlerFuture, RequestContext};
