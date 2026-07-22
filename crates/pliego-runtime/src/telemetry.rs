// SPDX-License-Identifier: Apache-2.0

use crate::{CancelReason, RenderMode, RequestOutcome, RuntimeReceipt};
use http::{HeaderMap, Request, Version};
use opentelemetry::global::{self, BoxedSpan, BoxedTracer};
use opentelemetry::metrics::{Histogram, UpDownCounter};
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::{Span, SpanKind, Status, Tracer};
use opentelemetry::{Context as OpenTelemetryContext, InstrumentationScope, KeyValue};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use pliego_router::RouteMatch;
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Instant, SystemTime};

const INSTRUMENTATION_NAME: &str = "dev.pliegors.runtime";
const SEMANTIC_CONVENTION_SCHEMA: &str = "https://opentelemetry.io/schemas/1.43.0";
const RECEIPT_CONTRACT: &str = "dev.pliegors.runtime-receipt/v1";
const MAX_KNOWN_METHODS: usize = 64;
const MAX_METHOD_BYTES: usize = 32;
const HTTP_SERVER_DURATION_BOUNDARIES: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];
const STANDARD_METHODS: &[&str] = &[
    "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT", "QUERY", "TRACE",
];
const FRAMEWORK_DIAGNOSTIC_CODES: &[&str] = &[
    "PLG-REN-001",
    "PLG-REN-002",
    "PLG-REN-003",
    "PLG-REN-004",
    "PLG-REN-005",
    "PLG-REN-006",
    "PLG-REN-007",
    "PLG-REN-008",
    "PLG-REN-201",
    "PLG-REN-202",
    "PLG-REN-203",
    "PLG-REN-204",
    "PLG-REN-205",
    "PLG-REN-206",
    "PLG-REN-207",
    "PLG-REN-208",
    "PLG-REN-209",
    "PLG-REN-210",
    "PLG-RTE-001",
    "PLG-RTE-002",
    "PLG-RTE-003",
    "PLG-RTE-004",
    "PLG-RTE-005",
    "PLG-RTE-006",
    "PLG-RTE-007",
    "PLG-RTE-008",
    "PLG-RTE-009",
    "PLG-RTE-010",
    "PLG-RTE-011",
    "PLG-RTE-012",
    "PLG-RTE-013",
    "PLG-RTE-014",
    "PLG-RTE-101",
    "PLG-RTE-404",
    "PLG-RTE-405",
    "PLG-RUN-001",
    "PLG-RUN-002",
    "PLG-RUN-003",
    "PLG-RUN-101",
    "PLG-RUN-102",
    "PLG-RUN-103",
    "PLG-RUN-104",
    "PLG-RUN-105",
    "PLG-RUN-106",
    "PLG-RUN-107",
    "PLG-RUN-301",
    "PLG-RUN-302",
    "PLG-RUN-303",
    "PLG-RUN-304",
    "PLG-RUN-305",
    "PLG-RUN-306",
    "PLG-RUN-408",
    "PLG-RUN-499",
    "PLG-RUN-500",
    "PLG-RUN-501",
    "PLG-RUN-502",
    "PLG-RUN-503",
    "PLG-RUN-504",
    "PLG-RUN-505",
    "PLG-RUN-506",
    "PLG-RUN-507",
];

/// Controls whether an inbound W3C trace parent may cross the runtime boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RemoteTracePolicy {
    /// Start a new trace regardless of attacker-controlled request headers.
    #[default]
    Ignore,
    /// Accept only a valid W3C `traceparent`; provider state remains local.
    AcceptW3c,
}

/// Trusted public scheme for the HTTP server represented by telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HttpScheme {
    Http,
    Https,
}

impl HttpScheme {
    fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

/// Bounded OpenTelemetry policy for the native runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTelemetryConfig {
    server_scheme: HttpScheme,
    remote_trace_policy: RemoteTracePolicy,
    known_methods: BTreeSet<String>,
}

impl OpenTelemetryConfig {
    /// Create telemetry policy with an operator-owned, trusted public scheme.
    pub fn new(server_scheme: HttpScheme) -> Self {
        Self {
            server_scheme,
            remote_trace_policy: RemoteTracePolicy::Ignore,
            known_methods: STANDARD_METHODS
                .iter()
                .map(|method| (*method).to_owned())
                .collect(),
        }
    }

    pub fn remote_trace_policy(mut self, policy: RemoteTracePolicy) -> Self {
        self.remote_trace_policy = policy;
        self
    }

    /// Admit one additional, exact HTTP method into the low-cardinality set.
    pub fn known_method(
        mut self,
        method: impl Into<String>,
    ) -> Result<Self, OpenTelemetryConfigError> {
        let method = method.into();
        validate_method(&method)?;
        if !self.known_methods.contains(&method) && self.known_methods.len() == MAX_KNOWN_METHODS {
            return Err(OpenTelemetryConfigError::TooManyKnownMethods {
                maximum: MAX_KNOWN_METHODS,
            });
        }
        self.known_methods.insert(method);
        Ok(self)
    }

    pub fn remote_policy(&self) -> RemoteTracePolicy {
        self.remote_trace_policy
    }

    pub fn server_scheme(&self) -> HttpScheme {
        self.server_scheme
    }

    pub fn known_methods(&self) -> &BTreeSet<String> {
        &self.known_methods
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenTelemetryConfigError {
    InvalidKnownMethod(String),
    TooManyKnownMethods { maximum: usize },
}

impl OpenTelemetryConfigError {
    pub fn code(&self) -> &'static str {
        "PLG-OTEL-001"
    }
}

impl Display for OpenTelemetryConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKnownMethod(method) => {
                write!(formatter, "invalid OpenTelemetry known method: {method:?}")
            }
            Self::TooManyKnownMethods { maximum } => write!(
                formatter,
                "OpenTelemetry known method set exceeds maximum {maximum}"
            ),
        }
    }
}

impl std::error::Error for OpenTelemetryConfigError {}

fn validate_method(method: &str) -> Result<(), OpenTelemetryConfigError> {
    if method.is_empty()
        || method.len() > MAX_METHOD_BYTES
        || !method.bytes().all(is_method_token_byte)
    {
        return Err(OpenTelemetryConfigError::InvalidKnownMethod(
            method.to_owned(),
        ));
    }
    Ok(())
}

fn is_method_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

pub(crate) struct OpenTelemetryRuntime {
    tracer: BoxedTracer,
    request_duration: Histogram<f64>,
    active_requests: UpDownCounter<i64>,
    response_body_size: Histogram<u64>,
    config: OpenTelemetryConfig,
}

impl OpenTelemetryRuntime {
    pub(crate) fn from_global(config: OpenTelemetryConfig) -> Self {
        let scope = InstrumentationScope::builder(INSTRUMENTATION_NAME)
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_schema_url(SEMANTIC_CONVENTION_SCHEMA)
            .build();
        let tracer = global::tracer_with_scope(scope.clone());
        let meter = global::meter_with_scope(scope);
        let request_duration = meter
            .f64_histogram("http.server.request.duration")
            .with_description("Duration of inbound HTTP requests through the PliegoRS runtime")
            .with_unit("s")
            .with_boundaries(HTTP_SERVER_DURATION_BOUNDARIES.to_vec())
            .build();
        let active_requests = meter
            .i64_up_down_counter("http.server.active_requests")
            .with_description("Number of HTTP requests active in the PliegoRS runtime")
            .with_unit("{request}")
            .build();
        let response_body_size = meter
            .u64_histogram("http.server.response.body.size")
            .with_description("Size of HTTP response bodies emitted by the PliegoRS runtime")
            .with_unit("By")
            .build();
        Self {
            tracer,
            request_duration,
            active_requests,
            response_body_size,
            config,
        }
    }

    pub(crate) fn start(&self, request: &Request<crate::Body>) -> OpenTelemetryRequest {
        let started = Instant::now();
        let span_started_at = SystemTime::now();
        let method = if self
            .config
            .known_methods
            .contains(request.method().as_str())
        {
            request.method().as_str().to_owned()
        } else {
            "_OTHER".to_owned()
        };
        let protocol = protocol_version(request.version()).to_owned();
        let scheme = self.config.server_scheme.as_str().to_owned();
        let active_attributes = vec![
            KeyValue::new("http.request.method", method.clone()),
            KeyValue::new("network.protocol.version", protocol.clone()),
            KeyValue::new("url.scheme", scheme.clone()),
        ];
        let parent = match self.config.remote_trace_policy {
            RemoteTracePolicy::Ignore => OpenTelemetryContext::new(),
            RemoteTracePolicy::AcceptW3c => {
                TraceContextPropagator::new().extract(&W3cTraceParent(request.headers()))
            }
        };
        let span_method = span_method_name(&method).to_owned();
        let span = self
            .tracer
            .span_builder(span_method.clone())
            .with_kind(SpanKind::Server)
            .with_start_time(span_started_at)
            .with_attributes([
                KeyValue::new("http.request.method", method.clone()),
                KeyValue::new("network.protocol.version", protocol.clone()),
                KeyValue::new("pliego.runtime.receipt", RECEIPT_CONTRACT),
                KeyValue::new("url.scheme", scheme.clone()),
            ])
            .start_with_context(&self.tracer, &parent);
        self.active_requests.add(1, &active_attributes);

        OpenTelemetryRequest {
            span: Mutex::new(Some(span)),
            route: Mutex::new(None),
            started,
            span_started_at,
            method,
            span_method,
            protocol,
            scheme,
            active_attributes,
            request_duration: self.request_duration.clone(),
            active_requests: self.active_requests.clone(),
            response_body_size: self.response_body_size.clone(),
            finished: AtomicBool::new(false),
        }
    }
}

pub(crate) struct OpenTelemetryRequest {
    span: Mutex<Option<BoxedSpan>>,
    route: Mutex<Option<TelemetryRoute>>,
    started: Instant,
    span_started_at: SystemTime,
    method: String,
    span_method: String,
    protocol: String,
    scheme: String,
    active_attributes: Vec<KeyValue>,
    request_duration: Histogram<f64>,
    active_requests: UpDownCounter<i64>,
    response_body_size: Histogram<u64>,
    finished: AtomicBool,
}

impl OpenTelemetryRequest {
    pub(crate) fn set_route(&self, route: &RouteMatch) {
        let resolved = TelemetryRoute {
            id: route.route_id().to_owned(),
            pattern: route.pattern().to_owned(),
        };
        *lock(&self.route) = Some(resolved.clone());
        if let Some(span) = lock(&self.span).as_mut() {
            span.update_name(format!("{} {}", self.span_method, resolved.pattern));
            span.set_attribute(KeyValue::new("http.route", resolved.pattern));
            span.set_attribute(KeyValue::new("pliego.route.id", resolved.id));
        }
    }

    pub(crate) fn finish(&self, receipt: &RuntimeReceipt) {
        if self.finished.swap(true, Ordering::AcqRel) {
            return;
        }

        let elapsed = self.started.elapsed();
        let span_finished_at = self.span_started_at + elapsed;
        self.active_requests.add(-1, &self.active_attributes);
        let mut metric_attributes = vec![
            KeyValue::new("http.request.method", self.method.clone()),
            KeyValue::new("network.protocol.version", self.protocol.clone()),
            KeyValue::new("url.scheme", self.scheme.clone()),
        ];
        if let Some(route) = lock(&self.route).clone() {
            metric_attributes.push(KeyValue::new("http.route", route.pattern));
        }
        if let Some(status) = receipt.response_status {
            metric_attributes.push(KeyValue::new(
                "http.response.status_code",
                i64::from(status),
            ));
        }
        if let Some(error_type) = error_type(receipt) {
            metric_attributes.push(KeyValue::new("error.type", error_type));
        }
        self.request_duration
            .record(elapsed.as_secs_f64(), &metric_attributes);
        self.response_body_size
            .record(receipt.response_bytes, &metric_attributes);

        let Some(mut span) = lock(&self.span).take() else {
            return;
        };
        if let Some(status) = receipt.response_status {
            span.set_attribute(KeyValue::new(
                "http.response.status_code",
                i64::from(status),
            ));
        }
        span.set_attribute(KeyValue::new(
            "http.response.body.size",
            i64::try_from(receipt.response_bytes).unwrap_or(i64::MAX),
        ));
        span.set_attribute(KeyValue::new(
            "pliego.runtime.outcome",
            outcome_name(&receipt.outcome),
        ));
        if let Some(mode) = receipt.render_mode {
            span.set_attribute(KeyValue::new("pliego.render.mode", render_mode_name(mode)));
        }
        for diagnostic in &receipt.diagnostics {
            span.add_event(
                "pliego.runtime.diagnostic",
                vec![KeyValue::new(
                    "pliego.diagnostic.code",
                    bounded_diagnostic_code(&diagnostic.code),
                )],
            );
        }
        if let Some(error_type) = error_type(receipt) {
            span.set_attribute(KeyValue::new("error.type", error_type));
            span.set_status(Status::error(""));
        }
        span.end_with_timestamp(span_finished_at);
    }
}

#[derive(Clone)]
struct TelemetryRoute {
    id: String,
    pattern: String,
}

fn error_type(receipt: &RuntimeReceipt) -> Option<String> {
    if receipt.outcome == RequestOutcome::Cancelled {
        return match receipt.cancel_reason.as_ref() {
            Some(CancelReason::ClientDisconnect) => None,
            Some(reason) => Some(cancel_reason_name(reason).to_owned()),
            None => Some("_OTHER".to_owned()),
        };
    }
    let server_failure = receipt.response_status.is_some_and(|status| status >= 500);
    if receipt.outcome == RequestOutcome::Failed || server_failure {
        return receipt
            .diagnostics
            .first()
            .map(|diagnostic| bounded_diagnostic_code(&diagnostic.code).to_owned())
            .or_else(|| receipt.response_status.map(|status| status.to_string()))
            .or_else(|| Some("_OTHER".to_owned()));
    }
    None
}

fn bounded_diagnostic_code(code: &str) -> &'static str {
    FRAMEWORK_DIAGNOSTIC_CODES
        .iter()
        .copied()
        .find(|candidate| *candidate == code)
        .unwrap_or("_OTHER")
}

fn cancel_reason_name(reason: &CancelReason) -> &'static str {
    match reason {
        CancelReason::ClientDisconnect => "client-disconnect",
        CancelReason::Deadline => "timeout",
        CancelReason::Shutdown => "shutdown",
        CancelReason::ApplicationAbort => "application-abort",
        CancelReason::RequestBodyLimit => "request-body-limit",
        CancelReason::ResponseBodyLimit => "response-body-limit",
    }
}

fn outcome_name(outcome: &RequestOutcome) -> &'static str {
    match outcome {
        RequestOutcome::Pending => "pending",
        RequestOutcome::Success => "success",
        RequestOutcome::Rejected => "rejected",
        RequestOutcome::Cancelled => "cancelled",
        RequestOutcome::Failed => "failed",
    }
}

fn render_mode_name(mode: RenderMode) -> &'static str {
    match mode {
        RenderMode::Complete => "complete",
        RenderMode::Ordered => "ordered",
        RenderMode::Boundary => "boundary",
        RenderMode::Layout => "layout",
    }
}

fn protocol_version(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "0.9",
        Version::HTTP_10 => "1.0",
        Version::HTTP_11 => "1.1",
        Version::HTTP_2 => "2",
        Version::HTTP_3 => "3",
        _ => "_OTHER",
    }
}

fn span_method_name(method: &str) -> &str {
    if method == "_OTHER" { "HTTP" } else { method }
}

struct W3cTraceParent<'a>(&'a HeaderMap);

impl Extractor for W3cTraceParent<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        if key != "traceparent" {
            return None;
        }
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        ["traceparent"]
            .into_iter()
            .filter(|key| self.0.contains_key(*key))
            .collect()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_bounds_known_methods_and_defaults_to_new_traces() {
        let config = OpenTelemetryConfig::new(HttpScheme::Https)
            .known_method("PROPFIND")
            .unwrap();
        assert_eq!(config.remote_policy(), RemoteTracePolicy::Ignore);
        assert_eq!(config.server_scheme(), HttpScheme::Https);
        assert!(config.known_methods().contains("GET"));
        assert!(config.known_methods().contains("PROPFIND"));
        assert!(matches!(
            OpenTelemetryConfig::new(HttpScheme::Http).known_method("bad method"),
            Err(OpenTelemetryConfigError::InvalidKnownMethod(ref method)) if method == "bad method"
        ));
        assert_eq!(bounded_diagnostic_code("PLG-RUN-500"), "PLG-RUN-500");
        assert_eq!(bounded_diagnostic_code("APP-SECRET-999"), "_OTHER");

        let mut bounded = OpenTelemetryConfig::new(HttpScheme::Http);
        for index in 0..(MAX_KNOWN_METHODS - STANDARD_METHODS.len()) {
            bounded = bounded.known_method(format!("CUSTOM{index}")).unwrap();
        }
        assert!(matches!(
            bounded.known_method("ONE-TOO-MANY"),
            Err(OpenTelemetryConfigError::TooManyKnownMethods {
                maximum: MAX_KNOWN_METHODS
            })
        ));
        assert_eq!(span_method_name("GET"), "GET");
        assert_eq!(span_method_name("_OTHER"), "HTTP");
    }
}
