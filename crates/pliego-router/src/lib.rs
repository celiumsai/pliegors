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
pub const MAX_ROUTE_LOADERS: usize = 32;
pub const MAX_ROUTE_ACTIONS: usize = 16;
pub const MAX_ROUTE_RESOURCES: usize = 32;
pub const MAX_RESOURCE_CAPABILITIES: usize = 32;
pub const MAX_SCOPE_DEPTH: usize = 16;
pub const DEFAULT_MAX_SCOPES: usize = 1_024;
pub const DEFAULT_MAX_ROUTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiddlewareCapability {
    RewritePath,
    Redirect,
    Reject,
    ReadBody,
    MutateResponseHeaders,
}

impl MiddlewareCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RewritePath => "rewrite-path",
            Self::Redirect => "redirect",
            Self::Reject => "reject",
            Self::ReadBody => "read-body",
            Self::MutateResponseHeaders => "mutate-response-headers",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MiddlewareCapabilities(BTreeSet<MiddlewareCapability>);

impl MiddlewareCapabilities {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn allowing(mut self, capability: MiddlewareCapability) -> Self {
        self.0.insert(capability);
        self
    }

    pub fn allows(&self, capability: MiddlewareCapability) -> bool {
        self.0.contains(&capability)
    }

    pub fn iter(&self) -> impl Iterator<Item = MiddlewareCapability> + '_ {
        self.0.iter().copied()
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteScopeKind {
    Group,
    Layout,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteResourceSpec {
    id: String,
    capabilities: BTreeSet<String>,
}

impl RouteResourceSpec {
    pub fn new(id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_data_id(&id).map_err(|_| RouteError::InvalidResourceId(id.clone()))?;
        Ok(Self {
            id,
            capabilities: BTreeSet::new(),
        })
    }

    pub fn requiring(mut self, capability: impl Into<String>) -> Result<Self, RouteError> {
        let capability = capability.into();
        validate_data_id(&capability)
            .map_err(|_| RouteError::InvalidResourceCapability(capability.clone()))?;
        if self.capabilities.len() >= MAX_RESOURCE_CAPABILITIES {
            return Err(RouteError::TooManyResourceCapabilities(
                MAX_RESOURCE_CAPABILITIES,
            ));
        }
        self.capabilities.insert(capability);
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn capabilities(&self) -> &BTreeSet<String> {
        &self.capabilities
    }
}

impl RouteScopeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::Layout => "layout",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteScopeSpec {
    id: String,
    kind: RouteScopeKind,
    parent: Option<String>,
    middleware: Vec<String>,
    error_boundaries: Vec<String>,
    loaders: Vec<String>,
    resources: Vec<RouteResourceSpec>,
}

impl RouteScopeSpec {
    pub fn new(id: impl Into<String>, kind: RouteScopeKind) -> Result<Self, RouteError> {
        let id = id.into();
        validate_route_id(&id).map_err(|_| RouteError::InvalidScopeId(id.clone()))?;
        Ok(Self {
            id,
            kind,
            parent: None,
            middleware: Vec::new(),
            error_boundaries: Vec::new(),
            loaders: Vec::new(),
            resources: Vec::new(),
        })
    }

    pub fn parent(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_route_id(&id).map_err(|_| RouteError::InvalidScopeId(id.clone()))?;
        self.parent = Some(id);
        Ok(self)
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

    pub fn loader(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_data_id(&id).map_err(|_| RouteError::InvalidLoaderId(id.clone()))?;
        if self.loaders.len() >= MAX_ROUTE_LOADERS {
            return Err(RouteError::TooManyLoaders(MAX_ROUTE_LOADERS));
        }
        if self.loaders.contains(&id) {
            return Err(RouteError::DuplicateLoader(id));
        }
        self.loaders.push(id);
        Ok(self)
    }

    pub fn resource(mut self, resource: RouteResourceSpec) -> Result<Self, RouteError> {
        if self.resources.len() >= MAX_ROUTE_RESOURCES {
            return Err(RouteError::TooManyResources(MAX_ROUTE_RESOURCES));
        }
        if self
            .resources
            .iter()
            .any(|current| current.id == resource.id)
        {
            return Err(RouteError::DuplicateResource(resource.id));
        }
        self.resources.push(resource);
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> RouteScopeKind {
        self.kind
    }

    pub fn parent_id(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    pub fn middleware_ids(&self) -> &[String] {
        &self.middleware
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }

    pub fn loader_ids(&self) -> &[String] {
        &self.loaders
    }

    pub fn resources(&self) -> &[RouteResourceSpec] {
        &self.resources
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RouteSpec {
    id: String,
    method: RouteMethod,
    pattern: RoutePattern,
    scope: Option<String>,
    middleware: Vec<String>,
    error_boundaries: Vec<String>,
    loaders: Vec<String>,
    actions: Vec<String>,
    cache_policy: Option<String>,
    resources: Vec<RouteResourceSpec>,
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
            scope: None,
            middleware: Vec::new(),
            error_boundaries: Vec::new(),
            loaders: Vec::new(),
            actions: Vec::new(),
            cache_policy: None,
            resources: Vec::new(),
        })
    }

    pub fn scope(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_route_id(&id).map_err(|_| RouteError::InvalidScopeId(id.clone()))?;
        self.scope = Some(id);
        Ok(self)
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

    pub fn loader(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_data_id(&id).map_err(|_| RouteError::InvalidLoaderId(id.clone()))?;
        if self.loaders.len() >= MAX_ROUTE_LOADERS {
            return Err(RouteError::TooManyLoaders(MAX_ROUTE_LOADERS));
        }
        if self.loaders.contains(&id) {
            return Err(RouteError::DuplicateLoader(id));
        }
        self.loaders.push(id);
        Ok(self)
    }

    pub fn action(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_data_id(&id).map_err(|_| RouteError::InvalidActionId(id.clone()))?;
        if self.actions.len() >= MAX_ROUTE_ACTIONS {
            return Err(RouteError::TooManyActions(MAX_ROUTE_ACTIONS));
        }
        if self.actions.contains(&id) {
            return Err(RouteError::DuplicateAction(id));
        }
        self.actions.push(id);
        Ok(self)
    }

    pub fn cache_policy(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_data_id(&id).map_err(|_| RouteError::InvalidCachePolicyId(id.clone()))?;
        self.cache_policy = Some(id);
        Ok(self)
    }

    pub fn resource(mut self, resource: RouteResourceSpec) -> Result<Self, RouteError> {
        if self.resources.len() >= MAX_ROUTE_RESOURCES {
            return Err(RouteError::TooManyResources(MAX_ROUTE_RESOURCES));
        }
        if self
            .resources
            .iter()
            .any(|current| current.id == resource.id)
        {
            return Err(RouteError::DuplicateResource(resource.id));
        }
        self.resources.push(resource);
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

    pub fn scope_id(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    pub fn middleware_ids(&self) -> &[String] {
        &self.middleware
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }

    pub fn loader_ids(&self) -> &[String] {
        &self.loaders
    }

    pub fn action_ids(&self) -> &[String] {
        &self.actions
    }

    pub fn cache_policy_id(&self) -> Option<&str> {
        self.cache_policy.as_deref()
    }

    pub fn resources(&self) -> &[RouteResourceSpec] {
        &self.resources
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

fn validate_data_id(id: &str) -> Result<(), ()> {
    if id.is_empty() || id.len() > 96 {
        return Err(());
    }
    let mut bytes = id.bytes();
    let Some(first) = bytes.next() else {
        return Err(());
    };
    if !first.is_ascii_lowercase()
        || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || id.ends_with('-')
        || id.contains("--")
    {
        return Err(());
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct RouteGraphBuilder {
    max_routes: usize,
    routes: Vec<RouteSpec>,
    scopes: Vec<RouteScopeSpec>,
    pre_route_middleware: Vec<String>,
    middleware_capabilities: BTreeMap<String, MiddlewareCapabilities>,
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
            scopes: Vec::new(),
            pre_route_middleware: Vec::new(),
            middleware_capabilities: BTreeMap::new(),
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
            scopes: Vec::new(),
            pre_route_middleware: Vec::new(),
            middleware_capabilities: BTreeMap::new(),
            error_boundaries: Vec::new(),
        })
    }

    pub fn declare_middleware(
        mut self,
        id: impl Into<String>,
        capabilities: MiddlewareCapabilities,
    ) -> Result<Self, RouteError> {
        let id = id.into();
        validate_behavior_id(&id, BehaviorKind::Middleware)?;
        if self
            .middleware_capabilities
            .insert(id.clone(), capabilities)
            .is_some()
        {
            return Err(RouteError::DuplicateMiddlewareDeclaration(id));
        }
        Ok(self)
    }

    pub fn pre_route_middleware(mut self, id: impl Into<String>) -> Result<Self, RouteError> {
        let id = id.into();
        validate_behavior_id(&id, BehaviorKind::Middleware)?;
        if self.pre_route_middleware.len() >= MAX_ROUTE_MIDDLEWARE {
            return Err(RouteError::TooManyMiddleware(MAX_ROUTE_MIDDLEWARE));
        }
        if self.pre_route_middleware.contains(&id) {
            return Err(RouteError::DuplicateMiddleware(id));
        }
        self.pre_route_middleware.push(id);
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

    pub fn scope(mut self, scope: RouteScopeSpec) -> Self {
        self.scopes.push(scope);
        self
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
        self.scopes.sort_by(|left, right| left.id.cmp(&right.id));
        if self.scopes.len() > DEFAULT_MAX_SCOPES {
            return Err(RouteError::TooManyScopes {
                actual: self.scopes.len(),
                maximum: DEFAULT_MAX_SCOPES,
            });
        }
        let mut scope_map = BTreeMap::new();
        for scope in &self.scopes {
            if scope_map.insert(scope.id.clone(), scope).is_some() {
                return Err(RouteError::DuplicateScopeId(scope.id.clone()));
            }
        }
        for scope in &self.scopes {
            resolve_scope_chain(Some(&scope.id), &scope_map)?;
        }
        let mut resolved_scopes = BTreeMap::new();
        let mut reached_scopes = BTreeSet::new();
        for route in &self.routes {
            let resolved = resolve_scope_chain(route.scope.as_deref(), &scope_map)?;
            reached_scopes.extend(resolved.ids.iter().cloned());
            if resolved.middleware.len() + route.middleware.len() > MAX_ROUTE_MIDDLEWARE {
                return Err(RouteError::TooManyMiddleware(MAX_ROUTE_MIDDLEWARE));
            }
            if let Some(id) = route
                .middleware
                .iter()
                .find(|id| resolved.middleware.contains(*id))
            {
                return Err(RouteError::DuplicateInheritedMiddleware(id.clone()));
            }
            if resolved.loaders.len() + route.loaders.len() > MAX_ROUTE_LOADERS {
                return Err(RouteError::TooManyLoaders(MAX_ROUTE_LOADERS));
            }
            if let Some(id) = route
                .loaders
                .iter()
                .find(|id| resolved.loaders.contains(*id))
            {
                return Err(RouteError::DuplicateInheritedLoader(id.clone()));
            }
            if self.error_boundaries.len()
                + resolved.error_boundaries.len()
                + route.error_boundaries.len()
                > MAX_ERROR_BOUNDARIES
            {
                return Err(RouteError::TooManyErrorBoundaries(MAX_ERROR_BOUNDARIES));
            }
            let mut error_boundaries = BTreeSet::new();
            for id in self
                .error_boundaries
                .iter()
                .chain(resolved.error_boundaries.iter())
                .chain(route.error_boundaries.iter())
            {
                if !error_boundaries.insert(id.clone()) {
                    return Err(RouteError::DuplicateInheritedErrorBoundary(id.clone()));
                }
            }
            resolved_scopes.insert(route.id.clone(), resolved);
        }
        if let Some(id) = self
            .scopes
            .iter()
            .map(|scope| &scope.id)
            .find(|id| !reached_scopes.contains(*id))
        {
            return Err(RouteError::UnreferencedScope(id.clone()));
        }
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

        let route_middleware: BTreeSet<_> = self
            .routes
            .iter()
            .flat_map(|route| route.middleware.iter().cloned())
            .chain(
                self.scopes
                    .iter()
                    .flat_map(|scope| scope.middleware.iter().cloned()),
            )
            .collect();
        if let Some(id) = self
            .pre_route_middleware
            .iter()
            .find(|id| route_middleware.contains(*id))
        {
            return Err(RouteError::MiddlewarePhaseConflict(id.clone()));
        }
        let referenced_middleware: BTreeSet<_> = route_middleware
            .iter()
            .cloned()
            .chain(self.pre_route_middleware.iter().cloned())
            .collect();
        if let Some(id) = referenced_middleware
            .iter()
            .find(|id| !self.middleware_capabilities.contains_key(*id))
        {
            return Err(RouteError::MissingMiddlewareDeclaration(id.clone()));
        }
        if let Some(id) = self
            .middleware_capabilities
            .keys()
            .find(|id| !referenced_middleware.contains(*id))
        {
            return Err(RouteError::UnreferencedMiddlewareDeclaration(id.clone()));
        }

        let all_error_boundaries: BTreeSet<_> = self
            .error_boundaries
            .iter()
            .chain(
                self.scopes
                    .iter()
                    .flat_map(|scope| scope.error_boundaries.iter()),
            )
            .chain(
                self.routes
                    .iter()
                    .flat_map(|route| route.error_boundaries.iter()),
            )
            .cloned()
            .collect();

        let digest = graph_digest(
            &self.routes,
            &self.scopes,
            &self.pre_route_middleware,
            &self.middleware_capabilities,
            &self.error_boundaries,
        );
        self.routes.sort_by(match_order);
        Ok(RouteGraph {
            routes: self.routes,
            scopes: self.scopes,
            resolved_scopes,
            route_middleware,
            all_error_boundaries,
            pre_route_middleware: self.pre_route_middleware,
            middleware_capabilities: self.middleware_capabilities,
            error_boundaries: self.error_boundaries,
            digest,
        })
    }
}

#[derive(Clone, Debug, Default)]
struct ResolvedScopeChain {
    ids: Vec<String>,
    layouts: Vec<String>,
    middleware: Vec<String>,
    error_boundaries: Vec<String>,
    loaders: Vec<String>,
    resources: BTreeMap<String, BTreeSet<String>>,
}

fn resolve_scope_chain(
    leaf: Option<&str>,
    scopes: &BTreeMap<String, &RouteScopeSpec>,
) -> Result<ResolvedScopeChain, RouteError> {
    let Some(mut current) = leaf else {
        return Ok(ResolvedScopeChain::default());
    };
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    loop {
        if !seen.insert(current.to_owned()) {
            return Err(RouteError::ScopeCycle(current.to_owned()));
        }
        let scope = scopes
            .get(current)
            .copied()
            .ok_or_else(|| RouteError::UnknownScope(current.to_owned()))?;
        chain.push(scope);
        match scope.parent.as_deref() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    chain.reverse();
    if chain.len() > MAX_SCOPE_DEPTH {
        return Err(RouteError::TooManyScopes {
            actual: chain.len(),
            maximum: MAX_SCOPE_DEPTH,
        });
    }
    let mut resolved = ResolvedScopeChain::default();
    let mut middleware = BTreeSet::new();
    let mut boundaries = BTreeSet::new();
    let mut loaders = BTreeSet::new();
    for scope in chain {
        resolved.ids.push(scope.id.clone());
        if scope.kind == RouteScopeKind::Layout {
            resolved.layouts.push(scope.id.clone());
        }
        for id in &scope.middleware {
            if !middleware.insert(id.clone()) {
                return Err(RouteError::DuplicateInheritedMiddleware(id.clone()));
            }
            resolved.middleware.push(id.clone());
        }
        for id in &scope.error_boundaries {
            if !boundaries.insert(id.clone()) {
                return Err(RouteError::DuplicateInheritedErrorBoundary(id.clone()));
            }
            resolved.error_boundaries.push(id.clone());
        }
        for id in &scope.loaders {
            if !loaders.insert(id.clone()) {
                return Err(RouteError::DuplicateInheritedLoader(id.clone()));
            }
            resolved.loaders.push(id.clone());
        }
        for resource in &scope.resources {
            resolved
                .resources
                .entry(resource.id.clone())
                .or_default()
                .extend(resource.capabilities.iter().cloned());
        }
    }
    Ok(resolved)
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

fn graph_digest(
    routes: &[RouteSpec],
    scopes: &[RouteScopeSpec],
    pre_route_middleware: &[String],
    middleware_capabilities: &BTreeMap<String, MiddlewareCapabilities>,
    error_boundaries: &[String],
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-route-graph-v6\0");
    digest_sequence(&mut digest, b"pre-route-middleware", pre_route_middleware);
    digest_sequence(&mut digest, b"root-error-boundaries", error_boundaries);
    for (id, capabilities) in middleware_capabilities {
        digest.update((id.len() as u64).to_be_bytes());
        digest.update(id.as_bytes());
        let values: Vec<_> = capabilities
            .iter()
            .map(|capability| capability.as_str().to_owned())
            .collect();
        digest_sequence(&mut digest, b"middleware-capabilities", &values);
    }
    for scope in scopes {
        for value in [
            scope.id.as_str(),
            scope.kind.as_str(),
            scope.parent.as_deref().unwrap_or(""),
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        digest_sequence(&mut digest, b"scope-middleware", &scope.middleware);
        digest_sequence(
            &mut digest,
            b"scope-error-boundaries",
            &scope.error_boundaries,
        );
        digest_sequence(&mut digest, b"scope-loaders", &scope.loaders);
        digest_resources(&mut digest, b"scope-resources", &scope.resources);
    }
    for route in routes {
        for value in [
            route.method.as_str(),
            route.id.as_str(),
            route.pattern.authored(),
            route.pattern.canonical(),
            route.scope.as_deref().unwrap_or(""),
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
        digest_sequence(&mut digest, b"route-loaders", &route.loaders);
        digest_sequence(&mut digest, b"route-actions", &route.actions);
        let cache_policy = route.cache_policy.iter().cloned().collect::<Vec<_>>();
        digest_sequence(&mut digest, b"route-cache-policy", &cache_policy);
        digest_resources(&mut digest, b"route-resources", &route.resources);
    }
    encode_hex(&digest.finalize())
}

fn digest_resources(digest: &mut Sha256, label: &[u8], resources: &[RouteResourceSpec]) {
    digest.update((label.len() as u64).to_be_bytes());
    digest.update(label);
    digest.update((resources.len() as u64).to_be_bytes());
    let mut resources = resources.iter().collect::<Vec<_>>();
    resources.sort_by(|left, right| left.id.cmp(&right.id));
    for resource in resources {
        digest.update((resource.id.len() as u64).to_be_bytes());
        digest.update(resource.id.as_bytes());
        let capabilities = resource.capabilities.iter().cloned().collect::<Vec<_>>();
        digest_sequence(digest, b"resource-capabilities", &capabilities);
    }
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
    scopes: Vec<RouteScopeSpec>,
    resolved_scopes: BTreeMap<String, ResolvedScopeChain>,
    route_middleware: BTreeSet<String>,
    all_error_boundaries: BTreeSet<String>,
    pre_route_middleware: Vec<String>,
    middleware_capabilities: BTreeMap<String, MiddlewareCapabilities>,
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

    pub fn scopes(&self) -> &[RouteScopeSpec] {
        &self.scopes
    }

    pub fn route_middleware_ids(&self) -> &BTreeSet<String> {
        &self.route_middleware
    }

    pub fn all_error_boundary_ids(&self) -> &BTreeSet<String> {
        &self.all_error_boundaries
    }

    pub fn pre_route_middleware_ids(&self) -> &[String] {
        &self.pre_route_middleware
    }

    pub fn middleware_capabilities(&self, id: &str) -> Option<&MiddlewareCapabilities> {
        self.middleware_capabilities.get(id)
    }

    pub fn middleware_declarations(&self) -> &BTreeMap<String, MiddlewareCapabilities> {
        &self.middleware_capabilities
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }

    pub fn route_resource_requirements(
        &self,
        route_id: &str,
    ) -> Option<BTreeMap<String, BTreeSet<String>>> {
        let route = self.routes.iter().find(|route| route.id == route_id)?;
        let mut resources = self.resolved_scopes.get(route_id)?.resources.clone();
        for resource in &route.resources {
            resources
                .entry(resource.id.clone())
                .or_default()
                .extend(resource.capabilities.iter().cloned());
        }
        Some(resources)
    }

    pub fn route_loader_ids(&self, route_id: &str) -> Option<Vec<String>> {
        let route = self.routes.iter().find(|route| route.id == route_id)?;
        let inherited = self.resolved_scopes.get(route_id)?;
        Some(
            inherited
                .loaders
                .iter()
                .chain(route.loaders.iter())
                .cloned()
                .collect(),
        )
    }

    pub fn resolve(&self, method: &RouteMethod, path: &str) -> Result<RouteMatch, ResolveError> {
        validate_request_path(path)?;
        let normalized: String = path.nfc().collect();
        validate_request_path(&normalized)?;
        for route in self.routes.iter().filter(|route| &route.method == method) {
            if let Some(parameters) = route.pattern.match_admitted(&normalized) {
                let inherited = self
                    .resolved_scopes
                    .get(&route.id)
                    .expect("sealed routes have resolved scope chains");
                let middleware = inherited
                    .middleware
                    .iter()
                    .chain(route.middleware.iter())
                    .cloned()
                    .collect();
                let error_boundaries = inherited
                    .error_boundaries
                    .iter()
                    .chain(route.error_boundaries.iter())
                    .cloned()
                    .collect();
                let loaders = inherited
                    .loaders
                    .iter()
                    .chain(route.loaders.iter())
                    .cloned()
                    .collect();
                let mut resources = inherited.resources.clone();
                for resource in &route.resources {
                    resources
                        .entry(resource.id.clone())
                        .or_default()
                        .extend(resource.capabilities.iter().cloned());
                }
                return Ok(RouteMatch {
                    route_id: route.id.clone(),
                    method: route.method.clone(),
                    pattern: route.pattern.canonical.clone(),
                    parameters,
                    scopes: inherited.ids.clone(),
                    layouts: inherited.layouts.clone(),
                    middleware,
                    error_boundaries,
                    loaders,
                    actions: route.actions.clone(),
                    cache_policy: route.cache_policy.clone(),
                    resources,
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
    scopes: Vec<String>,
    layouts: Vec<String>,
    middleware: Vec<String>,
    error_boundaries: Vec<String>,
    loaders: Vec<String>,
    actions: Vec<String>,
    cache_policy: Option<String>,
    resources: BTreeMap<String, BTreeSet<String>>,
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

    pub fn scope_ids(&self) -> &[String] {
        &self.scopes
    }

    pub fn layout_ids(&self) -> &[String] {
        &self.layouts
    }

    pub fn middleware_ids(&self) -> &[String] {
        &self.middleware
    }

    pub fn error_boundary_ids(&self) -> &[String] {
        &self.error_boundaries
    }

    pub fn loader_ids(&self) -> &[String] {
        &self.loaders
    }

    pub fn action_ids(&self) -> &[String] {
        &self.actions
    }

    pub fn cache_policy_id(&self) -> Option<&str> {
        self.cache_policy.as_deref()
    }

    pub fn resource_requirements(&self) -> &BTreeMap<String, BTreeSet<String>> {
        &self.resources
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
    DuplicateMiddlewareDeclaration(String),
    MissingMiddlewareDeclaration(String),
    UnreferencedMiddlewareDeclaration(String),
    MiddlewarePhaseConflict(String),
    InvalidScopeId(String),
    DuplicateScopeId(String),
    UnknownScope(String),
    UnreferencedScope(String),
    ScopeCycle(String),
    TooManyScopes {
        actual: usize,
        maximum: usize,
    },
    DuplicateInheritedMiddleware(String),
    DuplicateInheritedErrorBoundary(String),
    InvalidLoaderId(String),
    DuplicateLoader(String),
    DuplicateInheritedLoader(String),
    TooManyLoaders(usize),
    InvalidActionId(String),
    DuplicateAction(String),
    TooManyActions(usize),
    InvalidCachePolicyId(String),
    InvalidResourceId(String),
    DuplicateResource(String),
    TooManyResources(usize),
    InvalidResourceCapability(String),
    TooManyResourceCapabilities(usize),
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
            Self::DuplicateMiddlewareDeclaration(_)
            | Self::MissingMiddlewareDeclaration(_)
            | Self::UnreferencedMiddlewareDeclaration(_)
            | Self::MiddlewarePhaseConflict(_) => "PLG-RTE-013",
            Self::InvalidScopeId(_)
            | Self::DuplicateScopeId(_)
            | Self::UnknownScope(_)
            | Self::UnreferencedScope(_)
            | Self::ScopeCycle(_)
            | Self::TooManyScopes { .. }
            | Self::DuplicateInheritedMiddleware(_)
            | Self::DuplicateInheritedErrorBoundary(_) => "PLG-RTE-014",
            Self::InvalidErrorBoundaryId(_)
            | Self::DuplicateErrorBoundary(_)
            | Self::TooManyErrorBoundaries(_) => "PLG-RTE-012",
            Self::InvalidLoaderId(_)
            | Self::DuplicateLoader(_)
            | Self::DuplicateInheritedLoader(_)
            | Self::TooManyLoaders(_) => "PLG-RTE-015",
            Self::InvalidActionId(_) | Self::DuplicateAction(_) | Self::TooManyActions(_) => {
                "PLG-RTE-016"
            }
            Self::InvalidCachePolicyId(_) => "PLG-RTE-017",
            Self::InvalidResourceId(_)
            | Self::DuplicateResource(_)
            | Self::TooManyResources(_)
            | Self::InvalidResourceCapability(_)
            | Self::TooManyResourceCapabilities(_) => "PLG-RTE-018",
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
            Self::DuplicateMiddlewareDeclaration(value) => {
                write!(formatter, "duplicate middleware declaration: {value}")
            }
            Self::MissingMiddlewareDeclaration(value) => {
                write!(
                    formatter,
                    "middleware {value} has no capability declaration"
                )
            }
            Self::UnreferencedMiddlewareDeclaration(value) => {
                write!(
                    formatter,
                    "middleware declaration {value} is not referenced"
                )
            }
            Self::MiddlewarePhaseConflict(value) => write!(
                formatter,
                "middleware {value} cannot use both pre-route and route phases"
            ),
            Self::InvalidScopeId(value) => write!(formatter, "invalid route scope ID {value:?}"),
            Self::DuplicateScopeId(value) => write!(formatter, "duplicate route scope ID: {value}"),
            Self::UnknownScope(value) => write!(formatter, "unknown route scope: {value}"),
            Self::UnreferencedScope(value) => {
                write!(formatter, "unreferenced route scope: {value}")
            }
            Self::ScopeCycle(value) => write!(formatter, "route scope cycle reaches {value}"),
            Self::TooManyScopes { actual, maximum } => write!(
                formatter,
                "route graph or chain has {actual} scopes; maximum is {maximum}"
            ),
            Self::DuplicateInheritedMiddleware(value) => write!(
                formatter,
                "middleware {value} appears more than once in one inherited route chain"
            ),
            Self::DuplicateInheritedErrorBoundary(value) => write!(
                formatter,
                "error boundary {value} appears more than once in one inherited route chain"
            ),
            Self::InvalidLoaderId(value) => write!(formatter, "invalid loader ID {value:?}"),
            Self::DuplicateLoader(value) => write!(formatter, "duplicate loader ID: {value}"),
            Self::DuplicateInheritedLoader(value) => write!(
                formatter,
                "loader {value} appears more than once in one inherited route chain"
            ),
            Self::TooManyLoaders(maximum) => {
                write!(formatter, "route exceeds {maximum} loader entries")
            }
            Self::InvalidActionId(value) => write!(formatter, "invalid action ID {value:?}"),
            Self::DuplicateAction(value) => write!(formatter, "duplicate action ID: {value}"),
            Self::TooManyActions(maximum) => {
                write!(formatter, "route exceeds {maximum} action entries")
            }
            Self::InvalidCachePolicyId(value) => {
                write!(formatter, "invalid cache policy ID {value:?}")
            }
            Self::InvalidResourceId(value) => {
                write!(formatter, "invalid resource ID {value:?}")
            }
            Self::DuplicateResource(value) => {
                write!(formatter, "duplicate resource ID: {value}")
            }
            Self::TooManyResources(maximum) => {
                write!(formatter, "route exceeds {maximum} resource entries")
            }
            Self::InvalidResourceCapability(value) => {
                write!(formatter, "invalid resource capability {value:?}")
            }
            Self::TooManyResourceCapabilities(maximum) => {
                write!(formatter, "resource exceeds {maximum} capability entries")
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
            .declare_middleware("request-id", MiddlewareCapabilities::none())
            .unwrap()
            .declare_middleware(
                "authorization",
                MiddlewareCapabilities::none().allowing(MiddlewareCapability::Reject),
            )
            .unwrap()
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
            .declare_middleware(
                "authorization",
                MiddlewareCapabilities::none().allowing(MiddlewareCapability::Reject),
            )
            .unwrap()
            .declare_middleware("request-id", MiddlewareCapabilities::none())
            .unwrap()
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
        assert!(
            graph
                .middleware_capabilities("authorization")
                .unwrap()
                .allows(MiddlewareCapability::Reject)
        );
    }

    #[test]
    fn data_requirements_are_inherited_merged_and_digest_bound() {
        let layout = RouteScopeSpec::new("account-layout", RouteScopeKind::Layout)
            .unwrap()
            .loader("identity-loader")
            .unwrap()
            .resource(
                RouteResourceSpec::new("accounts")
                    .unwrap()
                    .requiring("read")
                    .unwrap(),
            )
            .unwrap();
        let account = route("account", RouteMethod::get(), "/account")
            .scope("account-layout")
            .unwrap()
            .loader("account-loader")
            .unwrap()
            .action("rename-account")
            .unwrap()
            .cache_policy("account-private")
            .unwrap()
            .resource(
                RouteResourceSpec::new("accounts")
                    .unwrap()
                    .requiring("write")
                    .unwrap(),
            )
            .unwrap();
        let graph = RouteGraphBuilder::new()
            .scope(layout)
            .route(account)
            .seal()
            .unwrap();
        let matched = graph.resolve(&RouteMethod::get(), "/account").unwrap();
        assert_eq!(
            matched.loader_ids(),
            &["identity-loader".to_owned(), "account-loader".to_owned()]
        );
        assert_eq!(matched.action_ids(), &["rename-account".to_owned()]);
        assert_eq!(matched.cache_policy_id(), Some("account-private"));
        assert_eq!(
            matched.resource_requirements().get("accounts").unwrap(),
            &BTreeSet::from(["read".to_owned(), "write".to_owned()])
        );

        let changed = RouteGraphBuilder::new()
            .scope(
                RouteScopeSpec::new("account-layout", RouteScopeKind::Layout)
                    .unwrap()
                    .loader("identity-loader")
                    .unwrap()
                    .resource(
                        RouteResourceSpec::new("accounts")
                            .unwrap()
                            .requiring("read")
                            .unwrap(),
                    )
                    .unwrap(),
            )
            .route(
                route("account", RouteMethod::get(), "/account")
                    .scope("account-layout")
                    .unwrap()
                    .loader("account-loader-v2")
                    .unwrap()
                    .action("rename-account")
                    .unwrap()
                    .cache_policy("account-private")
                    .unwrap()
                    .resource(
                        RouteResourceSpec::new("accounts")
                            .unwrap()
                            .requiring("write")
                            .unwrap(),
                    )
                    .unwrap(),
            )
            .seal()
            .unwrap();
        assert_ne!(graph.digest(), changed.digest());
    }

    #[test]
    fn resource_authorship_order_does_not_change_the_graph_digest() {
        let alpha = RouteResourceSpec::new("alpha")
            .unwrap()
            .requiring("read")
            .unwrap();
        let beta = RouteResourceSpec::new("beta")
            .unwrap()
            .requiring("write")
            .unwrap();
        let first = RouteGraphBuilder::new()
            .route(
                route("home", RouteMethod::get(), "/")
                    .resource(alpha.clone())
                    .unwrap()
                    .resource(beta.clone())
                    .unwrap(),
            )
            .seal()
            .unwrap();
        let second = RouteGraphBuilder::new()
            .route(
                route("home", RouteMethod::get(), "/")
                    .resource(beta)
                    .unwrap()
                    .resource(alpha)
                    .unwrap(),
            )
            .seal()
            .unwrap();
        assert_eq!(first.digest(), second.digest());
    }

    #[test]
    fn middleware_capabilities_are_explicit_reachable_and_digest_bound() {
        let guarded = route("home", RouteMethod::get(), "/")
            .middleware("security")
            .unwrap();
        let missing = RouteGraphBuilder::new()
            .route(guarded.clone())
            .seal()
            .unwrap_err();
        assert_eq!(
            missing,
            RouteError::MissingMiddlewareDeclaration("security".to_owned())
        );

        let unreferenced = RouteGraphBuilder::new()
            .declare_middleware("security", MiddlewareCapabilities::none())
            .unwrap()
            .route(route("home", RouteMethod::get(), "/"))
            .seal()
            .unwrap_err();
        assert_eq!(
            unreferenced,
            RouteError::UnreferencedMiddlewareDeclaration("security".to_owned())
        );

        let baseline = RouteGraphBuilder::new()
            .declare_middleware("security", MiddlewareCapabilities::none())
            .unwrap()
            .route(guarded.clone())
            .seal()
            .unwrap();
        let elevated = RouteGraphBuilder::new()
            .declare_middleware(
                "security",
                MiddlewareCapabilities::none()
                    .allowing(MiddlewareCapability::Reject)
                    .allowing(MiddlewareCapability::MutateResponseHeaders),
            )
            .unwrap()
            .route(guarded)
            .seal()
            .unwrap();
        assert_ne!(baseline.digest(), elevated.digest());
    }

    #[test]
    fn pre_route_middleware_is_ordered_digest_bound_and_phase_exclusive() {
        let capabilities = MiddlewareCapabilities::none()
            .allowing(MiddlewareCapability::RewritePath)
            .allowing(MiddlewareCapability::MutateResponseHeaders);
        let graph = RouteGraphBuilder::new()
            .declare_middleware("canonicalize", capabilities.clone())
            .unwrap()
            .pre_route_middleware("canonicalize")
            .unwrap()
            .route(route("home", RouteMethod::get(), "/"))
            .seal()
            .unwrap();
        assert_eq!(
            graph.pre_route_middleware_ids(),
            &["canonicalize".to_owned()]
        );
        assert_eq!(
            graph.middleware_capabilities("canonicalize"),
            Some(&capabilities)
        );

        let without_pre_route = RouteGraphBuilder::new()
            .route(route("home", RouteMethod::get(), "/"))
            .seal()
            .unwrap();
        assert_ne!(graph.digest(), without_pre_route.digest());

        let conflict = RouteGraphBuilder::new()
            .declare_middleware("canonicalize", capabilities)
            .unwrap()
            .pre_route_middleware("canonicalize")
            .unwrap()
            .route(
                route("home", RouteMethod::get(), "/")
                    .middleware("canonicalize")
                    .unwrap(),
            )
            .seal()
            .unwrap_err();
        assert_eq!(
            conflict,
            RouteError::MiddlewarePhaseConflict("canonicalize".to_owned())
        );
    }

    #[test]
    fn group_and_layout_scopes_inherit_root_to_leaf_and_bind_the_digest() {
        let group = RouteScopeSpec::new("app-group", RouteScopeKind::Group)
            .unwrap()
            .middleware("group-policy")
            .unwrap();
        let layout = RouteScopeSpec::new("account-layout", RouteScopeKind::Layout)
            .unwrap()
            .parent("app-group")
            .unwrap()
            .middleware("layout-policy")
            .unwrap()
            .error_boundary("layout-error")
            .unwrap();
        let scoped_route = route("account", RouteMethod::get(), "/account")
            .scope("account-layout")
            .unwrap()
            .middleware("route-policy")
            .unwrap()
            .error_boundary("route-error")
            .unwrap();
        let graph = RouteGraphBuilder::new()
            .declare_middleware("group-policy", MiddlewareCapabilities::none())
            .unwrap()
            .declare_middleware("layout-policy", MiddlewareCapabilities::none())
            .unwrap()
            .declare_middleware("route-policy", MiddlewareCapabilities::none())
            .unwrap()
            .scope(group)
            .scope(layout)
            .route(scoped_route)
            .seal()
            .unwrap();
        let matched = graph.resolve(&RouteMethod::get(), "/account").unwrap();
        assert_eq!(
            matched.scope_ids(),
            &["app-group".to_owned(), "account-layout".to_owned()]
        );
        assert_eq!(matched.layout_ids(), &["account-layout".to_owned()]);
        assert_eq!(
            matched.middleware_ids(),
            &[
                "group-policy".to_owned(),
                "layout-policy".to_owned(),
                "route-policy".to_owned()
            ]
        );
        assert_eq!(
            matched.error_boundary_ids(),
            &["layout-error".to_owned(), "route-error".to_owned()]
        );
        assert_eq!(graph.scopes().len(), 2);

        let flat = RouteGraphBuilder::new()
            .declare_middleware("route-policy", MiddlewareCapabilities::none())
            .unwrap()
            .route(
                route("account", RouteMethod::get(), "/account")
                    .middleware("route-policy")
                    .unwrap()
                    .error_boundary("route-error")
                    .unwrap(),
            )
            .seal()
            .unwrap();
        assert_ne!(graph.digest(), flat.digest());
    }

    #[test]
    fn scope_graph_rejects_unknown_cycles_unreachable_and_duplicate_inheritance() {
        let unknown = RouteGraphBuilder::new()
            .route(
                route("home", RouteMethod::get(), "/")
                    .scope("missing")
                    .unwrap(),
            )
            .seal()
            .unwrap_err();
        assert_eq!(unknown, RouteError::UnknownScope("missing".to_owned()));

        let cycle = RouteGraphBuilder::new()
            .scope(
                RouteScopeSpec::new("first", RouteScopeKind::Group)
                    .unwrap()
                    .parent("second")
                    .unwrap(),
            )
            .scope(
                RouteScopeSpec::new("second", RouteScopeKind::Layout)
                    .unwrap()
                    .parent("first")
                    .unwrap(),
            )
            .route(
                route("home", RouteMethod::get(), "/")
                    .scope("second")
                    .unwrap(),
            )
            .seal()
            .unwrap_err();
        assert!(matches!(cycle, RouteError::ScopeCycle(_)));

        let unreachable = RouteGraphBuilder::new()
            .scope(RouteScopeSpec::new("unused", RouteScopeKind::Group).unwrap())
            .route(route("home", RouteMethod::get(), "/"))
            .seal()
            .unwrap_err();
        assert_eq!(
            unreachable,
            RouteError::UnreferencedScope("unused".to_owned())
        );

        let duplicate = RouteGraphBuilder::new()
            .declare_middleware("policy", MiddlewareCapabilities::none())
            .unwrap()
            .scope(
                RouteScopeSpec::new("group", RouteScopeKind::Group)
                    .unwrap()
                    .middleware("policy")
                    .unwrap(),
            )
            .route(
                route("home", RouteMethod::get(), "/")
                    .scope("group")
                    .unwrap()
                    .middleware("policy")
                    .unwrap(),
            )
            .seal()
            .unwrap_err();
        assert_eq!(
            duplicate,
            RouteError::DuplicateInheritedMiddleware("policy".to_owned())
        );

        let duplicate_boundary = RouteGraphBuilder::new()
            .error_boundary("shared-error")
            .unwrap()
            .scope(
                RouteScopeSpec::new("group", RouteScopeKind::Group)
                    .unwrap()
                    .error_boundary("shared-error")
                    .unwrap(),
            )
            .route(
                route("home", RouteMethod::get(), "/")
                    .scope("group")
                    .unwrap(),
            )
            .seal()
            .unwrap_err();
        assert_eq!(
            duplicate_boundary,
            RouteError::DuplicateInheritedErrorBoundary("shared-error".to_owned())
        );

        let mut too_many_boundaries = RouteGraphBuilder::new();
        for index in 0..MAX_ERROR_BOUNDARIES {
            too_many_boundaries = too_many_boundaries
                .error_boundary(format!("root-error-{index}"))
                .unwrap();
        }
        let too_many_boundaries = too_many_boundaries
            .scope(
                RouteScopeSpec::new("group", RouteScopeKind::Group)
                    .unwrap()
                    .error_boundary("scope-error")
                    .unwrap(),
            )
            .route(
                route("home", RouteMethod::get(), "/")
                    .scope("group")
                    .unwrap(),
            )
            .seal()
            .unwrap_err();
        assert_eq!(
            too_many_boundaries,
            RouteError::TooManyErrorBoundaries(MAX_ERROR_BOUNDARIES)
        );
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
        assert_eq!(json["scopes"], serde_json::json!([]));
        assert_eq!(json["layouts"], serde_json::json!([]));
        assert_eq!(json["middleware"], serde_json::json!([]));
        assert_eq!(json["error_boundaries"], serde_json::json!([]));
    }
}
