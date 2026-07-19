// SPDX-License-Identifier: Apache-2.0

use crate::{
    CancelReason, LimitError, RequestContext, RequestIdentity, RequestLimits, RequestScope,
    RequestState, RuntimeDiagnostic, RuntimeReceipt, RuntimeReceiptSink, ScopeError,
};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use futures_util::FutureExt;
use http::{Request, Response, StatusCode};
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::{BodyExt, Limited};
use pliego_router::{ResolveError, RouteGraph, RouteMethod};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::future::{Future, IntoFuture};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
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

#[derive(Clone)]
pub struct NativeRuntimeBuilder {
    graph: Arc<RouteGraph>,
    deployment_id: String,
    limits: RequestLimits,
    handlers: BTreeMap<String, Arc<dyn RuntimeHandler>>,
    receipt_sink: Arc<dyn RuntimeReceiptSink>,
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
            handlers: BTreeMap::new(),
            receipt_sink: Arc::new(|_: RuntimeReceipt| {}),
        })
    }

    pub fn limits(mut self, limits: RequestLimits) -> Result<Self, RuntimeBuildError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }

    pub fn handler<H>(mut self, route_id: impl Into<String>, handler: H) -> Self
    where
        H: RuntimeHandler,
    {
        self.handlers.insert(route_id.into(), Arc::new(handler));
        self
    }

    pub fn receipt_sink<S>(mut self, sink: S) -> Self
    where
        S: RuntimeReceiptSink,
    {
        self.receipt_sink = Arc::new(sink);
        self
    }

    pub fn build(self) -> Result<NativeRuntime, RuntimeBuildError> {
        self.limits.validate()?;
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
        let registry = Arc::new(RequestRegistry::new(self.limits.max_concurrent_requests));
        Ok(NativeRuntime {
            state: Arc::new(RuntimeState {
                graph: self.graph,
                deployment_id: self.deployment_id,
                limits: self.limits,
                handlers: self.handlers,
                receipt_sink: self.receipt_sink,
                request_sequence: AtomicU64::new(0),
                registry,
            }),
        })
    }
}

struct RuntimeState {
    graph: Arc<RouteGraph>,
    deployment_id: String,
    limits: RequestLimits,
    handlers: BTreeMap<String, Arc<dyn RuntimeHandler>>,
    receipt_sink: Arc<dyn RuntimeReceiptSink>,
    request_sequence: AtomicU64,
    registry: Arc<RequestRegistry>,
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
        let registry = self.state.registry.clone();
        let drain_deadline = self.state.limits.graceful_shutdown_deadline();
        let shutdown_started = CancellationToken::new();
        let shutdown_observer = shutdown_started.clone();
        let graceful = async move {
            shutdown.await;
            registry.begin_shutdown();
            shutdown_started.cancel();
        };
        let server = axum::serve(listener, self.router())
            .with_graceful_shutdown(graceful)
            .into_future();
        tokio::pin!(server);
        tokio::select! {
            result = &mut server => result,
            _ = shutdown_observer.cancelled() => {
                match tokio::time::timeout(drain_deadline, &mut server).await {
                    Ok(result) => result,
                    Err(_) => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "PliegoRS graceful shutdown exceeded its drain deadline",
                    )),
                }
            }
        }
    }

    pub fn begin_shutdown(&self) {
        self.state.registry.begin_shutdown();
    }

    pub fn active_request_count(&self) -> usize {
        self.state.registry.active_count()
    }
}

async fn dispatch(
    State(state): State<Arc<RuntimeState>>,
    request: Request<Body>,
) -> Response<Body> {
    let sequence = state.request_sequence.fetch_add(1, Ordering::AcqRel);
    let identity = RequestIdentity::new(
        format!("{}-{sequence:016x}", state.deployment_id),
        state.deployment_id.clone(),
    )
    .expect("deployment identity was validated by the builder");
    let scope = RequestScope::open(identity, state.limits.clone(), state.receipt_sink.clone());
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
        scope.reject(diagnostic.clone());
        return scoped_response(scope, status, diagnostic.code)
            .unwrap_or_else(|_| fallback_response());
    }
    if scope.transition(RequestState::HeadAdmitted).is_err() {
        return lifecycle_failure_response(scope);
    }

    let method = match RouteMethod::new(parts.method.as_str()) {
        Ok(method) => method,
        Err(error) => {
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("route diagnostics are bounded");
            scope.reject(diagnostic.clone());
            return scoped_response(scope, StatusCode::BAD_REQUEST, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
        }
    };
    let path = match decode_path(parts.uri.path()) {
        Ok(path) => path,
        Err(diagnostic) => {
            scope.reject(diagnostic.clone());
            return scoped_response(scope, StatusCode::BAD_REQUEST, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
        }
    };
    let matched = match state.graph.resolve(&method, &path) {
        Ok(matched) => matched,
        Err(error) => {
            let (status, allow) = resolve_status(&error);
            let diagnostic = RuntimeDiagnostic::new(error.code(), bounded(&error.to_string(), 320))
                .expect("route diagnostics are bounded");
            scope.reject(diagnostic.clone());
            let mut response = scoped_response(scope, status, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
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
        return lifecycle_failure_response(scope);
    }
    scope.set_route(matched.route_id());

    let request_scope = scope.clone();
    let limited = Limited::new(body, state.limits.max_body_bytes).map_err(move |error| {
        request_scope.cancel(CancelReason::RequestBodyLimit);
        error
    });
    let request = Request::from_parts(parts, Body::new(limited));
    let handler = state
        .handlers
        .get(matched.route_id())
        .expect("runtime graph and handler registry were sealed together")
        .clone();
    let context = RequestContext::new(scope.clone(), matched);
    if scope.transition(RequestState::HandlerRunning).is_err() {
        return lifecycle_failure_response(scope);
    }

    let handler_future = match catch_unwind(AssertUnwindSafe(|| handler.call(context, request))) {
        Ok(future) => future,
        Err(_) => {
            let diagnostic = RuntimeDiagnostic::new(
                "PLG-RUN-502",
                "request handler panicked before returning its future",
            )
            .expect("static diagnostic is valid");
            scope.fail(diagnostic.clone());
            return scoped_response(scope, StatusCode::INTERNAL_SERVER_ERROR, diagnostic.code)
                .unwrap_or_else(|_| fallback_response());
        }
    };
    let cancellation = scope.cancellation_token();
    let response = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            let (status, code) = match scope.cancel_reason() {
                Some(CancelReason::Deadline) => (StatusCode::GATEWAY_TIMEOUT, "PLG-RUN-408"),
                Some(CancelReason::Shutdown) => (StatusCode::SERVICE_UNAVAILABLE, "PLG-RUN-503"),
                _ => (StatusCode::REQUEST_TIMEOUT, "PLG-RUN-499"),
            };
            scoped_response(scope, status, code).unwrap_or_else(|_| fallback_response())
        }
        result = AssertUnwindSafe(handler_future).catch_unwind() => {
            match result {
                Ok(Ok(response)) => wrap_handler_response(scope, response)
                    .unwrap_or_else(|_| fallback_response()),
                Ok(Err(error)) => {
                    warn!(code = %error.diagnostic.code, "PliegoRS handler failed");
                    scope.fail(error.diagnostic.clone());
                    scoped_response(scope, error.status, error.diagnostic.code)
                        .unwrap_or_else(|_| fallback_response())
                }
                Err(_) => {
                    let diagnostic = RuntimeDiagnostic::new(
                        "PLG-RUN-502",
                        "request handler panicked while being polled",
                    )
                    .expect("static diagnostic is valid");
                    scope.fail(diagnostic.clone());
                    scoped_response(scope, StatusCode::INTERNAL_SERVER_ERROR, diagnostic.code)
                        .unwrap_or_else(|_| fallback_response())
                }
            }
        }
    };
    response
}

fn wrap_handler_response(
    scope: RequestScope,
    response: Response<Body>,
) -> Result<Response<Body>, ScopeError> {
    if let Some(mode) = response.extensions().get::<crate::RenderMode>().copied() {
        scope.set_render_mode(mode);
    }
    let (parts, body) = response.into_parts();
    commit_response_or_close(&scope, parts.status.as_u16())?;
    debug!(request_id = %scope.identity().request_id, status = parts.status.as_u16(), "PliegoRS response committed");
    Ok(Response::from_parts(
        parts,
        Body::new(ScopedBody::new(body, scope, false)),
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
    let diagnostic = RuntimeDiagnostic::new(
        "PLG-RUN-500",
        "request lifecycle entered an invalid transition",
    )
    .expect("static diagnostic is valid");
    scope.fail(diagnostic.clone());
    scoped_response(scope, StatusCode::INTERNAL_SERVER_ERROR, diagnostic.code)
        .unwrap_or_else(|_| fallback_response())
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
        LimitError::InvalidPolicy { .. }
        | LimitError::RequestTarget { .. }
        | LimitError::InvalidContentLength => StatusCode::BAD_REQUEST,
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
    InvalidIdentity(ScopeError),
    MissingHandler(String),
    UnknownHandler(String),
}

impl Display for RuntimeBuildError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimits(error) => Display::fmt(error, formatter),
            Self::InvalidIdentity(error) => Display::fmt(error, formatter),
            Self::MissingHandler(route) => {
                write!(formatter, "route {route} has no runtime handler")
            }
            Self::UnknownHandler(route) => {
                write!(formatter, "handler references unknown route {route}")
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
