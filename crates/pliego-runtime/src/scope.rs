// SPDX-License-Identifier: Apache-2.0

use crate::{RenderMode, RequestLimits};
use pliego_router::RouteMatch;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

type Cleanup = Box<dyn FnOnce(Option<CancelReason>) -> Result<(), String> + Send + 'static>;
type InternalCleanup = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestState {
    Accepted,
    HeadAdmitted,
    RouteResolved,
    ScopeOpen,
    HandlerRunning,
    ResponseCommitted,
    BodyStreaming,
    Rejected,
    Cancelled,
    Failed,
    ScopeDraining,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CancelReason {
    ClientDisconnect,
    Deadline,
    Shutdown,
    ApplicationAbort,
    RequestBodyLimit,
    ResponseBodyLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestOutcome {
    Pending,
    Success,
    Rejected,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeDiagnostic {
    pub code: String,
    pub message: String,
}

impl RuntimeDiagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Result<Self, ScopeError> {
        let code = code.into();
        let message = message.into();
        if code.is_empty()
            || code.len() > 32
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(ScopeError::InvalidDiagnosticCode(code));
        }
        if message.is_empty()
            || message.len() > 512
            || message
                .chars()
                .any(|character| matches!(character, '\r' | '\n' | '\0'))
        {
            return Err(ScopeError::InvalidDiagnosticMessage);
        }
        Ok(Self { code, message })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestIdentity {
    pub request_id: String,
    pub deployment_id: String,
}

impl RequestIdentity {
    pub fn new(
        request_id: impl Into<String>,
        deployment_id: impl Into<String>,
    ) -> Result<Self, ScopeError> {
        let request_id = request_id.into();
        let deployment_id = deployment_id.into();
        validate_identity("request_id", &request_id)?;
        validate_identity("deployment_id", &deployment_id)?;
        Ok(Self {
            request_id,
            deployment_id,
        })
    }
}

fn validate_identity(field: &'static str, value: &str) -> Result<(), ScopeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(ScopeError::InvalidIdentity {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

#[derive(Clone)]
pub struct RequestContext {
    scope: RequestScope,
    route: RouteMatch,
}

#[derive(Clone)]
pub struct PreRouteContext {
    scope: RequestScope,
}

impl PreRouteContext {
    pub(crate) fn new(scope: RequestScope) -> Self {
        Self { scope }
    }

    pub fn scope(&self) -> &RequestScope {
        &self.scope
    }
}

impl RequestContext {
    pub(crate) fn new(scope: RequestScope, route: RouteMatch) -> Self {
        Self { scope, route }
    }

    pub fn scope(&self) -> &RequestScope {
        &self.scope
    }

    pub fn route(&self) -> &RouteMatch {
        &self.route
    }

    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.route.parameter(name)
    }
}

pub trait RuntimeReceiptSink: Send + Sync + 'static {
    fn record(&self, receipt: RuntimeReceipt);
}

impl<F> RuntimeReceiptSink for F
where
    F: Fn(RuntimeReceipt) + Send + Sync + 'static,
{
    fn record(&self, receipt: RuntimeReceipt) {
        self(receipt);
    }
}

#[derive(Clone, Default)]
pub struct InMemoryReceiptSink {
    receipts: Arc<Mutex<Vec<RuntimeReceipt>>>,
}

impl InMemoryReceiptSink {
    pub fn receipts(&self) -> Vec<RuntimeReceipt> {
        lock(&self.receipts).clone()
    }
}

impl RuntimeReceiptSink for InMemoryReceiptSink {
    fn record(&self, receipt: RuntimeReceipt) {
        lock(&self.receipts).push(receipt);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeReceipt {
    pub contract: String,
    pub request_id: String,
    pub deployment_id: String,
    pub route_id: Option<String>,
    pub route_scopes: Vec<String>,
    pub limit_policy_sha256: String,
    pub outcome: RequestOutcome,
    pub final_state: RequestState,
    pub cancel_reason: Option<CancelReason>,
    pub response_status: Option<u16>,
    pub response_bytes: u64,
    pub render_mode: Option<RenderMode>,
    pub middleware: Vec<String>,
    pub error_boundary: Option<String>,
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

struct ScopeInner {
    identity: RequestIdentity,
    limit_policy_sha256: String,
    limits: RequestLimits,
    deadline: Instant,
    state: Mutex<RequestState>,
    outcome: Mutex<RequestOutcome>,
    route_id: Mutex<Option<String>>,
    route_scopes: Mutex<Vec<String>>,
    cancel_reason: Mutex<Option<CancelReason>>,
    cancellation: CancellationToken,
    completion: CancellationToken,
    cleanups: Mutex<Vec<Cleanup>>,
    internal_cleanups: Mutex<Vec<InternalCleanup>>,
    diagnostics: Mutex<Vec<RuntimeDiagnostic>>,
    response_status: Mutex<Option<u16>>,
    render_mode: Mutex<Option<RenderMode>>,
    middleware: Mutex<Vec<String>>,
    error_boundary: Mutex<Option<String>>,
    response_bytes: AtomicU64,
    receipt_recorded: AtomicBool,
    sink: Arc<dyn RuntimeReceiptSink>,
}

#[derive(Clone)]
pub struct RequestScope {
    inner: Arc<ScopeInner>,
}

impl RequestScope {
    pub(crate) fn open(
        identity: RequestIdentity,
        limits: RequestLimits,
        sink: Arc<dyn RuntimeReceiptSink>,
    ) -> Self {
        let deadline = Instant::now() + limits.deadline();
        let scope = Self {
            inner: Arc::new(ScopeInner {
                identity,
                limit_policy_sha256: limits.digest(),
                limits,
                deadline,
                state: Mutex::new(RequestState::Accepted),
                outcome: Mutex::new(RequestOutcome::Pending),
                route_id: Mutex::new(None),
                route_scopes: Mutex::new(Vec::new()),
                cancel_reason: Mutex::new(None),
                cancellation: CancellationToken::new(),
                completion: CancellationToken::new(),
                cleanups: Mutex::new(Vec::new()),
                internal_cleanups: Mutex::new(Vec::new()),
                diagnostics: Mutex::new(Vec::new()),
                response_status: Mutex::new(None),
                render_mode: Mutex::new(None),
                middleware: Mutex::new(Vec::new()),
                error_boundary: Mutex::new(None),
                response_bytes: AtomicU64::new(0),
                receipt_recorded: AtomicBool::new(false),
                sink,
            }),
        };
        scope.spawn_deadline();
        scope
    }

    fn spawn_deadline(&self) {
        let scope = self.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::from_std(scope.inner.deadline);
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    scope.cancel(CancelReason::Deadline);
                }
                _ = scope.inner.completion.cancelled() => {}
            }
        });
    }

    pub fn identity(&self) -> &RequestIdentity {
        &self.inner.identity
    }

    pub fn state(&self) -> RequestState {
        lock(&self.inner.state).clone()
    }

    pub fn outcome(&self) -> RequestOutcome {
        lock(&self.inner.outcome).clone()
    }

    pub fn deadline(&self) -> Instant {
        self.inner.deadline
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancellation.is_cancelled()
    }

    pub fn cancel_reason(&self) -> Option<CancelReason> {
        lock(&self.inner.cancel_reason).clone()
    }

    pub fn register_cleanup<F>(&self, cleanup: F) -> Result<(), ScopeError>
    where
        F: FnOnce(Option<CancelReason>) -> Result<(), String> + Send + 'static,
    {
        let mut cleanups = lock(&self.inner.cleanups);
        if matches!(
            self.state(),
            RequestState::ScopeDraining | RequestState::Closed
        ) {
            return Err(ScopeError::ScopeClosed);
        }
        if cleanups.len() >= self.inner.limits.max_cleanups {
            return Err(ScopeError::CleanupLimit(self.inner.limits.max_cleanups));
        }
        cleanups.push(Box::new(cleanup));
        Ok(())
    }

    pub fn diagnostics(&self) -> Vec<RuntimeDiagnostic> {
        lock(&self.inner.diagnostics).clone()
    }

    pub(crate) fn register_internal_cleanup<F>(&self, cleanup: F) -> Result<(), ScopeError>
    where
        F: FnOnce() + Send + 'static,
    {
        let mut cleanups = lock(&self.inner.internal_cleanups);
        if matches!(
            self.state(),
            RequestState::ScopeDraining | RequestState::Closed
        ) {
            return Err(ScopeError::ScopeClosed);
        }
        cleanups.push(Box::new(cleanup));
        Ok(())
    }

    pub(crate) fn transition(&self, next: RequestState) -> Result<(), ScopeError> {
        let mut state = lock(&self.inner.state);
        if !transition_allowed(&state, &next) {
            return Err(ScopeError::InvalidTransition {
                from: state.clone(),
                to: next,
            });
        }
        *state = next;
        Ok(())
    }

    pub(crate) fn set_route(&self, route: &RouteMatch) {
        *lock(&self.inner.route_id) = Some(route.route_id().to_owned());
        *lock(&self.inner.route_scopes) = route.scope_ids().to_vec();
    }

    pub(crate) fn commit_response(&self, status: u16) -> Result<(), ScopeError> {
        self.transition(RequestState::ResponseCommitted)?;
        *lock(&self.inner.response_status) = Some(status);
        Ok(())
    }

    pub(crate) fn set_render_mode(&self, mode: RenderMode) {
        *lock(&self.inner.render_mode) = Some(mode);
    }

    pub(crate) fn record_middleware(&self, id: &str) {
        lock(&self.inner.middleware).push(id.to_owned());
    }

    pub(crate) fn set_error_boundary(&self, id: &str) {
        *lock(&self.inner.error_boundary) = Some(id.to_owned());
    }

    pub(crate) fn add_response_bytes(&self, bytes: usize) -> Result<(), ScopeError> {
        let previous = self
            .inner
            .response_bytes
            .fetch_add(bytes as u64, Ordering::AcqRel);
        let total = previous.saturating_add(bytes as u64);
        if total > self.inner.limits.max_response_bytes as u64 {
            self.push_internal_diagnostic(
                "PLG-RUN-105",
                format!(
                    "response body reached {total} bytes; maximum is {}",
                    self.inner.limits.max_response_bytes
                ),
            );
            self.cancel(CancelReason::ResponseBodyLimit);
            return Err(ScopeError::ResponseBodyLimit {
                actual: total,
                maximum: self.inner.limits.max_response_bytes as u64,
            });
        }
        Ok(())
    }

    pub(crate) fn reject(&self, diagnostic: RuntimeDiagnostic) {
        self.push_diagnostic(diagnostic);
        let mut outcome = lock(&self.inner.outcome);
        if *outcome == RequestOutcome::Pending {
            *outcome = RequestOutcome::Rejected;
        }
        drop(outcome);
        let _ = self.transition(RequestState::Rejected);
    }

    pub(crate) fn fail(&self, diagnostic: RuntimeDiagnostic) {
        self.push_diagnostic(diagnostic);
        let mut outcome = lock(&self.inner.outcome);
        if *outcome == RequestOutcome::Pending {
            *outcome = RequestOutcome::Failed;
        }
        drop(outcome);
        let _ = self.transition(RequestState::Failed);
    }

    pub(crate) fn cancel(&self, reason: CancelReason) {
        if matches!(
            self.state(),
            RequestState::ScopeDraining | RequestState::Closed
        ) {
            return;
        }
        let mut current_reason = lock(&self.inner.cancel_reason);
        if current_reason.is_none() {
            *current_reason = Some(reason);
        }
        drop(current_reason);
        let mut outcome = lock(&self.inner.outcome);
        if *outcome == RequestOutcome::Pending {
            *outcome = RequestOutcome::Cancelled;
        }
        drop(outcome);
        self.inner.cancellation.cancel();
        if !matches!(self.state(), RequestState::Rejected | RequestState::Failed) {
            let _ = self.transition(RequestState::Cancelled);
        }
    }

    pub(crate) fn complete_success(&self) {
        if self.outcome() == RequestOutcome::Pending {
            *lock(&self.inner.outcome) = RequestOutcome::Success;
        }
        self.drain_and_close();
    }

    pub(crate) fn drain_and_close(&self) {
        if self.state() == RequestState::Closed {
            return;
        }
        let _ = self.transition(RequestState::ScopeDraining);
        let reason = self.cancel_reason();
        let mut cleanups = {
            let mut registered = lock(&self.inner.cleanups);
            std::mem::take(&mut *registered)
        };
        while let Some(cleanup) = cleanups.pop() {
            match catch_unwind(AssertUnwindSafe(|| cleanup(reason.clone()))) {
                Ok(Ok(())) => {}
                Ok(Err(message)) => self.push_internal_diagnostic(
                    "PLG-RUN-302",
                    format!("request cleanup failed: {}", bounded(&message, 320)),
                ),
                Err(_) => self
                    .push_internal_diagnostic("PLG-RUN-303", "request cleanup panicked".to_owned()),
            }
        }
        let mut internal_cleanups = {
            let mut registered = lock(&self.inner.internal_cleanups);
            std::mem::take(&mut *registered)
        };
        while let Some(cleanup) = internal_cleanups.pop() {
            if catch_unwind(AssertUnwindSafe(cleanup)).is_err() {
                self.push_internal_diagnostic(
                    "PLG-RUN-306",
                    "internal request cleanup panicked".to_owned(),
                );
            }
        }
        let _ = self.transition(RequestState::Closed);
        self.inner.completion.cancel();
        self.record_receipt_once();
    }

    pub fn receipt(&self) -> RuntimeReceipt {
        RuntimeReceipt {
            contract: "dev.pliegors.runtime-receipt/v1".to_owned(),
            request_id: self.inner.identity.request_id.clone(),
            deployment_id: self.inner.identity.deployment_id.clone(),
            route_id: lock(&self.inner.route_id).clone(),
            route_scopes: lock(&self.inner.route_scopes).clone(),
            limit_policy_sha256: self.inner.limit_policy_sha256.clone(),
            outcome: self.outcome(),
            final_state: self.state(),
            cancel_reason: self.cancel_reason(),
            response_status: *lock(&self.inner.response_status),
            response_bytes: self.inner.response_bytes.load(Ordering::Acquire),
            render_mode: *lock(&self.inner.render_mode),
            middleware: lock(&self.inner.middleware).clone(),
            error_boundary: lock(&self.inner.error_boundary).clone(),
            diagnostics: self.diagnostics(),
        }
    }

    fn push_diagnostic(&self, diagnostic: RuntimeDiagnostic) {
        let mut diagnostics = lock(&self.inner.diagnostics);
        if diagnostics.len() < self.inner.limits.max_diagnostics {
            diagnostics.push(diagnostic);
        }
    }

    fn push_internal_diagnostic(&self, code: &str, message: String) {
        if let Ok(diagnostic) = RuntimeDiagnostic::new(code, message) {
            self.push_diagnostic(diagnostic);
        }
    }

    fn record_receipt_once(&self) {
        if !self.inner.receipt_recorded.swap(true, Ordering::AcqRel) {
            self.inner.sink.record(self.receipt());
        }
    }
}

fn transition_allowed(from: &RequestState, to: &RequestState) -> bool {
    use RequestState::*;
    matches!(
        (from, to),
        (Accepted, HeadAdmitted | Rejected | Cancelled | Failed)
            | (
                HeadAdmitted,
                RouteResolved | ResponseCommitted | Rejected | Cancelled | Failed
            )
            | (RouteResolved, ScopeOpen | Rejected | Cancelled | Failed)
            | (ScopeOpen, HandlerRunning | Rejected | Cancelled | Failed)
            | (HandlerRunning, ResponseCommitted | Cancelled | Failed)
            | (
                ResponseCommitted,
                BodyStreaming | ScopeDraining | Cancelled | Failed
            )
            | (BodyStreaming, ScopeDraining | Cancelled | Failed)
            | (
                Rejected | Cancelled | Failed,
                ResponseCommitted | ScopeDraining
            )
            | (ScopeDraining, Closed)
    )
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeError {
    InvalidIdentity {
        field: &'static str,
        value: String,
    },
    InvalidDiagnosticCode(String),
    InvalidDiagnosticMessage,
    InvalidTransition {
        from: RequestState,
        to: RequestState,
    },
    ScopeClosed,
    CleanupLimit(usize),
    ResponseBodyLimit {
        actual: u64,
        maximum: u64,
    },
}

impl ScopeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentity { .. } => "PLG-RUN-002",
            Self::InvalidDiagnosticCode(_) | Self::InvalidDiagnosticMessage => "PLG-RUN-003",
            Self::InvalidTransition { .. } => "PLG-RUN-301",
            Self::ScopeClosed => "PLG-RUN-304",
            Self::CleanupLimit(_) => "PLG-RUN-305",
            Self::ResponseBodyLimit { .. } => "PLG-RUN-105",
        }
    }
}

impl Display for ScopeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentity { field, value } => {
                write!(formatter, "invalid {field} {value:?}")
            }
            Self::InvalidDiagnosticCode(value) => {
                write!(formatter, "invalid diagnostic code {value:?}")
            }
            Self::InvalidDiagnosticMessage => formatter.write_str("invalid diagnostic message"),
            Self::InvalidTransition { from, to } => {
                write!(formatter, "invalid request transition {from:?} -> {to:?}")
            }
            Self::ScopeClosed => formatter.write_str("request scope is already draining or closed"),
            Self::CleanupLimit(maximum) => {
                write!(formatter, "request cleanup limit reached: {maximum}")
            }
            Self::ResponseBodyLimit { actual, maximum } => write!(
                formatter,
                "response body reached {actual} bytes; maximum is {maximum}"
            ),
        }
    }
}

impl std::error::Error for ScopeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn scope(limits: RequestLimits, sink: Arc<dyn RuntimeReceiptSink>) -> RequestScope {
        RequestScope::open(
            RequestIdentity::new("request-1", "deployment-1").unwrap(),
            limits,
            sink,
        )
    }

    #[tokio::test]
    async fn cleanup_is_lifo_and_receipt_is_recorded_once() {
        let sink = InMemoryReceiptSink::default();
        let output = Arc::new(Mutex::new(Vec::new()));
        let request = scope(RequestLimits::default(), Arc::new(sink.clone()));
        request.transition(RequestState::HeadAdmitted).unwrap();
        request.transition(RequestState::RouteResolved).unwrap();
        request.transition(RequestState::ScopeOpen).unwrap();
        request.transition(RequestState::HandlerRunning).unwrap();
        request.commit_response(200).unwrap();
        for value in [1, 2, 3] {
            let output = output.clone();
            request
                .register_cleanup(move |_| {
                    lock(&output).push(value);
                    Ok(())
                })
                .unwrap();
        }
        request.complete_success();
        request.complete_success();
        assert_eq!(*lock(&output), vec![3, 2, 1]);
        assert_eq!(sink.receipts().len(), 1);
        assert_eq!(sink.receipts()[0].outcome, RequestOutcome::Success);
    }

    #[tokio::test]
    async fn deadline_cancels_open_scope() {
        let limits = RequestLimits {
            deadline_ms: 10,
            ..RequestLimits::default()
        };
        let request = scope(limits, Arc::new(InMemoryReceiptSink::default()));
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            request.cancellation_token().cancelled(),
        )
        .await
        .expect("deadline cancellation should be observable");
        assert!(request.is_cancelled());
        assert_eq!(request.cancel_reason(), Some(CancelReason::Deadline));
        request.drain_and_close();
    }

    #[tokio::test]
    async fn cleanup_failure_is_bounded_diagnostic() {
        let request = scope(
            RequestLimits::default(),
            Arc::new(InMemoryReceiptSink::default()),
        );
        request
            .register_cleanup(|_| Err("failure".repeat(100)))
            .unwrap();
        request.cancel(CancelReason::ApplicationAbort);
        request.drain_and_close();
        assert_eq!(request.diagnostics().len(), 1);
        assert_eq!(request.diagnostics()[0].code, "PLG-RUN-302");
        assert!(request.diagnostics()[0].message.len() <= 512);
    }

    #[test]
    fn diagnostic_and_identity_reject_control_or_unbounded_values() {
        assert!(RuntimeDiagnostic::new("bad", "message").is_err());
        assert!(RuntimeDiagnostic::new("PLG-RUN-001", "line\nbreak").is_err());
        assert!(RequestIdentity::new("bad request", "deployment").is_err());
    }
}
