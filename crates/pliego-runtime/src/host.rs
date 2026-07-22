// SPDX-License-Identifier: Apache-2.0

use crate::telemetry::OpenTelemetryRuntime;
use crate::transport::TimedIo;
use crate::{
    CancelReason, ErrorBoundaryContext, LimitError, MiddlewareNext, OpenTelemetryConfig,
    PreRouteContext, PreRouteNext, PublicError, PublicErrorClass, RequestContext, RequestIdentity,
    RequestLimits, RequestScope, RequestState, RuntimeDiagnostic, RuntimeErrorBoundary,
    RuntimeMiddleware, RuntimePreRouteMiddleware, RuntimeReceipt, RuntimeReceiptSink, ScopeError,
    TransportLimitError, TransportLimits,
};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use futures_util::FutureExt;
use http::{Request, Response, StatusCode};
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use pliego_data::{
    ActionPolicy, CachePolicy, DataCancelReason, DataContext, DataContextOptions, DataIdentity,
    DataPolicyGrants, DataRequestValues, LoaderPolicy, ResourceGrant, ResourceRegistry,
};
use pliego_router::{
    MiddlewareCapabilities, MiddlewareCapability, ResolveError, RouteGraph, RouteMatch, RouteMethod,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tracing::{debug, warn};

pub type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<Response<Body>, HandlerError>> + Send + 'static>>;

pub trait RuntimeHandler: Send + Sync + 'static {
    fn call(&self, context: RequestContext, request: Request<Body>) -> HandlerFuture;
}

impl<F, Fut> RuntimeHandler for F
where
    F: Fn(RequestContext, Request<Body>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response<Body>, HandlerError>> + Send + 'static,
{
    fn call(&self, context: RequestContext, request: Request<Body>) -> HandlerFuture {
        Box::pin(self(context, request))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HandlerError {
    status: StatusCode,
    diagnostic: RuntimeDiagnostic,
}

impl HandlerError {
    pub fn new(status: StatusCode, diagnostic: RuntimeDiagnostic) -> Self {
        Self { status, diagnostic }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        let message = bounded(&message.into(), 320);
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            diagnostic: RuntimeDiagnostic::new("PLG-RUN-500", message)
                .expect("internal diagnostic is bounded"),
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn diagnostic(&self) -> &RuntimeDiagnostic {
        &self.diagnostic
    }
}

impl Display for HandlerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} {}",
            self.diagnostic.code, self.diagnostic.message
        )
    }
}

impl std::error::Error for HandlerError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeContractManifest {
    pub contract: String,
    pub application_contract_sha256: String,
    pub actions: Vec<ActionContractManifest>,
    pub loaders: Vec<LoaderContractManifest>,
    pub caches: Vec<CacheContractManifest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionContractManifest {
    pub id: String,
    pub semantic_revision: u32,
    pub contract_sha256: String,
    pub accepted_media_types: Vec<String>,
    pub accepted_content_encodings: Vec<String>,
    pub max_encoded_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_form_fields: usize,
    pub max_output_bytes: usize,
    pub origin_policy: String,
    pub csrf_policy: String,
    pub requires_authentication: bool,
    pub requires_authorization: bool,
    pub post_commit_grace_ms: u64,
    pub idempotency_policy_id: Option<String>,
    pub resources: Vec<ContractResourceRequirement>,
    pub invalidations: Vec<ActionInvalidationManifest>,
}

impl ActionContractManifest {
    pub fn explain(&self) -> String {
        let media = display_contract_items(&self.accepted_media_types);
        let encodings = display_contract_items(&self.accepted_content_encodings);
        let resources = self
            .resources
            .iter()
            .map(|resource| {
                format!(
                    "{}({})",
                    resource.id,
                    display_contract_items(&resource.capabilities)
                )
            })
            .collect::<Vec<_>>();
        let invalidations = self
            .invalidations
            .iter()
            .map(|intent| {
                format!(
                    "{}:{}({})",
                    intent.cache_policy_id,
                    intent.consistency,
                    display_contract_items(&intent.tags)
                )
            })
            .collect::<Vec<_>>();
        format!(
            "PLIEGO inspect action {}\nrevision: {}\ncontract: {}\nmedia: {}\ncontent encodings: {}\nlimits: encoded={} decoded={} form-fields={} output={}\nsecurity: origin={} csrf={} authentication={} authorization={}\nidempotency: {}\npost-commit grace: {} ms\nresources: {}\ninvalidations: {}",
            self.id,
            self.semantic_revision,
            self.contract_sha256,
            media,
            encodings,
            self.max_encoded_bytes,
            self.max_decoded_bytes,
            self.max_form_fields,
            self.max_output_bytes,
            self.origin_policy,
            self.csrf_policy,
            self.requires_authentication,
            self.requires_authorization,
            self.idempotency_policy_id.as_deref().unwrap_or("none"),
            self.post_commit_grace_ms,
            display_contract_items(&resources),
            display_contract_items(&invalidations),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoaderContractManifest {
    pub id: String,
    pub semantic_revision: u32,
    pub contract_sha256: String,
    pub cache_policy_id: Option<String>,
    pub max_output_bytes: usize,
    pub deduplicates_in_request: bool,
    pub resources: Vec<ContractResourceRequirement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheContractManifest {
    pub id: String,
    pub semantic_revision: u32,
    pub contract_sha256: String,
    pub namespace: String,
    pub compatibility_epoch: u32,
    pub domain: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractResourceRequirement {
    pub id: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionInvalidationManifest {
    pub cache_policy_id: String,
    pub tags: Vec<String>,
    pub consistency: String,
}

#[derive(Clone)]
pub struct NativeRuntimeBuilder {
    graph: Arc<RouteGraph>,
    deployment_id: String,
    limits: RequestLimits,
    transport_limits: TransportLimits,
    handlers: BTreeMap<String, Arc<dyn RuntimeHandler>>,
    middleware: BTreeMap<String, MiddlewareRegistration>,
    pre_route_middleware: BTreeMap<String, PreRouteMiddlewareRegistration>,
    error_boundaries: BTreeMap<String, Arc<dyn RuntimeErrorBoundary>>,
    duplicate_middleware: BTreeSet<String>,
    duplicate_pre_route_middleware: BTreeSet<String>,
    duplicate_error_boundaries: BTreeSet<String>,
    receipt_sink: Arc<dyn RuntimeReceiptSink>,
    telemetry: Option<Arc<OpenTelemetryRuntime>>,
    resources: ResourceRegistry,
    action_policies: BTreeMap<String, ActionPolicy>,
    duplicate_action_policies: BTreeSet<String>,
    loader_policies: BTreeMap<String, LoaderPolicy>,
    duplicate_loader_policies: BTreeSet<String>,
    cache_policies: BTreeMap<String, CachePolicy>,
    duplicate_cache_policies: BTreeSet<String>,
}

impl NativeRuntimeBuilder {
    pub fn new(
        graph: RouteGraph,
        deployment_id: impl Into<String>,
    ) -> Result<Self, RuntimeBuildError> {
        let deployment_id = deployment_id.into();
        RequestIdentity::new("probe", deployment_id.clone())?;
        Ok(Self {
            graph: Arc::new(graph),
            deployment_id,
            limits: RequestLimits::default(),
            transport_limits: TransportLimits::default(),
            handlers: BTreeMap::new(),
            middleware: BTreeMap::new(),
            pre_route_middleware: BTreeMap::new(),
            error_boundaries: BTreeMap::new(),
            duplicate_middleware: BTreeSet::new(),
            duplicate_pre_route_middleware: BTreeSet::new(),
            duplicate_error_boundaries: BTreeSet::new(),
            receipt_sink: Arc::new(|_: RuntimeReceipt| {}),
            telemetry: None,
            resources: ResourceRegistry::empty(),
            action_policies: BTreeMap::new(),
            duplicate_action_policies: BTreeSet::new(),
            loader_policies: BTreeMap::new(),
            duplicate_loader_policies: BTreeSet::new(),
            cache_policies: BTreeMap::new(),
            duplicate_cache_policies: BTreeSet::new(),
        })
    }

    pub fn limits(mut self, limits: RequestLimits) -> Result<Self, RuntimeBuildError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }

    pub fn transport_limits(mut self, limits: TransportLimits) -> Result<Self, RuntimeBuildError> {
        limits.validate()?;
        self.transport_limits = limits;
        Ok(self)
    }

    pub fn handler<H>(mut self, route_id: impl Into<String>, handler: H) -> Self
    where
        H: RuntimeHandler,
    {
        self.handlers.insert(route_id.into(), Arc::new(handler));
        self
    }

    pub fn middleware<M>(mut self, id: impl Into<String>, middleware: M) -> Self
    where
        M: RuntimeMiddleware,
    {
        self.register_middleware(id.into(), MiddlewareCapabilities::none(), middleware);
        self
    }

    pub fn middleware_with_capabilities<M>(
        mut self,
        id: impl Into<String>,
        capabilities: MiddlewareCapabilities,
        middleware: M,
    ) -> Self
    where
        M: RuntimeMiddleware,
    {
        self.register_middleware(id.into(), capabilities, middleware);
        self
    }

    fn register_middleware<M>(
        &mut self,
        id: String,
        capabilities: MiddlewareCapabilities,
        middleware: M,
    ) where
        M: RuntimeMiddleware,
    {
        if self
            .middleware
            .insert(
                id.clone(),
                MiddlewareRegistration {
                    capabilities,
                    handler: Arc::new(middleware),
                },
            )
            .is_some()
        {
            self.duplicate_middleware.insert(id);
        }
    }

    pub fn pre_route_middleware<M>(
        mut self,
        id: impl Into<String>,
        capabilities: MiddlewareCapabilities,
        middleware: M,
    ) -> Self
    where
        M: RuntimePreRouteMiddleware,
    {
        let id = id.into();
        if self
            .pre_route_middleware
            .insert(
                id.clone(),
                PreRouteMiddlewareRegistration {
                    capabilities,
                    handler: Arc::new(middleware),
                },
            )
            .is_some()
        {
            self.duplicate_pre_route_middleware.insert(id);
        }
        self
    }

    pub fn error_boundary<B>(mut self, id: impl Into<String>, boundary: B) -> Self
    where
        B: RuntimeErrorBoundary,
    {
        let id = id.into();
        if self
            .error_boundaries
            .insert(id.clone(), Arc::new(boundary))
            .is_some()
        {
            self.duplicate_error_boundaries.insert(id);
        }
        self
    }

    pub fn receipt_sink<S>(mut self, sink: S) -> Self
    where
        S: RuntimeReceiptSink,
    {
        self.receipt_sink = Arc::new(sink);
        self
    }

    pub fn resources(mut self, resources: ResourceRegistry) -> Self {
        self.resources = resources;
        self
    }

    pub fn action_policy(mut self, policy: ActionPolicy) -> Self {
        let id = policy.id().to_owned();
        if self.action_policies.insert(id.clone(), policy).is_some() {
            self.duplicate_action_policies.insert(id);
        }
        self
    }

    pub fn loader_policy(mut self, policy: LoaderPolicy) -> Self {
        let id = policy.id().to_owned();
        if self.loader_policies.insert(id.clone(), policy).is_some() {
            self.duplicate_loader_policies.insert(id);
        }
        self
    }

    pub fn cache_policy(mut self, policy: CachePolicy) -> Self {
        let id = policy.id().to_owned();
        if self.cache_policies.insert(id.clone(), policy).is_some() {
            self.duplicate_cache_policies.insert(id);
        }
        self
    }

    /// Enable OpenTelemetry through the operator's configured global providers.
    pub fn open_telemetry(mut self, config: OpenTelemetryConfig) -> Self {
        self.telemetry = Some(Arc::new(OpenTelemetryRuntime::from_global(config)));
        self
    }

    pub fn build(self) -> Result<NativeRuntime, RuntimeBuildError> {
        self.limits.validate()?;
        self.transport_limits.validate()?;
        if let Some(id) = self.duplicate_middleware.iter().next() {
            return Err(RuntimeBuildError::DuplicateMiddlewareRegistration(
                id.clone(),
            ));
        }
        if let Some(id) = self.duplicate_pre_route_middleware.iter().next() {
            return Err(RuntimeBuildError::DuplicatePreRouteMiddlewareRegistration(
                id.clone(),
            ));
        }
        if let Some(id) = self.duplicate_error_boundaries.iter().next() {
            return Err(RuntimeBuildError::DuplicateErrorBoundaryRegistration(
                id.clone(),
            ));
        }
        if let Some(id) = self.duplicate_action_policies.iter().next() {
            return Err(RuntimeBuildError::DuplicateActionPolicy(id.clone()));
        }
        if let Some(id) = self.duplicate_loader_policies.iter().next() {
            return Err(RuntimeBuildError::DuplicateLoaderPolicy(id.clone()));
        }
        if let Some(id) = self.duplicate_cache_policies.iter().next() {
            return Err(RuntimeBuildError::DuplicateCachePolicy(id.clone()));
        }
        let route_ids: BTreeSet<_> = self
            .graph
            .routes()
            .iter()
            .map(|route| route.id().to_owned())
            .collect();
        for route_id in &route_ids {
            if !self.handlers.contains_key(route_id) {
                return Err(RuntimeBuildError::MissingHandler(route_id.clone()));
            }
        }
        for route_id in self.handlers.keys() {
            if !route_ids.contains(route_id) {
                return Err(RuntimeBuildError::UnknownHandler(route_id.clone()));
            }
        }
        let middleware_ids = self.graph.route_middleware_ids().clone();
        validate_behavior_registry(
            &middleware_ids,
            self.middleware.keys(),
            RuntimeBuildError::MissingMiddleware,
            RuntimeBuildError::UnknownMiddleware,
        )?;
        let pre_route_ids: BTreeSet<_> = self
            .graph
            .pre_route_middleware_ids()
            .iter()
            .cloned()
            .collect();
        validate_behavior_registry(
            &pre_route_ids,
            self.pre_route_middleware.keys(),
            RuntimeBuildError::MissingPreRouteMiddleware,
            RuntimeBuildError::UnknownPreRouteMiddleware,
        )?;
        for (id, declared) in self.graph.middleware_declarations() {
            let registered = self
                .middleware
                .get(id)
                .map(|registration| &registration.capabilities)
                .or_else(|| {
                    self.pre_route_middleware
                        .get(id)
                        .map(|registration| &registration.capabilities)
                })
                .expect("middleware registry completeness was validated");
            if registered != declared {
                return Err(RuntimeBuildError::MiddlewareCapabilityMismatch {
                    id: id.clone(),
                    declared: declared.clone(),
                    registered: registered.clone(),
                });
            }
        }
        let error_boundary_ids = self.graph.all_error_boundary_ids().clone();
        validate_behavior_registry(
            &error_boundary_ids,
            self.error_boundaries.keys(),
            RuntimeBuildError::MissingErrorBoundary,
            RuntimeBuildError::UnknownErrorBoundary,
        )?;
        let action_ids = self
            .graph
            .routes()
            .iter()
            .flat_map(|route| route.action_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        validate_behavior_registry(
            &action_ids,
            self.action_policies.keys(),
            RuntimeBuildError::MissingActionPolicy,
            RuntimeBuildError::UnknownActionPolicy,
        )?;
        let loader_ids = self
            .graph
            .routes()
            .iter()
            .flat_map(|route| {
                self.graph
                    .route_loader_ids(route.id())
                    .expect("sealed routes have loader identities")
            })
            .collect::<BTreeSet<_>>();
        validate_behavior_registry(
            &loader_ids,
            self.loader_policies.keys(),
            RuntimeBuildError::MissingLoaderPolicy,
            RuntimeBuildError::UnknownLoaderPolicy,
        )?;
        let mut cache_ids = self
            .graph
            .routes()
            .iter()
            .filter_map(|route| route.cache_policy_id().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        cache_ids.extend(
            self.loader_policies
                .values()
                .filter_map(|policy| policy.cache_policy_id().map(str::to_owned)),
        );
        cache_ids.extend(
            self.action_policies
                .values()
                .flat_map(|policy| policy.invalidation_intents())
                .map(|intent| intent.cache_policy_id().to_owned()),
        );
        validate_behavior_registry(
            &cache_ids,
            self.cache_policies.keys(),
            RuntimeBuildError::MissingCachePolicy,
            RuntimeBuildError::UnknownCachePolicy,
        )?;
        for route in self.graph.routes() {
            let route_resources = self
                .graph
                .route_resource_requirements(route.id())
                .expect("sealed routes have resource requirements");
            for action_id in route.action_ids() {
                let policy = self
                    .action_policies
                    .get(action_id)
                    .expect("action policy registry completeness was validated");
                for requirement in policy.resource_requirements() {
                    let declared = route_resources.get(requirement.id()).ok_or_else(|| {
                        RuntimeBuildError::ActionResourceMismatch {
                            route: route.id().to_owned(),
                            action: action_id.clone(),
                            resource: requirement.id().to_owned(),
                        }
                    })?;
                    if requirement
                        .capabilities()
                        .iter()
                        .any(|capability| !declared.contains(capability))
                    {
                        return Err(RuntimeBuildError::ActionResourceMismatch {
                            route: route.id().to_owned(),
                            action: action_id.clone(),
                            resource: requirement.id().to_owned(),
                        });
                    }
                }
            }
            for loader_id in self
                .graph
                .route_loader_ids(route.id())
                .expect("sealed routes have loader identities")
            {
                let policy = self
                    .loader_policies
                    .get(&loader_id)
                    .expect("loader policy registry completeness was validated");
                for requirement in policy.resource_requirements() {
                    let declared = route_resources.get(requirement.id()).ok_or_else(|| {
                        RuntimeBuildError::LoaderResourceMismatch {
                            route: route.id().to_owned(),
                            loader: loader_id.clone(),
                            resource: requirement.id().to_owned(),
                        }
                    })?;
                    if requirement
                        .capabilities()
                        .iter()
                        .any(|capability| !declared.contains(capability))
                    {
                        return Err(RuntimeBuildError::LoaderResourceMismatch {
                            route: route.id().to_owned(),
                            loader: loader_id,
                            resource: requirement.id().to_owned(),
                        });
                    }
                }
            }
        }
        let contract_digest = runtime_contract_digest(
            &self.graph,
            &self.action_policies,
            &self.loader_policies,
            &self.cache_policies,
        );
        let registry = Arc::new(RequestRegistry::new(self.limits.max_concurrent_requests));
        Ok(NativeRuntime {
            state: Arc::new(RuntimeState {
                graph: self.graph,
                deployment_id: self.deployment_id,
                limits: self.limits,
                transport_limits: self.transport_limits,
                handlers: self.handlers,
                middleware: self.middleware,
                pre_route_middleware: self.pre_route_middleware,
                error_boundaries: self.error_boundaries,
                receipt_sink: self.receipt_sink,
                telemetry: self.telemetry,
                resources: self.resources,
                action_policies: Arc::new(self.action_policies),
                loader_policies: Arc::new(self.loader_policies),
                cache_policies: Arc::new(self.cache_policies),
                contract_digest,
                request_sequence: AtomicU64::new(0),
                server_started: AtomicBool::new(false),
                active_connections: AtomicUsize::new(0),
                rejected_connections: AtomicU64::new(0),
                registry,
            }),
        })
    }
}

fn validate_behavior_registry<'a, I, Missing, Unknown>(
    required: &BTreeSet<String>,
    registered: I,
    missing: Missing,
    unknown: Unknown,
) -> Result<(), RuntimeBuildError>
where
    I: IntoIterator<Item = &'a String>,
    Missing: Fn(String) -> RuntimeBuildError,
    Unknown: Fn(String) -> RuntimeBuildError,
{
    let registered: BTreeSet<_> = registered.into_iter().cloned().collect();
    if let Some(id) = required.iter().find(|id| !registered.contains(*id)) {
        return Err(missing(id.clone()));
    }
    if let Some(id) = registered.iter().find(|id| !required.contains(*id)) {
        return Err(unknown(id.clone()));
    }
    Ok(())
}

fn runtime_contract_digest(
    graph: &RouteGraph,
    action_policies: &BTreeMap<String, ActionPolicy>,
    loader_policies: &BTreeMap<String, LoaderPolicy>,
    cache_policies: &BTreeMap<String, CachePolicy>,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-runtime-contract-v1\0");
    digest.update(graph.digest().as_bytes());
    for (id, policy) in action_policies {
        digest.update((id.len() as u64).to_be_bytes());
        digest.update(id.as_bytes());
        digest.update(policy.contract_digest().as_bytes());
    }
    for (id, policy) in loader_policies {
        digest.update((id.len() as u64).to_be_bytes());
        digest.update(id.as_bytes());
        digest.update(policy.contract_digest().as_bytes());
    }
    for (id, policy) in cache_policies {
        digest.update((id.len() as u64).to_be_bytes());
        digest.update(id.as_bytes());
        digest.update(policy.contract_digest().as_bytes());
    }
    let bytes = digest.finalize();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit((byte >> 4) as u32, 16).expect("hex nibble is valid"));
        output.push(char::from_digit((byte & 0x0f) as u32, 16).expect("hex nibble is valid"));
    }
    output
}

struct RuntimeState {
    graph: Arc<RouteGraph>,
    deployment_id: String,
    limits: RequestLimits,
    transport_limits: TransportLimits,
    handlers: BTreeMap<String, Arc<dyn RuntimeHandler>>,
    middleware: BTreeMap<String, MiddlewareRegistration>,
    pre_route_middleware: BTreeMap<String, PreRouteMiddlewareRegistration>,
    error_boundaries: BTreeMap<String, Arc<dyn RuntimeErrorBoundary>>,
    receipt_sink: Arc<dyn RuntimeReceiptSink>,
    telemetry: Option<Arc<OpenTelemetryRuntime>>,
    resources: ResourceRegistry,
    action_policies: Arc<BTreeMap<String, ActionPolicy>>,
    loader_policies: Arc<BTreeMap<String, LoaderPolicy>>,
    cache_policies: Arc<BTreeMap<String, CachePolicy>>,
    contract_digest: String,
    request_sequence: AtomicU64,
    server_started: AtomicBool,
    active_connections: AtomicUsize,
    rejected_connections: AtomicU64,
    registry: Arc<RequestRegistry>,
}

#[derive(Clone)]
struct MiddlewareRegistration {
    capabilities: MiddlewareCapabilities,
    handler: Arc<dyn RuntimeMiddleware>,
}

#[derive(Clone)]
struct PreRouteMiddlewareRegistration {
    capabilities: MiddlewareCapabilities,
    handler: Arc<dyn RuntimePreRouteMiddleware>,
}

struct RequestRegistry {
    accepting: AtomicBool,
    maximum: usize,
    scopes: Mutex<BTreeMap<String, RequestScope>>,
}

impl RequestRegistry {
    fn new(maximum: usize) -> Self {
        Self {
            accepting: AtomicBool::new(true),
            maximum,
            scopes: Mutex::new(BTreeMap::new()),
        }
    }

    fn admit(self: &Arc<Self>, scope: &RequestScope) -> Result<(), AdmissionError> {
        let request_id = scope.identity().request_id.clone();
        let cleanup_request_id = request_id.clone();
        let registry = Arc::downgrade(self);
        scope
            .register_internal_cleanup(move || {
                if let Some(registry) = registry.upgrade() {
                    lock(&registry.scopes).remove(&cleanup_request_id);
                }
            })
            .map_err(AdmissionError::Internal)?;

        let mut scopes = lock(&self.scopes);
        if !self.accepting.load(Ordering::Acquire) {
            return Err(AdmissionError::Draining);
        }
        if scopes.len() >= self.maximum {
            return Err(AdmissionError::Overloaded);
        }
        scopes.insert(request_id, scope.clone());
        Ok(())
    }

    fn begin_shutdown(&self) {
        self.accepting.store(false, Ordering::Release);
        let scopes: Vec<_> = lock(&self.scopes).values().cloned().collect();
        for scope in scopes {
            scope.cancel(CancelReason::Shutdown);
        }
    }

    fn active_count(&self) -> usize {
        lock(&self.scopes).len()
    }
}

enum AdmissionError {
    Draining,
    Overloaded,
    Internal(ScopeError),
}

#[derive(Clone)]
pub struct NativeRuntime {
    state: Arc<RuntimeState>,
}

impl NativeRuntime {
    pub fn router(&self) -> Router {
        Router::new()
            .fallback(dispatch)
            .with_state(self.state.clone())
    }

    pub async fn serve<F>(self, listener: TcpListener, shutdown: F) -> io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if self
            .state
            .server_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "PliegoRS runtime server has already been started",
            ));
        }
        let registry = self.state.registry.clone();
        let drain_deadline = self.state.limits.graceful_shutdown_deadline();
        let connection_shutdown = CancellationToken::new();
        let connection_slots =
            Arc::new(Semaphore::new(self.state.transport_limits.max_connections));
        let mut connections = JoinSet::new();
        let mut accept_error = None;
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown => break,
                result = listener.accept() => {
                    let (stream, _) = match result {
                        Ok(connection) => connection,
                        Err(error) => {
                            accept_error = Some(error);
                            break;
                        }
                    };
                    let permit = match connection_slots.clone().try_acquire_owned() {
                        Ok(permit) => permit,
                        Err(_) => {
                            self.state.rejected_connections.fetch_add(1, Ordering::AcqRel);
                            drop(stream);
                            continue;
                        }
                    };
                    let state = self.state.clone();
                    let router = self.router();
                    let connection_shutdown = connection_shutdown.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        let _active = ActiveConnection::new(state.clone());
                        serve_connection(
                            stream,
                            router,
                            state.limits.clone(),
                            state.transport_limits.clone(),
                            connection_shutdown,
                        )
                        .await;
                    });
                }
                result = connections.join_next(), if !connections.is_empty() => {
                    if result.is_some_and(|result| result.is_err()) {
                        warn!(target: "pliegors::transport", "PliegoRS connection task failed");
                    }
                }
            }
        }

        registry.begin_shutdown();
        connection_shutdown.cancel();
        let drain = async {
            while let Some(result) = connections.join_next().await {
                if result.is_err() {
                    warn!(target: "pliegors::transport", "PliegoRS connection task failed");
                }
            }
        };
        if tokio::time::timeout(drain_deadline, drain).await.is_err() {
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "PliegoRS graceful shutdown exceeded its drain deadline",
            ));
        }
        if let Some(error) = accept_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub fn begin_shutdown(&self) {
        self.state.registry.begin_shutdown();
    }

    pub fn active_request_count(&self) -> usize {
        self.state.registry.active_count()
    }

    pub fn active_connection_count(&self) -> usize {
        self.state.active_connections.load(Ordering::Acquire)
    }

    pub fn rejected_connection_count(&self) -> u64 {
        self.state.rejected_connections.load(Ordering::Acquire)
    }

    pub fn transport_policy_sha256(&self) -> String {
        self.state.transport_limits.digest()
    }

    pub fn contract_sha256(&self) -> &str {
        &self.state.contract_digest
    }

    pub fn contract_manifest(&self) -> RuntimeContractManifest {
        RuntimeContractManifest {
            contract: "dev.pliegors.runtime-contract/v1".to_owned(),
            application_contract_sha256: self.state.contract_digest.clone(),
            actions: self
                .state
                .action_policies
                .values()
                .map(action_contract_manifest)
                .collect(),
            loaders: self
                .state
                .loader_policies
                .values()
                .map(loader_contract_manifest)
                .collect(),
            caches: self
                .state
                .cache_policies
                .values()
                .map(cache_contract_manifest)
                .collect(),
        }
    }
}

fn action_contract_manifest(policy: &ActionPolicy) -> ActionContractManifest {
    ActionContractManifest {
        id: policy.id().to_owned(),
        semantic_revision: policy.semantic_revision(),
        contract_sha256: policy.contract_digest(),
        accepted_media_types: policy
            .accepted_media_types()
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect(),
        accepted_content_encodings: policy
            .accepted_content_encodings()
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect(),
        max_encoded_bytes: policy.max_encoded_bytes_value(),
        max_decoded_bytes: policy.max_decoded_bytes_value(),
        max_form_fields: policy.max_form_fields_value(),
        max_output_bytes: policy.max_output_bytes_value(),
        origin_policy: policy.origin_policy_value().as_str().to_owned(),
        csrf_policy: policy.csrf_policy_value().as_str().to_owned(),
        requires_authentication: policy.requires_authentication(),
        requires_authorization: policy.requires_authorization(),
        post_commit_grace_ms: policy.post_commit_grace_ms(),
        idempotency_policy_id: policy.idempotency_policy_id().map(str::to_owned),
        resources: contract_resources(policy.resource_requirements()),
        invalidations: policy
            .invalidation_intents()
            .iter()
            .map(|intent| ActionInvalidationManifest {
                cache_policy_id: intent.cache_policy_id().to_owned(),
                tags: intent
                    .tags_value()
                    .iter()
                    .map(|tag| tag.as_str().to_owned())
                    .collect(),
                consistency: intent.consistency().as_str().to_owned(),
            })
            .collect(),
    }
}

fn loader_contract_manifest(policy: &LoaderPolicy) -> LoaderContractManifest {
    LoaderContractManifest {
        id: policy.id().to_owned(),
        semantic_revision: policy.semantic_revision(),
        contract_sha256: policy.contract_digest(),
        cache_policy_id: policy.cache_policy_id().map(str::to_owned),
        max_output_bytes: policy.max_output_bytes_value(),
        deduplicates_in_request: policy.deduplicates_in_request(),
        resources: contract_resources(policy.resource_requirements()),
    }
}

fn cache_contract_manifest(policy: &CachePolicy) -> CacheContractManifest {
    CacheContractManifest {
        id: policy.id().to_owned(),
        semantic_revision: policy.semantic_revision(),
        contract_sha256: policy.contract_digest(),
        namespace: policy.namespace().to_owned(),
        compatibility_epoch: policy.compatibility_epoch(),
        domain: policy.domain().as_str().to_owned(),
    }
}

fn contract_resources(
    requirements: &[pliego_data::ResourceRequirement],
) -> Vec<ContractResourceRequirement> {
    let mut resources = requirements
        .iter()
        .map(|requirement| ContractResourceRequirement {
            id: requirement.id().to_owned(),
            capabilities: requirement
                .capabilities()
                .iter()
                .map(str::to_owned)
                .collect(),
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| left.id.cmp(&right.id));
    resources
}

fn display_contract_items(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

struct ActiveConnection {
    state: Arc<RuntimeState>,
}

impl ActiveConnection {
    fn new(state: Arc<RuntimeState>) -> Self {
        state.active_connections.fetch_add(1, Ordering::AcqRel);
        Self { state }
    }
}

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        self.state.active_connections.fetch_sub(1, Ordering::AcqRel);
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    router: Router,
    request_limits: RequestLimits,
    limits: TransportLimits,
    shutdown: CancellationToken,
) {
    let io = TokioIo::new(TimedIo::new(stream, &limits));
    let service = router.map_request(|request: Request<Incoming>| request.map(Body::new));
    let service = TowerToHyperService::new(service);
    let mut builder = ConnectionBuilder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(limits.http1_header_read_timeout())
        .max_headers(request_limits.max_header_count)
        .max_buf_size(request_limits.max_header_bytes.max(8 * 1_024));
    builder
        .http2()
        .timer(TokioTimer::new())
        .max_concurrent_streams(limits.http2_max_concurrent_streams)
        .max_header_list_size(request_limits.max_header_bytes as u32)
        .initial_stream_window_size(limits.http2_initial_stream_window_bytes)
        .initial_connection_window_size(limits.http2_initial_connection_window_bytes)
        .max_send_buf_size(limits.http2_max_send_buffer_bytes);
    let connection = builder.serve_connection_with_upgrades(io, service);
    tokio::pin!(connection);
    tokio::select! {
        result = &mut connection => {
            if result.is_err() {
                debug!(target: "pliegors::transport", "PliegoRS transport connection closed");
            }
        }
        _ = shutdown.cancelled() => {
            connection.as_mut().graceful_shutdown();
            if connection.await.is_err() {
                debug!(target: "pliegors::transport", "PliegoRS transport connection closed during drain");
            }
        }
    }
}

async fn dispatch(
    State(state): State<Arc<RuntimeState>>,
    request: Request<Body>,
) -> Response<Body> {
    let telemetry = state.telemetry.as_ref().and_then(|runtime| {
        catch_unwind(AssertUnwindSafe(|| runtime.start(&request)))
            .map(Arc::new)
            .map_err(|_| warn!("PliegoRS OpenTelemetry request start panicked"))
            .ok()
    });
    let sequence = state.request_sequence.fetch_add(1, Ordering::AcqRel);
    let identity = RequestIdentity::new(
        format!("{}-{sequence:016x}", state.deployment_id),
        state.deployment_id.clone(),
    )
    .expect("deployment identity was validated by the builder");
    let scope = RequestScope::open(
        identity,
        state.contract_digest.clone(),
        state.limits.clone(),
        state.receipt_sink.clone(),
        telemetry,
    );
    match state.registry.admit(&scope) {
        Ok(()) => {}
        Err(AdmissionError::Draining) => {
            let diagnostic = RuntimeDiagnostic::new(
                "PLG-RUN-503",
                "runtime is draining and cannot admit new requests",
            )
            .expect("static diagnostic is valid");
            scope.reject(diagnostic.clone());
            return scoped_response(scope, StatusCode::SERVICE_UNAVAILABLE, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
        }
        Err(AdmissionError::Overloaded) => {
            let diagnostic =
                RuntimeDiagnostic::new("PLG-RUN-107", "concurrent request limit reached")
                    .expect("static diagnostic is valid");
            scope.reject(diagnostic.clone());
            return scoped_response(scope, StatusCode::SERVICE_UNAVAILABLE, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
        }
        Err(AdmissionError::Internal(error)) => {
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("scope diagnostics are bounded");
            scope.fail(diagnostic.clone());
            return scoped_response(scope, StatusCode::INTERNAL_SERVER_ERROR, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
        }
    }
    let (parts, body) = request.into_parts();

    if let Err(error) = state.limits.admit_head(&parts) {
        let status = limit_status(&error);
        let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
            .expect("limit diagnostics are bounded");
        let public = PublicError::new(PublicErrorClass::InvalidRequest, status, error.code());
        let response = recover_error(&state, &scope, None, public, diagnostic).await;
        return wrap_handler_response(scope, response).unwrap_or_else(|_| fallback_response());
    }
    if let Err(error) = state.limits.admit_body_format(&parts) {
        let status = limit_status(&error);
        let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
            .expect("body format diagnostics are bounded");
        let public = PublicError::new(PublicErrorClass::InvalidRequest, status, error.code());
        let response = recover_error(&state, &scope, None, public, diagnostic).await;
        return wrap_handler_response(scope, response).unwrap_or_else(|_| fallback_response());
    }
    if scope.transition(RequestState::HeadAdmitted).is_err() {
        return lifecycle_failure_response(scope);
    }

    let request_scope = scope.clone();
    let limited = Limited::new(body, state.limits.max_body_bytes).map_err(move |error| {
        request_scope.cancel(CancelReason::RequestBodyLimit);
        error
    });
    let tracker = BodyReadTracker::new();
    let tracked = TrackedRequestBody::new(Body::new(limited), tracker.clone());
    let mut request = Request::from_parts(parts, Body::new(tracked));
    request.extensions_mut().insert(tracker);
    let cancellation = scope.cancellation_token();
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => plain_cancelled_response(&scope),
        result = pre_route_future(state.clone(), scope.clone(), request) => {
            match result {
                Ok(response) => response,
                Err(error) => {
                    warn!(code = %error.diagnostic.code, "PliegoRS pre-route middleware failed");
                    let public = public_handler_error(&error);
                    recover_error(&state, &scope, None, public, error.diagnostic).await
                }
            }
        }
    };
    wrap_handler_response(scope, response).unwrap_or_else(|_| fallback_response())
}

async fn routed_response(
    state: Arc<RuntimeState>,
    scope: RequestScope,
    request: Request<Body>,
) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let method = match RouteMethod::new(parts.method.as_str()) {
        Ok(method) => method,
        Err(error) => {
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("route diagnostics are bounded");
            let public = PublicError::new(
                PublicErrorClass::InvalidRequest,
                StatusCode::BAD_REQUEST,
                error.code(),
            );
            let response = recover_error(&state, &scope, None, public, diagnostic).await;
            return response;
        }
    };
    let path = match decode_path(parts.uri.path()) {
        Ok(path) => path,
        Err(diagnostic) => {
            let public = PublicError::new(
                PublicErrorClass::InvalidRequest,
                StatusCode::BAD_REQUEST,
                "PLG-RUN-106",
            );
            let response = recover_error(&state, &scope, None, public, diagnostic).await;
            return response;
        }
    };
    let matched = match state.graph.resolve(&method, &path) {
        Ok(matched) => matched,
        Err(error) => {
            let (status, allow) = resolve_status(&error);
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("route diagnostics are bounded");
            let class = if matches!(error, ResolveError::NotFound) {
                PublicErrorClass::NotFound
            } else {
                PublicErrorClass::InvalidRequest
            };
            let public = PublicError::new(class, status, error.code());
            let mut response = recover_error(&state, &scope, None, public, diagnostic).await;
            if let Some(allow) = allow {
                if let Ok(value) = http::HeaderValue::from_str(&allow) {
                    response.headers_mut().insert(http::header::ALLOW, value);
                }
            }
            return response;
        }
    };
    if scope.transition(RequestState::RouteResolved).is_err()
        || scope.transition(RequestState::ScopeOpen).is_err()
    {
        return lifecycle_failure_uncommitted(&scope);
    }
    scope.set_route(&matched);

    let data = match open_data_context(&state, &scope, &matched, &parts) {
        Ok(data) => data,
        Err(error) => {
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("data diagnostics are bounded");
            let public = PublicError::new(
                PublicErrorClass::InternalFailure,
                StatusCode::INTERNAL_SERVER_ERROR,
                error.code(),
            );
            return recover_error(&state, &scope, Some(&matched), public, diagnostic).await;
        }
    };

    let request = Request::from_parts(parts, body);
    let context = RequestContext::new(
        scope.clone(),
        matched.clone(),
        data,
        state.action_policies.clone(),
        state.loader_policies.clone(),
        state.cache_policies.clone(),
    );
    if scope.transition(RequestState::HandlerRunning).is_err() {
        return lifecycle_failure_uncommitted(&scope);
    }

    let handler_future = match catch_unwind(AssertUnwindSafe(|| {
        route_handler_future(state.clone(), matched.clone(), context, request)
    })) {
        Ok(future) => future,
        Err(_) => {
            let diagnostic = RuntimeDiagnostic::new(
                "PLG-RUN-502",
                "request handler panicked before returning its future",
            )
            .expect("static diagnostic is valid");
            let public = PublicError::new(
                PublicErrorClass::InternalFailure,
                StatusCode::INTERNAL_SERVER_ERROR,
                "PLG-RUN-502",
            );
            let response = recover_error(&state, &scope, Some(&matched), public, diagnostic).await;
            return response;
        }
    };
    let cancellation = scope.cancellation_token();
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            plain_cancelled_response(&scope)
        }
        result = AssertUnwindSafe(handler_future).catch_unwind() => {
            match result {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    warn!(code = %error.diagnostic.code, "PliegoRS handler failed");
                    let public = public_handler_error(&error);
                    recover_error(
                        &state,
                        &scope,
                        Some(&matched),
                        public,
                        error.diagnostic.clone(),
                    ).await
                }
                Err(_) => {
                    let diagnostic = RuntimeDiagnostic::new(
                        "PLG-RUN-502",
                        "request handler panicked while being polled",
                    )
                    .expect("static diagnostic is valid");
                    let public = PublicError::new(
                        PublicErrorClass::InternalFailure,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "PLG-RUN-502",
                    );
                    recover_error(
                        &state,
                        &scope,
                        Some(&matched),
                        public,
                        diagnostic,
                    ).await
                }
            }
        }
    };
    response
}

fn open_data_context(
    state: &RuntimeState,
    scope: &RequestScope,
    matched: &RouteMatch,
    request: &http::request::Parts,
) -> Result<DataContext, pliego_data::DataError> {
    let mut grants = Vec::with_capacity(matched.resource_requirements().len());
    for (resource_id, capabilities) in matched.resource_requirements() {
        let mut grant = ResourceGrant::new(resource_id.clone())?;
        for capability in capabilities {
            grant = grant.allowing(capability.clone())?;
        }
        grants.push(grant);
    }
    let identity = DataIdentity::new(
        scope.identity().request_id.clone(),
        matched.route_id().to_owned(),
        scope.identity().deployment_id.clone(),
    )
    .map_err(|error| pliego_data::DataError::LoaderFailure(error.to_string()))?;
    let values = admitted_data_values(matched, request)?;
    let mut policy_grants = DataPolicyGrants::new();
    for id in matched.loader_ids() {
        let policy = state
            .loader_policies
            .get(id)
            .expect("loader registry was sealed with the route graph");
        policy_grants = policy_grants.loader(policy)?;
        if let Some(cache_id) = policy.cache_policy_id() {
            policy_grants = policy_grants.cache(
                state
                    .cache_policies
                    .get(cache_id)
                    .expect("loader cache policy registry was sealed"),
            )?;
        }
    }
    for id in matched.action_ids() {
        let policy = state
            .action_policies
            .get(id)
            .expect("action registry was sealed with the route graph");
        policy_grants = policy_grants.action(policy)?;
        for intent in policy.invalidation_intents() {
            policy_grants = policy_grants.cache(
                state
                    .cache_policies
                    .get(intent.cache_policy_id())
                    .expect("action invalidation cache policy registry was sealed"),
            )?;
        }
    }
    if let Some(id) = matched.cache_policy_id() {
        policy_grants = policy_grants.cache(
            state
                .cache_policies
                .get(id)
                .expect("route cache policy registry was sealed"),
        )?;
    }
    let (context, control) = DataContext::open_sealed(
        identity,
        scope.deadline(),
        state.resources.clone(),
        grants,
        values,
        policy_grants,
        DataContextOptions {
            max_receipts: state.limits.max_data_receipts,
            max_cleanups: state.limits.max_data_cleanups,
        },
    )?;
    scope.attach_data_context(context.clone());

    let cleanup_control = control.clone();
    scope
        .register_internal_cleanup(move || cleanup_control.close())
        .map_err(|error| pliego_data::DataError::LoaderFailure(error.to_string()))?;

    let cancellation = scope.cancellation_token();
    let completion = scope.completion_token();
    let cancellation_scope = scope.clone();
    tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                control.cancel(data_cancel_reason(
                    cancellation_scope.cancel_reason().as_ref()
                ));
            }
            _ = completion.cancelled() => {}
        }
    });
    Ok(context)
}

fn admitted_data_values(
    matched: &RouteMatch,
    request: &http::request::Parts,
) -> Result<DataRequestValues, pliego_data::DataError> {
    let mut query = BTreeMap::<String, Vec<String>>::new();
    if let Some(authored) = request.uri.query() {
        validate_query_percent_encoding(authored)?;
        let pairs =
            serde_urlencoded::from_str::<Vec<(String, String)>>(authored).map_err(|_| {
                pliego_data::DataError::RequestValues("query decoding failed".to_owned())
            })?;
        for (name, value) in pairs {
            query.entry(name).or_default().push(value);
        }
    }
    let metadata = BTreeMap::from([("method".to_owned(), request.method.as_str().to_owned())]);
    DataRequestValues::new(matched.parameters().clone(), query, metadata)
}

fn validate_query_percent_encoding(value: &str) -> Result<(), pliego_data::DataError> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(pliego_data::DataError::RequestValues(
                    "query contains invalid percent encoding".to_owned(),
                ));
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn data_cancel_reason(reason: Option<&CancelReason>) -> DataCancelReason {
    match reason {
        Some(CancelReason::ClientDisconnect) => DataCancelReason::ClientDisconnect,
        Some(CancelReason::Deadline) => DataCancelReason::Deadline,
        Some(CancelReason::Shutdown) => DataCancelReason::Shutdown,
        Some(CancelReason::ApplicationAbort) => DataCancelReason::ApplicationAbort,
        Some(CancelReason::RequestBodyLimit) => DataCancelReason::RequestBodyLimit,
        Some(CancelReason::ResponseBodyLimit) => DataCancelReason::ResponseBodyLimit,
        None => DataCancelReason::ScopeClosed,
    }
}

#[derive(Clone)]
struct PreRouteMiddlewareLayer {
    id: String,
    capabilities: MiddlewareCapabilities,
    handler: Arc<dyn RuntimePreRouteMiddleware>,
}

struct MiddlewareEnforcement {
    capabilities: MiddlewareCapabilities,
    next_called: Arc<AtomicBool>,
    downstream: Arc<Mutex<Option<ResponseObservation>>>,
    body_tracker: BodyReadTracker,
    body_reads_at_entry: u64,
}

#[derive(Clone)]
struct ResponseObservation {
    status: StatusCode,
    headers: http::HeaderMap,
}

impl From<&Response<Body>> for ResponseObservation {
    fn from(response: &Response<Body>) -> Self {
        Self {
            status: response.status(),
            headers: response.headers().clone(),
        }
    }
}

fn validate_forwarded_request(
    original_method: &http::Method,
    original_uri: &http::Uri,
    request: &Request<Body>,
    capabilities: &MiddlewareCapabilities,
    body_tracker: &BodyReadTracker,
    body_reads_at_entry: u64,
) -> Result<(), HandlerError> {
    let forwarded_tracker = request.extensions().get::<BodyReadTracker>();
    if forwarded_tracker.is_none_or(|tracker| !tracker.same(body_tracker)) {
        return Err(middleware_capability_error(
            "middleware removed or replaced the request body tracker",
        ));
    }
    if body_tracker.reads() > body_reads_at_entry
        && !capabilities.allows(MiddlewareCapability::ReadBody)
    {
        return Err(middleware_capability_error(
            "middleware read the request body without read-body",
        ));
    }
    if request.method() != original_method {
        return Err(middleware_capability_error(
            "middleware changed the request method",
        ));
    }
    let forwarded_uri = request.uri();
    if forwarded_uri == original_uri {
        return Ok(());
    }
    if !capabilities.allows(MiddlewareCapability::RewritePath) {
        return Err(middleware_capability_error(
            "middleware rewrote the request without rewrite-path",
        ));
    }
    if forwarded_uri.scheme() != original_uri.scheme()
        || forwarded_uri.authority() != original_uri.authority()
        || forwarded_uri.query() != original_uri.query()
    {
        return Err(middleware_capability_error(
            "rewrite-path cannot change scheme, authority, or query",
        ));
    }
    Ok(())
}

fn validate_response_effects(
    response: Response<Body>,
    enforcement: Option<&MiddlewareEnforcement>,
) -> Result<Response<Body>, HandlerError> {
    let Some(enforcement) = enforcement else {
        return Ok(response);
    };
    let next_called = enforcement.next_called.load(Ordering::Acquire);
    if !next_called {
        if enforcement.body_tracker.reads() > enforcement.body_reads_at_entry
            && !enforcement
                .capabilities
                .allows(MiddlewareCapability::ReadBody)
        {
            return Err(middleware_capability_error(
                "middleware read the request body without read-body",
            ));
        }
        require_status_capability(response.status(), &enforcement.capabilities)?;
        return Ok(response);
    }
    let downstream = lock(&enforcement.downstream).clone();
    if let Some(downstream) = downstream {
        if response.headers() != &downstream.headers
            && !enforcement
                .capabilities
                .allows(MiddlewareCapability::MutateResponseHeaders)
        {
            return Err(middleware_capability_error(
                "middleware changed response headers without mutate-response-headers",
            ));
        }
        if response.status() != downstream.status {
            require_status_capability(response.status(), &enforcement.capabilities)?;
            if !response.status().is_redirection()
                && !response.status().is_client_error()
                && !response.status().is_server_error()
            {
                return Err(middleware_capability_error(
                    "middleware changed the downstream success status",
                ));
            }
        }
    }
    Ok(response)
}

fn require_status_capability(
    status: StatusCode,
    capabilities: &MiddlewareCapabilities,
) -> Result<(), HandlerError> {
    let required = if status.is_redirection() {
        Some(MiddlewareCapability::Redirect)
    } else if status.is_client_error() || status.is_server_error() {
        Some(MiddlewareCapability::Reject)
    } else {
        None
    };
    if let Some(required) = required {
        if !capabilities.allows(required) {
            return Err(middleware_capability_error(format!(
                "middleware returned {} without {}",
                status,
                required.as_str()
            )));
        }
    }
    Ok(())
}

fn middleware_capability_error(message: impl Into<String>) -> HandlerError {
    HandlerError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        RuntimeDiagnostic::new("PLG-RUN-507", bounded(&message.into(), 320))
            .expect("capability diagnostics are bounded"),
    )
}

fn pre_route_future(
    state: Arc<RuntimeState>,
    scope: RequestScope,
    request: Request<Body>,
) -> HandlerFuture {
    let layers: Vec<PreRouteMiddlewareLayer> = state
        .graph
        .pre_route_middleware_ids()
        .iter()
        .map(|id| {
            let registration = state
                .pre_route_middleware
                .get(id)
                .expect("pre-route registry was sealed with the route graph");
            PreRouteMiddlewareLayer {
                id: id.clone(),
                capabilities: registration.capabilities.clone(),
                handler: registration.handler.clone(),
            }
        })
        .collect();
    run_pre_route_middleware(state, scope, Arc::new(layers), 0, request)
}

fn run_pre_route_middleware(
    state: Arc<RuntimeState>,
    scope: RequestScope,
    layers: Arc<Vec<PreRouteMiddlewareLayer>>,
    index: usize,
    request: Request<Body>,
) -> HandlerFuture {
    Box::pin(async move {
        let (future, enforcement) = if let Some(layer) = layers.get(index).cloned() {
            scope.record_middleware(&layer.id);
            let next_state = state.clone();
            let next_scope = scope.clone();
            let next_layers = layers.clone();
            let original_method = request.method().clone();
            let original_uri = request.uri().clone();
            let body_tracker = request
                .extensions()
                .get::<BodyReadTracker>()
                .expect("admitted requests carry a body tracker")
                .clone();
            let body_reads_at_entry = body_tracker.reads();
            let forwarded_capabilities = layer.capabilities.clone();
            let forwarded_body_tracker = body_tracker.clone();
            let next_called = Arc::new(AtomicBool::new(false));
            let next_observer = next_called.clone();
            let downstream: Arc<Mutex<Option<ResponseObservation>>> = Arc::new(Mutex::new(None));
            let downstream_observer = downstream.clone();
            let next = PreRouteNext::new(Box::new(move |request| {
                next_observer.store(true, Ordering::Release);
                if let Err(error) = validate_forwarded_request(
                    &original_method,
                    &original_uri,
                    &request,
                    &forwarded_capabilities,
                    &forwarded_body_tracker,
                    body_reads_at_entry,
                ) {
                    return Box::pin(async move { Err(error) });
                }
                let future = run_pre_route_middleware(
                    next_state,
                    next_scope,
                    next_layers,
                    index + 1,
                    request,
                );
                Box::pin(async move {
                    let result = future.await;
                    if let Ok(response) = &result {
                        *lock(&downstream_observer) = Some(ResponseObservation::from(response));
                    }
                    result
                })
            }));
            let context = PreRouteContext::new(scope.clone());
            (
                catch_unwind(AssertUnwindSafe(|| {
                    layer.handler.call(context, request, next)
                })),
                Some(MiddlewareEnforcement {
                    capabilities: layer.capabilities,
                    next_called,
                    downstream,
                    body_tracker,
                    body_reads_at_entry,
                }),
            )
        } else {
            return Ok(routed_response(state, scope, request).await);
        };
        let future = match future {
            Ok(future) => future,
            Err(_) => {
                let diagnostic = RuntimeDiagnostic::new(
                    "PLG-RUN-506",
                    "pre-route middleware panicked before returning its future",
                )
                .expect("static diagnostic is valid");
                let public = PublicError::new(
                    PublicErrorClass::InternalFailure,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-RUN-506",
                );
                return Ok(recover_error(&state, &scope, None, public, diagnostic).await);
            }
        };
        match AssertUnwindSafe(future).catch_unwind().await {
            Ok(Ok(response)) => match validate_response_effects(response, enforcement.as_ref()) {
                Ok(response) => Ok(response),
                Err(error) => {
                    let public = public_handler_error(&error);
                    Ok(recover_error(&state, &scope, None, public, error.diagnostic).await)
                }
            },
            Ok(Err(error)) => {
                warn!(code = %error.diagnostic.code, "PliegoRS pre-route layer failed");
                let public = public_handler_error(&error);
                Ok(recover_error(&state, &scope, None, public, error.diagnostic).await)
            }
            Err(_) => {
                let diagnostic = RuntimeDiagnostic::new(
                    "PLG-RUN-506",
                    "pre-route middleware panicked while being polled",
                )
                .expect("static diagnostic is valid");
                let public = PublicError::new(
                    PublicErrorClass::InternalFailure,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-RUN-506",
                );
                Ok(recover_error(&state, &scope, None, public, diagnostic).await)
            }
        }
    })
}

#[derive(Clone)]
struct MiddlewareLayer {
    id: String,
    capabilities: MiddlewareCapabilities,
    handler: Arc<dyn RuntimeMiddleware>,
}

fn route_handler_future(
    state: Arc<RuntimeState>,
    matched: RouteMatch,
    context: RequestContext,
    request: Request<Body>,
) -> HandlerFuture {
    let layers: Vec<MiddlewareLayer> = matched
        .middleware_ids()
        .iter()
        .map(|id| {
            let registration = state
                .middleware
                .get(id)
                .expect("middleware registry was sealed with the route graph");
            MiddlewareLayer {
                id: id.clone(),
                capabilities: registration.capabilities.clone(),
                handler: registration.handler.clone(),
            }
        })
        .collect();
    let handler = state
        .handlers
        .get(matched.route_id())
        .expect("runtime graph and handler registry were sealed together")
        .clone();
    run_middleware(
        state,
        Arc::new(matched),
        Arc::new(layers),
        handler,
        0,
        context,
        request,
    )
}

fn run_middleware(
    state: Arc<RuntimeState>,
    route: Arc<RouteMatch>,
    layers: Arc<Vec<MiddlewareLayer>>,
    handler: Arc<dyn RuntimeHandler>,
    index: usize,
    context: RequestContext,
    request: Request<Body>,
) -> HandlerFuture {
    Box::pin(async move {
        let scope = context.scope().clone();
        let (future, enforcement) = if let Some(layer) = layers.get(index).cloned() {
            scope.record_middleware(&layer.id);
            let next_state = state.clone();
            let next_route = route.clone();
            let next_layers = layers.clone();
            let next_handler = handler.clone();
            let next_context = context.clone();
            let original_method = request.method().clone();
            let original_uri = request.uri().clone();
            let body_tracker = request
                .extensions()
                .get::<BodyReadTracker>()
                .expect("admitted requests carry a body tracker")
                .clone();
            let body_reads_at_entry = body_tracker.reads();
            let forwarded_capabilities = layer.capabilities.clone();
            let forwarded_body_tracker = body_tracker.clone();
            let next_called = Arc::new(AtomicBool::new(false));
            let next_observer = next_called.clone();
            let downstream: Arc<Mutex<Option<ResponseObservation>>> = Arc::new(Mutex::new(None));
            let downstream_observer = downstream.clone();
            let next = MiddlewareNext::new(Box::new(move |request| {
                next_observer.store(true, Ordering::Release);
                if let Err(error) = validate_forwarded_request(
                    &original_method,
                    &original_uri,
                    &request,
                    &forwarded_capabilities,
                    &forwarded_body_tracker,
                    body_reads_at_entry,
                ) {
                    return Box::pin(async move { Err(error) });
                }
                let future = run_middleware(
                    next_state,
                    next_route,
                    next_layers,
                    next_handler,
                    index + 1,
                    next_context,
                    request,
                );
                Box::pin(async move {
                    let result = future.await;
                    if let Ok(response) = &result {
                        *lock(&downstream_observer) = Some(ResponseObservation::from(response));
                    }
                    result
                })
            }));
            (
                catch_unwind(AssertUnwindSafe(|| {
                    layer.handler.call(context, request, next)
                })),
                Some(MiddlewareEnforcement {
                    capabilities: layer.capabilities,
                    next_called,
                    downstream,
                    body_tracker,
                    body_reads_at_entry,
                }),
            )
        } else {
            (
                catch_unwind(AssertUnwindSafe(|| handler.call(context, request))),
                None,
            )
        };
        let future = match future {
            Ok(future) => future,
            Err(_) => {
                let diagnostic = RuntimeDiagnostic::new(
                    "PLG-RUN-502",
                    "route execution panicked before returning its future",
                )
                .expect("static diagnostic is valid");
                let public = PublicError::new(
                    PublicErrorClass::InternalFailure,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-RUN-502",
                );
                return Ok(recover_error(&state, &scope, Some(&route), public, diagnostic).await);
            }
        };
        match AssertUnwindSafe(future).catch_unwind().await {
            Ok(Ok(response)) => match validate_response_effects(response, enforcement.as_ref()) {
                Ok(response) => Ok(response),
                Err(error) => {
                    let public = public_handler_error(&error);
                    Ok(recover_error(&state, &scope, Some(&route), public, error.diagnostic).await)
                }
            },
            Ok(Err(error)) => {
                warn!(code = %error.diagnostic.code, "PliegoRS route layer failed");
                let public = public_handler_error(&error);
                Ok(recover_error(&state, &scope, Some(&route), public, error.diagnostic).await)
            }
            Err(_) => {
                let diagnostic = RuntimeDiagnostic::new(
                    "PLG-RUN-502",
                    "route execution panicked while being polled",
                )
                .expect("static diagnostic is valid");
                let public = PublicError::new(
                    PublicErrorClass::InternalFailure,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PLG-RUN-502",
                );
                Ok(recover_error(&state, &scope, Some(&route), public, diagnostic).await)
            }
        }
    })
}

fn public_handler_error(error: &HandlerError) -> PublicError {
    let class = match error.status {
        StatusCode::NOT_FOUND => PublicErrorClass::NotFound,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            PublicErrorClass::UnauthorizedOrForbidden
        }
        status if status.is_client_error() => PublicErrorClass::InvalidRequest,
        _ => PublicErrorClass::InternalFailure,
    };
    let status = if error.status.is_client_error() || error.status.is_server_error() {
        error.status
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    PublicError::new(class, status, error.diagnostic.code.clone())
}

async fn recover_error(
    state: &RuntimeState,
    scope: &RequestScope,
    route: Option<&RouteMatch>,
    public: PublicError,
    diagnostic: RuntimeDiagnostic,
) -> Response<Body> {
    if public.class() == PublicErrorClass::InternalFailure {
        scope.fail(diagnostic);
    } else {
        scope.reject(diagnostic);
    }

    let route_id = route.map(|route| route.route_id().to_owned());
    let mut boundary_ids = Vec::new();
    if let Some(route) = route {
        boundary_ids.extend(route.error_boundary_ids().iter().rev().cloned());
    }
    boundary_ids.extend(state.graph.error_boundary_ids().iter().rev().cloned());

    for id in boundary_ids {
        let boundary = state
            .error_boundaries
            .get(&id)
            .expect("error boundary registry was sealed with the route graph")
            .clone();
        let context = ErrorBoundaryContext::new(route_id.clone());
        let boundary_future =
            match catch_unwind(AssertUnwindSafe(|| boundary.call(context, public.clone()))) {
                Ok(future) => future,
                Err(_) => {
                    scope.fail(
                        RuntimeDiagnostic::new(
                            "PLG-RUN-504",
                            format!("error boundary {id} panicked before returning its future"),
                        )
                        .expect("sealed boundary IDs produce bounded diagnostics"),
                    );
                    continue;
                }
            };
        let cancellation = scope.cancellation_token();
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return plain_cancelled_response(scope);
            }
            result = AssertUnwindSafe(boundary_future).catch_unwind() => result,
        };
        let response = match result {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                scope.fail(error.diagnostic);
                continue;
            }
            Err(_) => {
                scope.fail(
                    RuntimeDiagnostic::new(
                        "PLG-RUN-504",
                        format!("error boundary {id} panicked while being polled"),
                    )
                    .expect("sealed boundary IDs produce bounded diagnostics"),
                );
                continue;
            }
        };
        if response.status() != public.status() {
            scope.fail(
                RuntimeDiagnostic::new(
                    "PLG-RUN-505",
                    format!(
                        "error boundary {id} returned {}; required {}",
                        response.status(),
                        public.status()
                    ),
                )
                .expect("HTTP status diagnostics are bounded"),
            );
            continue;
        }
        scope.set_error_boundary(&id);
        return response;
    }

    plain_error_response(public.status(), public.code())
}

fn plain_cancelled_response(scope: &RequestScope) -> Response<Body> {
    let (status, code) = match scope.cancel_reason() {
        Some(CancelReason::Deadline) => (StatusCode::GATEWAY_TIMEOUT, "PLG-RUN-408"),
        Some(CancelReason::Shutdown) => (StatusCode::SERVICE_UNAVAILABLE, "PLG-RUN-503"),
        _ => (StatusCode::REQUEST_TIMEOUT, "PLG-RUN-499"),
    };
    plain_error_response(status, code)
}

fn plain_error_response(status: StatusCode, code: impl AsRef<str>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(format!("{}\n", code.as_ref())))
        .expect("runtime error response is valid")
}

fn wrap_handler_response(
    scope: RequestScope,
    response: Response<Body>,
) -> Result<Response<Body>, ScopeError> {
    if let Some(mode) = response.extensions().get::<crate::RenderMode>().copied() {
        scope.set_render_mode(mode);
    }
    let terminal = scope.is_cancelled();
    let (parts, body) = response.into_parts();
    commit_response_or_close(&scope, parts.status.as_u16())?;
    debug!(request_id = %scope.identity().request_id, status = parts.status.as_u16(), "PliegoRS response committed");
    Ok(Response::from_parts(
        parts,
        Body::new(ScopedBody::new(body, scope, terminal)),
    ))
}

fn scoped_response(
    scope: RequestScope,
    status: StatusCode,
    code: impl Into<String>,
) -> Result<Response<Body>, ScopeError> {
    let code = code.into();
    let body = Body::from(format!("{code}\n"));
    let response = Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body)
        .expect("static runtime response is valid");
    let (parts, body) = response.into_parts();
    if !matches!(
        scope.state(),
        RequestState::ResponseCommitted | RequestState::BodyStreaming
    ) {
        commit_response_or_close(&scope, status.as_u16())?;
    }
    Ok(Response::from_parts(
        parts,
        Body::new(ScopedBody::new(body, scope, true)),
    ))
}

fn fallback_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("PLG-RUN-500\n"))
        .expect("fallback response is valid")
}

fn lifecycle_failure_response(scope: RequestScope) -> Response<Body> {
    let response = lifecycle_failure_uncommitted(&scope);
    wrap_handler_response(scope, response).unwrap_or_else(|_| fallback_response())
}

fn lifecycle_failure_uncommitted(scope: &RequestScope) -> Response<Body> {
    let diagnostic = RuntimeDiagnostic::new(
        "PLG-RUN-500",
        "request lifecycle entered an invalid transition",
    )
    .expect("static diagnostic is valid");
    scope.fail(diagnostic.clone());
    plain_error_response(StatusCode::INTERNAL_SERVER_ERROR, diagnostic.code)
}

fn commit_response_or_close(scope: &RequestScope, status: u16) -> Result<(), ScopeError> {
    match scope.commit_response(status) {
        Ok(()) => Ok(()),
        Err(error) => {
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("scope diagnostics are bounded");
            scope.fail(diagnostic);
            scope.drain_and_close();
            Err(error)
        }
    }
}

fn limit_status(error: &LimitError) -> StatusCode {
    match error {
        LimitError::HeaderCount { .. } | LimitError::HeaderBytes { .. } => {
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        }
        LimitError::BodyBytes { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        LimitError::UnsupportedContentEncoding | LimitError::UnsupportedMultipart => {
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        }
        LimitError::InvalidPolicy { .. }
        | LimitError::RequestTarget { .. }
        | LimitError::InvalidContentLength
        | LimitError::AmbiguousBodyLength => StatusCode::BAD_REQUEST,
    }
}

fn resolve_status(error: &ResolveError) -> (StatusCode, Option<String>) {
    match error {
        ResolveError::InvalidPath => (StatusCode::BAD_REQUEST, None),
        ResolveError::NotFound => (StatusCode::NOT_FOUND, None),
        ResolveError::MethodNotAllowed { allowed } => (
            StatusCode::METHOD_NOT_ALLOWED,
            Some(
                allowed
                    .iter()
                    .map(RouteMethod::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ),
    }
}

fn decode_path(encoded: &str) -> Result<String, RuntimeDiagnostic> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(invalid_path_diagnostic());
        }
        let high = hex(bytes[index + 1]).ok_or_else(invalid_path_diagnostic)?;
        let low = hex(bytes[index + 2]).ok_or_else(invalid_path_diagnostic)?;
        let value = (high << 4) | low;
        if matches!(value, 0 | b'/' | b'\\') {
            return Err(invalid_path_diagnostic());
        }
        decoded.push(value);
        index += 3;
    }
    let decoded = String::from_utf8(decoded).map_err(|_| invalid_path_diagnostic())?;
    if decoded.contains('%') {
        return Err(invalid_path_diagnostic());
    }
    Ok(decoded)
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn invalid_path_diagnostic() -> RuntimeDiagnostic {
    RuntimeDiagnostic::new("PLG-RUN-106", "request path encoding is invalid")
        .expect("static diagnostic is valid")
}

fn bounded(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        value.to_owned()
    } else {
        let mut end = maximum;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value[..end].to_owned()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
struct BodyReadTracker {
    polls: Arc<AtomicU64>,
}

impl BodyReadTracker {
    fn new() -> Self {
        Self {
            polls: Arc::new(AtomicU64::new(0)),
        }
    }

    fn reads(&self) -> u64 {
        self.polls.load(Ordering::Acquire)
    }

    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.polls, &other.polls)
    }
}

struct TrackedRequestBody {
    inner: Body,
    tracker: BodyReadTracker,
}

impl TrackedRequestBody {
    fn new(inner: Body, tracker: BodyReadTracker) -> Self {
        Self { inner, tracker }
    }
}

impl HttpBody for TrackedRequestBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.tracker.polls.fetch_add(1, Ordering::AcqRel);
        Pin::new(&mut self.inner).poll_frame(context)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

struct ScopedBody {
    inner: Body,
    scope: RequestScope,
    cancellation: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    allow_cancelled: bool,
    started: bool,
    finished: bool,
}

impl ScopedBody {
    fn new(inner: Body, scope: RequestScope, allow_cancelled: bool) -> Self {
        let token = scope.cancellation_token();
        Self {
            inner,
            scope,
            cancellation: Box::pin(async move { token.cancelled().await }),
            allow_cancelled,
            started: false,
            finished: false,
        }
    }

    fn finish(&mut self, success: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        if success {
            self.scope.complete_success();
        } else {
            self.scope.drain_and_close();
        }
    }
}

impl HttpBody for ScopedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.finished {
            return Poll::Ready(None);
        }
        if !self.allow_cancelled && self.cancellation.as_mut().poll(context).is_ready() {
            self.finish(false);
            return Poll::Ready(Some(Err(axum::Error::new(CancelledBody))));
        }
        match Pin::new(&mut self.inner).poll_frame(context) {
            Poll::Ready(Some(Ok(frame))) => {
                if !self.started {
                    self.started = true;
                    let _ = self.scope.transition(RequestState::BodyStreaming);
                }
                if let Some(data) = frame.data_ref() {
                    if let Err(error) = self.scope.add_response_bytes(data.len()) {
                        self.finish(false);
                        return Poll::Ready(Some(Err(axum::Error::new(error))));
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.scope.fail(
                    RuntimeDiagnostic::new("PLG-RUN-501", "response body stream failed")
                        .expect("static diagnostic is valid"),
                );
                self.finish(false);
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.finish(true);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.finished || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for ScopedBody {
    fn drop(&mut self) {
        if !self.finished {
            if self.scope.outcome() == crate::RequestOutcome::Pending {
                self.scope.cancel(CancelReason::ClientDisconnect);
            }
            self.finish(false);
        }
    }
}

#[derive(Debug)]
struct CancelledBody;

impl Display for CancelledBody {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("request scope cancelled")
    }
}

impl std::error::Error for CancelledBody {}

#[derive(Debug)]
pub enum RuntimeBuildError {
    InvalidLimits(LimitError),
    InvalidTransportLimits(TransportLimitError),
    InvalidIdentity(ScopeError),
    MissingHandler(String),
    UnknownHandler(String),
    MissingMiddleware(String),
    UnknownMiddleware(String),
    DuplicateMiddlewareRegistration(String),
    MissingPreRouteMiddleware(String),
    UnknownPreRouteMiddleware(String),
    DuplicatePreRouteMiddlewareRegistration(String),
    MiddlewareCapabilityMismatch {
        id: String,
        declared: MiddlewareCapabilities,
        registered: MiddlewareCapabilities,
    },
    MissingErrorBoundary(String),
    UnknownErrorBoundary(String),
    DuplicateErrorBoundaryRegistration(String),
    MissingActionPolicy(String),
    UnknownActionPolicy(String),
    DuplicateActionPolicy(String),
    ActionResourceMismatch {
        route: String,
        action: String,
        resource: String,
    },
    MissingLoaderPolicy(String),
    UnknownLoaderPolicy(String),
    DuplicateLoaderPolicy(String),
    LoaderResourceMismatch {
        route: String,
        loader: String,
        resource: String,
    },
    MissingCachePolicy(String),
    UnknownCachePolicy(String),
    DuplicateCachePolicy(String),
}

impl Display for RuntimeBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimits(error) => Display::fmt(error, formatter),
            Self::InvalidTransportLimits(error) => Display::fmt(error, formatter),
            Self::InvalidIdentity(error) => Display::fmt(error, formatter),
            Self::MissingHandler(route) => {
                write!(formatter, "route {route} has no runtime handler")
            }
            Self::UnknownHandler(route) => {
                write!(formatter, "handler references unknown route {route}")
            }
            Self::MissingMiddleware(id) => {
                write!(formatter, "route graph references missing middleware {id}")
            }
            Self::UnknownMiddleware(id) => {
                write!(formatter, "middleware registry contains unknown ID {id}")
            }
            Self::DuplicateMiddlewareRegistration(id) => {
                write!(
                    formatter,
                    "middleware ID {id} was registered more than once"
                )
            }
            Self::MissingPreRouteMiddleware(id) => {
                write!(
                    formatter,
                    "route graph references missing pre-route middleware {id}"
                )
            }
            Self::UnknownPreRouteMiddleware(id) => {
                write!(
                    formatter,
                    "pre-route middleware registry contains unknown ID {id}"
                )
            }
            Self::DuplicatePreRouteMiddlewareRegistration(id) => write!(
                formatter,
                "pre-route middleware ID {id} was registered more than once"
            ),
            Self::MiddlewareCapabilityMismatch {
                id,
                declared,
                registered,
            } => write!(
                formatter,
                "middleware {id} capability mismatch: graph declares {declared:?}, runtime registers {registered:?}"
            ),
            Self::MissingErrorBoundary(id) => {
                write!(
                    formatter,
                    "route graph references missing error boundary {id}"
                )
            }
            Self::UnknownErrorBoundary(id) => {
                write!(
                    formatter,
                    "error boundary registry contains unknown ID {id}"
                )
            }
            Self::DuplicateErrorBoundaryRegistration(id) => {
                write!(
                    formatter,
                    "error boundary ID {id} was registered more than once"
                )
            }
            Self::MissingActionPolicy(id) => {
                write!(
                    formatter,
                    "route graph references missing action policy {id}"
                )
            }
            Self::UnknownActionPolicy(id) => {
                write!(formatter, "action policy registry contains unknown ID {id}")
            }
            Self::DuplicateActionPolicy(id) => {
                write!(
                    formatter,
                    "action policy ID {id} was registered more than once"
                )
            }
            Self::ActionResourceMismatch {
                route,
                action,
                resource,
            } => write!(
                formatter,
                "route {route} does not seal resource {resource} required by action {action}"
            ),
            Self::MissingLoaderPolicy(id) => {
                write!(
                    formatter,
                    "route graph references missing loader policy {id}"
                )
            }
            Self::UnknownLoaderPolicy(id) => {
                write!(formatter, "loader policy registry contains unknown ID {id}")
            }
            Self::DuplicateLoaderPolicy(id) => {
                write!(
                    formatter,
                    "loader policy ID {id} was registered more than once"
                )
            }
            Self::LoaderResourceMismatch {
                route,
                loader,
                resource,
            } => write!(
                formatter,
                "route {route} does not seal resource {resource} required by loader {loader}"
            ),
            Self::MissingCachePolicy(id) => {
                write!(
                    formatter,
                    "route graph references missing cache policy {id}"
                )
            }
            Self::UnknownCachePolicy(id) => {
                write!(formatter, "cache policy registry contains unknown ID {id}")
            }
            Self::DuplicateCachePolicy(id) => {
                write!(
                    formatter,
                    "cache policy ID {id} was registered more than once"
                )
            }
        }
    }
}

impl std::error::Error for RuntimeBuildError {}

impl From<LimitError> for RuntimeBuildError {
    fn from(value: LimitError) -> Self {
        Self::InvalidLimits(value)
    }
}

impl From<TransportLimitError> for RuntimeBuildError {
    fn from(value: TransportLimitError) -> Self {
        Self::InvalidTransportLimits(value)
    }
}

impl From<ScopeError> for RuntimeBuildError {
    fn from(value: ScopeError) -> Self {
        Self::InvalidIdentity(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_decoder_is_single_pass_and_rejects_encoded_separators() {
        assert_eq!(decode_path("/caf%C3%A9").unwrap(), "/café");
        for path in ["/%2f", "/%5c", "/%00", "/%", "/%GG", "/%252f"] {
            assert!(decode_path(path).is_err(), "accepted {path}");
        }
    }
}
