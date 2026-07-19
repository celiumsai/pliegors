// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Sealed, deterministic route graphs for PliegoRS.
//!
//! The graph owns portable path grammar, collision admission, method dispatch,
//! typed parameter names, deterministic precedence, and a stable digest. It is
//! deliberately independent from Axum and any deployment provider.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

pub const MAX_PATTERN_BYTES: usize = 1_024;
pub const MAX_ROUTE_SEGMENTS: usize = 32;
pub const MAX_ROUTE_ID_BYTES: usize = 64;
pub const MAX_PARAMETER_NAME_BYTES: usize = 63;
pub const MAX_ROUTE_MIDDLEWARE: usize = 32;
pub const MAX_ERROR_BOUNDARIES: usize = 16;
pub const DEFAULT_MAX_ROUTES: usize = 4_096;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RouteMethod(String);

impl RouteMethod {
    pub fn new(value: impl Into<String>) -> Result<Self, RouteError> {
        let value = value.into();
        if value.is_empty() || value.len() > 32 {
            return Err(RouteError::InvalidMethod(value));
        }
        if !value.bytes().all(is_method_token)
            || value.bytes().any(|byte| byte.is_ascii_lowercase())
        {
            return Err(RouteError::InvalidMethod(value));
        }
        Ok(Self(value))
    }

    pub fn get() -> Self {
        Self("GET".to_owned())
    }

    pub fn post() -> Self {
        Self("POST".to_owned())
    }

    pub fn put() -> Self {
        Self("PUT".to_owned())
    }

    pub fn patch() -> Self {
        Self("PATCH".to_owned())
    }

    pub fn delete() -> Self {
        Self("DELETE".to_owned())
    }

    pub fn head() -> Self {
        Self("HEAD".to_owned())
    }

    pub fn options() -> Self {
        Self("OPTIONS".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for RouteMethod {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RouteMethod {
    type Err = RouteError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

fn is_method_token(byte: u8) -> bool {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Literal(String),
    Parameter(String),
    OptionalParameter(String),
    CatchAll(String),
    Group(String),
}

impl Segment {
    fn consumes_path(&self) -> bool {
        !matches!(self, Self::Group(_))
    }

    fn specificity(&self) -> u8 {
        match self {
            Self::Literal(_) => 4,
            Self::Parameter(_) => 3,
            Self::OptionalParameter(_) => 2,
            Self::CatchAll(_) => 1,
            Self::Group(_) => 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoutePattern {
    authored: String,
    canonical: String,
    segments: Vec<Segment>,
}

impl RoutePattern {
    pub fn parse(authored: impl Into<String>) -> Result<Self, RouteError> {
        let authored = authored.into();
        validate_pattern_envelope(&authored)?;
        if authored == "/" {
            return Ok(Self {
                authored,
                canonical: "/".to_owned(),
                segments: Vec::new(),
            });
        }

        let mut segments = Vec::new();
        let mut canonical = Vec::new();
        let mut parameter_names = BTreeSet::new();
        let raw_segments: Vec<_> = authored[1..].split('/').collect();
        if raw_segments.len() > MAX_ROUTE_SEGMENTS {
            return Err(RouteError::TooManySegments(raw_segments.len()));
        }

        for (index, raw) in raw_segments.iter().enumerate() {
            let segment = parse_segment(raw, index, raw_segments.len())?;
            if let Some(name) = parameter_name(&segment) {
                if !parameter_names.insert(name.to_owned()) {
                    return Err(RouteError::DuplicateParameter(name.to_owned()));
                }
            }
            if segment.consumes_path() {
                canonical.push(render_segment(&segment));
            }
            segments.push(segment);
        }

        let canonical = if canonical.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", canonical.join("/"))
        };
        Ok(Self {
            authored,
            canonical,
            segments,
        })
    }

    pub fn authored(&self) -> &str {
        &self.authored
    }

    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    fn path_segments(&self) -> impl Iterator<Item = &Segment> {
        self.segments
            .iter()
            .filter(|segment| segment.consumes_path())
    }

    fn shape_keys(&self) -> Vec<String> {
        let segments: Vec<_> = self.path_segments().collect();
        let mut full = Vec::with_capacity(segments.len());
        for segment in &segments {
            full.push(match segment {
                Segment::Literal(value) => format!("={}", portable_case_key(value)),
                Segment::Parameter(_) => ":".to_owned(),
                Segment::OptionalParameter(_) => ":".to_owned(),
                Segment::CatchAll(_) => "*".to_owned(),
                Segment::Group(_) => unreachable!("groups do not consume paths"),
            });
        }
        let mut keys = vec![format!("/{}", full.join("/"))];
        if matches!(segments.last(), Some(Segment::OptionalParameter(_))) {
            full.pop();
            keys.push(if full.is_empty() {
                "/".to_owned()
            } else {
                format!("/{}", full.join("/"))
            });
        }
        keys
    }

    fn specificity(&self) -> Vec<u8> {
        self.path_segments().map(Segment::specificity).collect()
    }

    fn match_admitted(&self, path: &str) -> Option<BTreeMap<String, String>> {
        let input: Vec<&str> = if path == "/" {
            Vec::new()
        } else {
            path[1..].split('/').collect()
        };
        let segments: Vec<_> = self.path_segments().collect();
        let mut parameters = BTreeMap::new();
        let mut input_index = 0usize;

        for (pattern_index, segment) in segments.iter().enumerate() {
            match segment {
                Segment::Literal(expected) => {
                    if input.get(input_index).copied() != Some(expected.as_str()) {
                        return None;
                    }
                    input_index += 1;
                }
                Segment::Parameter(name) => {
                    let value = input.get(input_index)?;
                    parameters.insert(name.clone(), (*value).to_owned());
                    input_index += 1;
                }
                Segment::OptionalParameter(name) => {
                    if let Some(value) = input.get(input_index) {
                        parameters.insert(name.clone(), (*value).to_owned());
                        input_index += 1;
                    }
                }
                Segment::CatchAll(name) => {
                    if pattern_index + 1 != segments.len() || input_index >= input.len() {
                        return None;
                    }
                    parameters.insert(name.clone(), input[input_index..].join("/"));
                    input_index = input.len();
                }
                Segment::Group(_) => unreachable!("groups do not consume paths"),
            }
        }

        (input_index == input.len()).then_some(parameters)
    }
}

fn validate_pattern_envelope(pattern: &str) -> Result<(), RouteError> {
    if pattern.is_empty() || pattern.len() > MAX_PATTERN_BYTES || !pattern.starts_with('/') {
        return Err(RouteError::InvalidPattern(pattern.to_owned()));
    }
    if pattern != "/" && pattern.ends_with('/') {
        return Err(RouteError::TrailingSlash(pattern.to_owned()));
    }
    if pattern.contains("//")
        || pattern.contains('\\')
        || pattern.contains('%')
        || pattern.contains('#')
        || pattern.contains('\0')
    {
        return Err(RouteError::InvalidPattern(pattern.to_owned()));
    }
    Ok(())
}

fn parse_segment(raw: &str, index: usize, count: usize) -> Result<Segment, RouteError> {
    if raw.is_empty() || raw == "." || raw == ".." {
        return Err(RouteError::InvalidSegment(raw.to_owned()));
    }
    if let Some(name) = raw.strip_prefix(':') {
        if let Some(optional) = name.strip_suffix('?') {
            validate_parameter(optional)?;
            if index + 1 != count {
                return Err(RouteError::OptionalMustBeTerminal(optional.to_owned()));
            }
            return Ok(Segment::OptionalParameter(optional.to_owned()));
        }
        validate_parameter(name)?;
        return Ok(Segment::Parameter(name.to_owned()));
    }
    if let Some(name) = raw.strip_prefix('*') {
        validate_parameter(name)?;
        if index + 1 != count {
            return Err(RouteError::CatchAllMustBeTerminal(name.to_owned()));
        }
        return Ok(Segment::CatchAll(name.to_owned()));
    }
    if raw.starts_with('(') || raw.ends_with(')') {
        let Some(name) = raw
            .strip_prefix('(')
            .and_then(|value| value.strip_suffix(')'))
        else {
            return Err(RouteError::InvalidGroup(raw.to_owned()));
        };
        validate_parameter(name)?;
        return Ok(Segment::Group(name.to_owned()));
    }
    if raw.starts_with(':')
        || raw.starts_with('*')
        || raw.contains('?')
        || raw.contains('(')
        || raw.contains(')')
        || raw
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(RouteError::InvalidSegment(raw.to_owned()));
    }
    let normalized: String = raw.nfc().collect();
    validate_portable_literal(&normalized)?;
    Ok(Segment::Literal(normalized))
}

fn validate_portable_literal(literal: &str) -> Result<(), RouteError> {
    if literal.is_empty()
        || literal.len() > 255
        || literal.ends_with(['.', ' '])
        || literal.chars().any(|character| {
            character.is_control()
                || matches!(character, '\0' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return Err(RouteError::InvalidSegment(literal.to_owned()));
    }
    let stem = literal.split('.').next().unwrap_or(literal);
    let reserved = portable_case_key(stem);
    if matches!(reserved.as_str(), "con" | "prn" | "aux" | "nul")
        || reserved.strip_prefix("com").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || reserved.strip_prefix("lpt").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
    {
        return Err(RouteError::InvalidSegment(literal.to_owned()));
    }
    Ok(())
}

fn portable_case_key(value: &str) -> String {
    value.nfkc().case_fold().nfkc().collect()
}

fn validate_parameter(name: &str) -> Result<(), RouteError> {
    if name.is_empty() || name.len() > MAX_PARAMETER_NAME_BYTES {
        return Err(RouteError::InvalidParameter(name.to_owned()));
    }
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return Err(RouteError::InvalidParameter(name.to_owned()));
    };
    if !first.is_ascii_lowercase()
        || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(RouteError::InvalidParameter(name.to_owned()));
    }
    Ok(())
}

fn parameter_name(segment: &Segment) -> Option<&str> {
    match segment {
        Segment::Parameter(name) | Segment::OptionalParameter(name) | Segment::CatchAll(name) => {
            Some(name)
        }
        Segment::Literal(_) | Segment::Group(_) => None,
    }
}

fn render_segment(segment: &Segment) -> String {
    match segment {
        Segment::Literal(value) => value.clone(),
        Segment::Parameter(name) => format!(":{name}"),
        Segment::OptionalParameter(name) => format!(":{name}?"),
        Segment::CatchAll(name) => format!("*{name}"),
        Segment::Group(_) => unreachable!("groups are not canonical path segments"),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteSpec {
    id: String,
    method: RouteMethod,
    pattern: RoutePattern,
    middleware: Vec<String>,
    error_boundaries: Vec<String>,
}

impl RouteSpec {
    pub fn new(
        id: impl Into<String>,
        method: RouteMethod,
        pattern: impl Into<String>,
    ) -> Result<Self, RouteError> {
        let id = id.into();
        validate_route_id(&id)?;
        Ok(Self {
            id,
            method,
            pattern: RoutePattern::parse(pattern)?,
            middleware: Vec::new(),
            error_boundaries: Vec::new(),
        })
    }

    pub fn middleware(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_behavior_id(&id, BehaviorKind::Middleware)?;
        if self.middleware.len() >= MAX_ROUTE_MIDDLEWARE {
            return Err(RouteError::TooManyMiddleware(MAX_ROUTE_MIDDLEWARE));
        }
        if self.middleware.contains(&id) {
            return Err(RouteError::DuplicateMiddleware(id));
        }
        self.middleware.push(id);
        Ok(self)
    }

    pub fn error_boundary(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_behavior_id(&id, BehaviorKind::ErrorBoundary)?;
        if self.error_boundaries.len() >= MAX_ERROR_BOUNDARIES {
            return Err(RouteError::TooManyErrorBoundaries(MAX_ERROR_BOUNDARIES));
        }
        if self.error_boundaries.contains(&id) {
            return Err(RouteError::DuplicateErrorBoundary(id));
        }
        self.error_boundaries.push(id);
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn method(&self) -> &RouteMethod {
        &self.method
    }

    pub fn pattern(&self) -> &RoutePattern {
        &self.pattern
    }

    pub fn middleware_ids(&self) -> &[String] {
        &self.middleware
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }
}

#[derive(Clone, Copy)]
enum BehaviorKind {
    Middleware,
    ErrorBoundary,
}

fn validate_behavior_id(id: &str, kind: BehaviorKind) -> Result<(), RouteError> {
    if validate_route_id(id).is_ok() {
        return Ok(());
    }
    match kind {
        BehaviorKind::Middleware => Err(RouteError::InvalidMiddlewareId(id.to_owned())),
        BehaviorKind::ErrorBoundary => Err(RouteError::InvalidErrorBoundaryId(id.to_owned())),
    }
}

fn validate_route_id(id: &str) -> Result<(), RouteError> {
    if id.is_empty() || id.len() > MAX_ROUTE_ID_BYTES {
        return Err(RouteError::InvalidRouteId(id.to_owned()));
    }
    let mut bytes = id.bytes();
    let Some(first) = bytes.next() else {
        return Err(RouteError::InvalidRouteId(id.to_owned()));
    };
    if !first.is_ascii_lowercase()
        || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || id.ends_with('-')
        || id.contains("--")
    {
        return Err(RouteError::InvalidRouteId(id.to_owned()));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct RouteGraphBuilder {
    max_routes: usize,
    routes: Vec<RouteSpec>,
    error_boundaries: Vec<String>,
}

impl Default for RouteGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RouteGraphBuilder {
    pub fn new() -> Self {
        Self {
            max_routes: DEFAULT_MAX_ROUTES,
            routes: Vec::new(),
            error_boundaries: Vec::new(),
        }
    }

    pub fn with_max_routes(max_routes: usize) -> Result<Self, RouteError> {
        if max_routes == 0 || max_routes > DEFAULT_MAX_ROUTES {
            return Err(RouteError::InvalidRouteLimit(max_routes));
        }
        Ok(Self {
            max_routes,
            routes: Vec::new(),
            error_boundaries: Vec::new(),
        })
    }

    pub fn error_boundary(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_behavior_id(&id, BehaviorKind::ErrorBoundary)?;
        if self.error_boundaries.len() >= MAX_ERROR_BOUNDARIES {
            return Err(RouteError::TooManyErrorBoundaries(MAX_ERROR_BOUNDARIES));
        }
        if self.error_boundaries.contains(&id) {
            return Err(RouteError::DuplicateErrorBoundary(id));
        }
        self.error_boundaries.push(id);
        Ok(self)
    }

    pub fn route(mut self, route: RouteSpec) -> Self {
        self.routes.push(route);
        self
    }

    pub fn push(&mut self, route: RouteSpec) {
        self.routes.push(route);
    }

    pub fn seal(mut self) -> Result<RouteGraph, RouteError> {
        if self.routes.len() > self.max_routes {
            return Err(RouteError::TooManyRoutes {
                actual: self.routes.len(),
                maximum: self.max_routes,
            });
        }

        self.routes.sort_by(canonical_route_order);
        let mut ids = BTreeSet::new();
        let mut shapes = BTreeMap::new();
        for route in &self.routes {
            if !ids.insert(route.id.clone()) {
                return Err(RouteError::DuplicateRouteId(route.id.clone()));
            }
            for shape in route.pattern.shape_keys() {
                let key = format!("{} {shape}", route.method);
                if let Some(existing) = shapes.insert(key.clone(), route.id.clone()) {
                    return Err(RouteError::RouteCollision {
                        method: route.method.clone(),
                        shape,
                        first: existing,
                        second: route.id.clone(),
                    });
                }
            }
        }

        let digest = graph_digest(&self.routes, &self.error_boundaries);
        self.routes.sort_by(match_order);
        Ok(RouteGraph {
            routes: self.routes,
            error_boundaries: self.error_boundaries,
            digest,
        })
    }
}

fn canonical_route_order(left: &RouteSpec, right: &RouteSpec) -> Ordering {
    left.method
        .cmp(&right.method)
        .then_with(|| left.pattern.authored.cmp(&right.pattern.authored))
        .then_with(|| left.id.cmp(&right.id))
}

fn match_order(left: &RouteSpec, right: &RouteSpec) -> Ordering {
    right
        .pattern
        .specificity()
        .cmp(&left.pattern.specificity())
        .then_with(|| left.pattern.canonical.cmp(&right.pattern.canonical))
        .then_with(|| left.id.cmp(&right.id))
}

fn graph_digest(routes: &[RouteSpec], error_boundaries: &[String]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-route-graph-v2\0");
    digest_sequence(&mut digest, b"root-error-boundaries", error_boundaries);
    for route in routes {
        for value in [
            route.method.as_str(),
            route.id.as_str(),
            route.pattern.authored(),
            route.pattern.canonical(),
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        digest_sequence(&mut digest, b"middleware", &route.middleware);
        digest_sequence(
            &mut digest,
            b"route-error-boundaries",
            &route.error_boundaries,
        );
    }
    encode_hex(&digest.finalize())
}

fn digest_sequence(digest: &mut Sha256, label: &[u8], values: &[String]) {
    digest.update((label.len() as u64).to_be_bytes());
    digest.update(label);
    digest.update((values.len() as u64).to_be_bytes());
    for value in values {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Clone, Debug)]
pub struct RouteGraph {
    routes: Vec<RouteSpec>,
    error_boundaries: Vec<String>,
    digest: String,
}

impl RouteGraph {
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn routes(&self) -> &[RouteSpec] {
        &self.routes
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }

    pub fn resolve(&self, method: &RouteMethod, path: &str) -> Result<RouteMatch, ResolveError> {
        validate_request_path(path)?;
        let normalized: String = path.nfc().collect();
        validate_request_path(&normalized)?;
        for route in self.routes.iter().filter(|route| &route.method == method) {
            if let Some(parameters) = route.pattern.match_admitted(&normalized) {
                return Ok(RouteMatch {
                    route_id: route.id.clone(),
                    method: route.method.clone(),
                    pattern: route.pattern.canonical.clone(),
                    parameters,
                    middleware: route.middleware.clone(),
                    error_boundaries: route.error_boundaries.clone(),
                });
            }
        }

        let mut allowed = BTreeSet::new();
        for route in &self.routes {
            if route.pattern.match_admitted(&normalized).is_some() {
                allowed.insert(route.method.clone());
            }
        }
        if allowed.is_empty() {
            Err(ResolveError::NotFound)
        } else {
            Err(ResolveError::MethodNotAllowed {
                allowed: allowed.into_iter().collect(),
            })
        }
    }
}

fn validate_request_path(path: &str) -> Result<(), ResolveError> {
    if path.is_empty()
        || path.len() > MAX_PATTERN_BYTES
        || !path.starts_with('/')
        || (path != "/" && path.ends_with('/'))
        || path.contains("//")
        || path.contains('\\')
        || path.contains('%')
        || path.contains('?')
        || path.contains('#')
        || path.contains('\0')
        || path.chars().any(char::is_control)
    {
        return Err(ResolveError::InvalidPath);
    }
    if path != "/"
        && path[1..]
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err(ResolveError::InvalidPath);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteMatch {
    route_id: String,
    method: RouteMethod,
    pattern: String,
    parameters: BTreeMap<String, String>,
    middleware: Vec<String>,
    error_boundaries: Vec<String>,
}

impl RouteMatch {
    pub fn route_id(&self) -> &str {
        &self.route_id
    }

    pub fn method(&self) -> &RouteMethod {
        &self.method
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn parameters(&self) -> &BTreeMap<String, String> {
        &self.parameters
    }

    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters.get(name).map(String::as_str)
    }

    pub fn middleware_ids(&self) -> &[String] {
        &self.middleware
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveError {
    InvalidPath,
    NotFound,
    MethodNotAllowed { allowed: Vec<RouteMethod> },
}

impl ResolveError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath => "PLG-RTE-101",
            Self::NotFound => "PLG-RTE-404",
            Self::MethodNotAllowed { .. } => "PLG-RTE-405",
        }
    }
}

impl Display for ResolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("request path is not canonical"),
            Self::NotFound => formatter.write_str("route not found"),
            Self::MethodNotAllowed { allowed } => write!(
                formatter,
                "method not allowed; allowed: {}",
                allowed
                    .iter()
                    .map(RouteMethod::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteError {
    InvalidMethod(String),
    InvalidPattern(String),
    TrailingSlash(String),
    TooManySegments(usize),
    InvalidSegment(String),
    InvalidParameter(String),
    InvalidGroup(String),
    OptionalMustBeTerminal(String),
    CatchAllMustBeTerminal(String),
    DuplicateParameter(String),
    InvalidRouteId(String),
    InvalidRouteLimit(usize),
    TooManyRoutes {
        actual: usize,
        maximum: usize,
    },
    DuplicateRouteId(String),
    InvalidMiddlewareId(String),
    DuplicateMiddleware(String),
    TooManyMiddleware(usize),
    InvalidErrorBoundaryId(String),
    DuplicateErrorBoundary(String),
    TooManyErrorBoundaries(usize),
    RouteCollision {
        method: RouteMethod,
        shape: String,
        first: String,
        second: String,
    },
}

impl RouteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidMethod(_) => "PLG-RTE-001",
            Self::InvalidPattern(_) | Self::TrailingSlash(_) => "PLG-RTE-002",
            Self::TooManySegments(_) => "PLG-RTE-003",
            Self::InvalidSegment(_) | Self::InvalidParameter(_) | Self::InvalidGroup(_) => {
                "PLG-RTE-004"
            }
            Self::OptionalMustBeTerminal(_) | Self::CatchAllMustBeTerminal(_) => "PLG-RTE-005",
            Self::DuplicateParameter(_) => "PLG-RTE-006",
            Self::InvalidRouteId(_) => "PLG-RTE-007",
            Self::InvalidRouteLimit(_) | Self::TooManyRoutes { .. } => "PLG-RTE-008",
            Self::DuplicateRouteId(_) => "PLG-RTE-009",
            Self::RouteCollision { .. } => "PLG-RTE-010",
            Self::InvalidMiddlewareId(_)
            | Self::DuplicateMiddleware(_)
            | Self::TooManyMiddleware(_) => "PLG-RTE-011",
            Self::InvalidErrorBoundaryId(_)
            | Self::DuplicateErrorBoundary(_)
            | Self::TooManyErrorBoundaries(_) => "PLG-RTE-012",
        }
    }
}

impl Display for RouteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMethod(value) => write!(formatter, "invalid route method {value:?}"),
            Self::InvalidPattern(value) => write!(formatter, "invalid route pattern {value:?}"),
            Self::TrailingSlash(value) => {
                write!(formatter, "route pattern has a trailing slash: {value}")
            }
            Self::TooManySegments(actual) => write!(formatter, "route has {actual} segments"),
            Self::InvalidSegment(value) => write!(formatter, "invalid route segment {value:?}"),
            Self::InvalidParameter(value) => write!(formatter, "invalid route parameter {value:?}"),
            Self::InvalidGroup(value) => write!(formatter, "invalid route group {value:?}"),
            Self::OptionalMustBeTerminal(value) => {
                write!(formatter, "optional parameter must be terminal: {value}")
            }
            Self::CatchAllMustBeTerminal(value) => {
                write!(formatter, "catch-all parameter must be terminal: {value}")
            }
            Self::DuplicateParameter(value) => {
                write!(formatter, "duplicate route parameter: {value}")
            }
            Self::InvalidRouteId(value) => write!(formatter, "invalid route ID {value:?}"),
            Self::InvalidRouteLimit(value) => write!(formatter, "invalid route limit {value}"),
            Self::TooManyRoutes { actual, maximum } => write!(
                formatter,
                "route graph has {actual} routes; maximum is {maximum}"
            ),
            Self::DuplicateRouteId(value) => write!(formatter, "duplicate route ID: {value}"),
            Self::InvalidMiddlewareId(value) => {
                write!(formatter, "invalid middleware ID {value:?}")
            }
            Self::DuplicateMiddleware(value) => {
                write!(formatter, "duplicate middleware ID: {value}")
            }
            Self::TooManyMiddleware(maximum) => {
                write!(formatter, "route exceeds {maximum} middleware entries")
            }
            Self::InvalidErrorBoundaryId(value) => {
                write!(formatter, "invalid error boundary ID {value:?}")
            }
            Self::DuplicateErrorBoundary(value) => {
                write!(formatter, "duplicate error boundary ID: {value}")
            }
            Self::TooManyErrorBoundaries(maximum) => {
                write!(
                    formatter,
                    "graph or route exceeds {maximum} error boundaries"
                )
            }
            Self::RouteCollision {
                method,
                shape,
                first,
                second,
            } => write!(
                formatter,
                "{method} route shape {shape} collides between {first} and {second}"
            ),
        }
    }
}

impl std::error::Error for RouteError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, method: RouteMethod, pattern: &str) -> RouteSpec {
        RouteSpec::new(id, method, pattern).unwrap()
    }

    #[test]
    fn parses_root_literals_parameters_groups_and_catch_all() {
        let pattern = RoutePattern::parse("/(docs)/guide/:section/*rest").unwrap();
        assert_eq!(pattern.canonical(), "/guide/:section/*rest");
        assert_eq!(pattern.segments().len(), 4);
    }

    #[test]
    fn rejects_ambiguous_or_non_canonical_patterns() {
        for pattern in [
            "",
            "guide",
            "/guide/",
            "/guide//item",
            "/../item",
            "/a/%2f",
            "/a\\b",
            "/a?b",
            "/a#b",
            "/:ID",
            "/:id/:id",
            "/:id?/tail",
            "/ *bad",
            "/(broken",
            "/a/*rest/tail",
        ] {
            assert!(
                RoutePattern::parse(pattern).is_err(),
                "accepted {pattern:?}"
            );
        }
    }

    #[test]
    fn validates_uppercase_http_tokens() {
        assert_eq!(RouteMethod::new("PURGE").unwrap().as_str(), "PURGE");
        for method in ["", "get", "BAD METHOD"] {
            assert!(RouteMethod::new(method).is_err());
        }
    }

    #[test]
    fn resolves_parameters_and_catch_all() {
        let graph = RouteGraphBuilder::new()
            .route(route("docs", RouteMethod::get(), "/docs/:section/*rest"))
            .seal()
            .unwrap();
        let matched = graph
            .resolve(&RouteMethod::get(), "/docs/runtime/limits/body")
            .unwrap();
        assert_eq!(matched.route_id(), "docs");
        assert_eq!(matched.parameter("section"), Some("runtime"));
        assert_eq!(matched.parameter("rest"), Some("limits/body"));
    }

    #[test]
    fn optional_terminal_parameter_matches_absent_or_present() {
        let graph = RouteGraphBuilder::new()
            .route(route("archive", RouteMethod::get(), "/archive/:year?"))
            .seal()
            .unwrap();
        assert_eq!(
            graph
                .resolve(&RouteMethod::get(), "/archive")
                .unwrap()
                .parameter("year"),
            None
        );
        assert_eq!(
            graph
                .resolve(&RouteMethod::get(), "/archive/2026")
                .unwrap()
                .parameter("year"),
            Some("2026")
        );
    }

    #[test]
    fn literal_route_precedes_parameter_route() {
        let graph = RouteGraphBuilder::new()
            .route(route("user", RouteMethod::get(), "/users/:id"))
            .route(route("new-user", RouteMethod::get(), "/users/new"))
            .seal()
            .unwrap();
        assert_eq!(
            graph
                .resolve(&RouteMethod::get(), "/users/new")
                .unwrap()
                .route_id(),
            "new-user"
        );
    }

    #[test]
    fn rejects_parameter_shape_collision() {
        let error = RouteGraphBuilder::new()
            .route(route("by-id", RouteMethod::get(), "/users/:id"))
            .route(route("by-name", RouteMethod::get(), "/users/:name"))
            .seal()
            .unwrap_err();
        assert!(matches!(error, RouteError::RouteCollision { .. }));
        assert_eq!(error.code(), "PLG-RTE-010");
    }

    #[test]
    fn rejects_portable_case_and_unicode_route_aliases() {
        for (first, second) in [
            ("/Guide", "/guide"),
            ("/caf\u{e9}", "/cafe\u{301}"),
            ("/Stra\u{df}e", "/STRASSE"),
        ] {
            let result = RouteGraphBuilder::new()
                .route(route("first", RouteMethod::get(), first))
                .route(route("second", RouteMethod::get(), second))
                .seal();
            assert!(
                matches!(result, Err(RouteError::RouteCollision { .. })),
                "portable alias was admitted: {first:?} versus {second:?}"
            );
        }
    }

    #[test]
    fn normalizes_admitted_unicode_path_and_rejects_portability_hazards() {
        let graph = RouteGraphBuilder::new()
            .route(route("cafe", RouteMethod::get(), "/caf\u{e9}/:item"))
            .seal()
            .unwrap();
        let matched = graph
            .resolve(&RouteMethod::get(), "/cafe\u{301}/cre\u{300}me")
            .unwrap();
        assert_eq!(matched.parameter("item"), Some("cr\u{e8}me"));

        for pattern in ["/CON", "/aux.txt", "/bad:name", "/trail."] {
            assert!(
                RoutePattern::parse(pattern).is_err(),
                "accepted {pattern:?}"
            );
        }
        for path in ["/item/%0a", "/item/\n"] {
            assert!(
                graph.resolve(&RouteMethod::get(), path).is_err(),
                "accepted {path:?}"
            );
        }
    }

    #[test]
    fn rejects_optional_expansion_collision() {
        let error = RouteGraphBuilder::new()
            .route(route("archive", RouteMethod::get(), "/archive"))
            .route(route("year", RouteMethod::get(), "/archive/:year?"))
            .seal()
            .unwrap_err();
        assert!(matches!(error, RouteError::RouteCollision { .. }));
    }

    #[test]
    fn same_shape_is_valid_for_distinct_methods() {
        let graph = RouteGraphBuilder::new()
            .route(route("read", RouteMethod::get(), "/items/:id"))
            .route(route("update", RouteMethod::post(), "/items/:id"))
            .seal()
            .unwrap();
        assert_eq!(graph.routes().len(), 2);
    }

    #[test]
    fn reports_allowed_methods_in_stable_order() {
        let graph = RouteGraphBuilder::new()
            .route(route("post", RouteMethod::post(), "/items"))
            .route(route("get", RouteMethod::get(), "/items"))
            .seal()
            .unwrap();
        let error = graph.resolve(&RouteMethod::delete(), "/items").unwrap_err();
        assert_eq!(
            error,
            ResolveError::MethodNotAllowed {
                allowed: vec![RouteMethod::get(), RouteMethod::post()]
            }
        );
    }

    #[test]
    fn distinguishes_invalid_target_from_not_found() {
        let graph = RouteGraphBuilder::new()
            .route(route("home", RouteMethod::get(), "/"))
            .seal()
            .unwrap();
        assert_eq!(
            graph.resolve(&RouteMethod::get(), "/missing").unwrap_err(),
            ResolveError::NotFound
        );
        for path in ["", "missing", "/a/", "/a//b", "/a%2fb", "/a?x=1", "/a/../b"] {
            assert_eq!(
                graph.resolve(&RouteMethod::get(), path).unwrap_err(),
                ResolveError::InvalidPath
            );
        }
    }

    #[test]
    fn duplicate_route_ids_fail_even_across_methods() {
        let error = RouteGraphBuilder::new()
            .route(route("item", RouteMethod::get(), "/items/:id"))
            .route(route("item", RouteMethod::post(), "/items/:id"))
            .seal()
            .unwrap_err();
        assert_eq!(error, RouteError::DuplicateRouteId("item".to_owned()));
    }

    #[test]
    fn digest_is_independent_of_registration_order() {
        let first = RouteGraphBuilder::new()
            .route(route("home", RouteMethod::get(), "/"))
            .route(route("item", RouteMethod::get(), "/items/:id"))
            .seal()
            .unwrap();
        let second = RouteGraphBuilder::new()
            .route(route("item", RouteMethod::get(), "/items/:id"))
            .route(route("home", RouteMethod::get(), "/"))
            .seal()
            .unwrap();
        assert_eq!(first.digest(), second.digest());
        assert_eq!(first.digest().len(), 64);
    }

    #[test]
    fn middleware_and_error_boundaries_are_ordered_and_digest_bound() {
        let guarded = route("account", RouteMethod::get(), "/account")
            .middleware("request-id")
            .unwrap()
            .middleware("authorization")
            .unwrap()
            .error_boundary("account-error")
            .unwrap();
        let graph = RouteGraphBuilder::new()
            .error_boundary("root-error")
            .unwrap()
            .route(guarded)
            .seal()
            .unwrap();
        let matched = graph.resolve(&RouteMethod::get(), "/account").unwrap();
        assert_eq!(
            matched.middleware_ids(),
            &["request-id".to_owned(), "authorization".to_owned()]
        );
        assert_eq!(matched.error_boundary_ids(), &["account-error".to_owned()]);
        assert_eq!(graph.error_boundary_ids(), &["root-error".to_owned()]);

        let changed = RouteGraphBuilder::new()
            .error_boundary("root-error")
            .unwrap()
            .route(
                route("account", RouteMethod::get(), "/account")
                    .middleware("authorization")
                    .unwrap()
                    .middleware("request-id")
                    .unwrap()
                    .error_boundary("account-error")
                    .unwrap(),
            )
            .seal()
            .unwrap();
        assert_ne!(graph.digest(), changed.digest());
    }

    #[test]
    fn behavior_ids_and_duplicates_fail_before_graph_sealing() {
        assert!(matches!(
            route("home", RouteMethod::get(), "/").middleware("Bad ID"),
            Err(RouteError::InvalidMiddlewareId(_))
        ));
        assert!(matches!(
            route("home", RouteMethod::get(), "/")
                .middleware("security")
                .unwrap()
                .middleware("security"),
            Err(RouteError::DuplicateMiddleware(_))
        ));
        assert!(matches!(
            RouteGraphBuilder::new()
                .error_boundary("root-error")
                .unwrap()
                .error_boundary("root-error"),
            Err(RouteError::DuplicateErrorBoundary(_))
        ));
    }

    #[test]
    fn route_limit_fails_before_graph_creation() {
        let mut builder = RouteGraphBuilder::with_max_routes(1).unwrap();
        builder.push(route("home", RouteMethod::get(), "/"));
        builder.push(route("about", RouteMethod::get(), "/about"));
        assert!(matches!(
            builder.seal(),
            Err(RouteError::TooManyRoutes {
                actual: 2,
                maximum: 1
            })
        ));
    }

    #[test]
    fn route_match_serializes_without_internal_matcher_state() {
        let graph = RouteGraphBuilder::new()
            .route(route("item", RouteMethod::get(), "/items/:id"))
            .seal()
            .unwrap();
        let matched = graph.resolve(&RouteMethod::get(), "/items/42").unwrap();
        let json = serde_json::to_value(matched).unwrap();
        assert_eq!(json["route_id"], "item");
        assert_eq!(json["parameters"]["id"], "42");
        assert_eq!(json["middleware"], serde_json::json!([]));
        assert_eq!(json["error_boundaries"], serde_json::json!([]));
    }
}
