// SPDX-License-Identifier: Apache-2.0

use crate::{PbocError, PbocManifest, PbocRoute, validate_manifest};
use pliego_router::{ResolveError, RouteGraph, RouteGraphBuilder, RouteMethod, RouteSpec};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug)]
pub struct PbocRouter {
    graph: RouteGraph,
    routes: BTreeMap<String, PbocRoute>,
}

impl PbocRouter {
    pub fn new(manifest: &PbocManifest) -> Result<Self, PbocRouterError> {
        validate_manifest(manifest).map_err(PbocRouterError::Manifest)?;
        let mut builder = RouteGraphBuilder::with_max_routes(manifest.routes.len().max(1))
            .map_err(|error| PbocRouterError::Graph(error.to_string()))?;
        let mut routes = BTreeMap::new();
        for route in &manifest.routes {
            let method = RouteMethod::new(route.method.clone())
                .map_err(|error| PbocRouterError::Graph(error.to_string()))?;
            let mut spec = RouteSpec::new(route.id.clone(), method, route.pattern.clone())
                .map_err(|error| PbocRouterError::Graph(error.to_string()))?;
            if let Some(cache_policy) = &route.cache_policy_id {
                spec = spec
                    .cache_policy(cache_policy.clone())
                    .map_err(|error| PbocRouterError::Graph(error.to_string()))?;
            }
            builder.push(spec);
            routes.insert(route.id.clone(), route.clone());
        }
        let graph = builder
            .seal()
            .map_err(|error| PbocRouterError::Graph(error.to_string()))?;
        Ok(Self { graph, routes })
    }

    pub fn resolve(&self, method: &str, path: &str) -> Result<PbocRouteMatch, PbocRouterError> {
        let method =
            RouteMethod::new(method.to_owned()).map_err(|_| PbocRouterError::InvalidRequest)?;
        let matched = self
            .graph
            .resolve(&method, path)
            .map_err(PbocRouterError::Resolve)?;
        let route = self
            .routes
            .get(matched.route_id())
            .expect("PBOC router graph and route map are built together")
            .clone();
        Ok(PbocRouteMatch {
            route,
            parameters: matched.parameters().clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PbocRouteMatch {
    pub route: PbocRoute,
    pub parameters: BTreeMap<String, String>,
}

#[derive(Debug)]
pub enum PbocRouterError {
    Manifest(PbocError),
    Graph(String),
    InvalidRequest,
    Resolve(ResolveError),
}

impl PbocRouterError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Resolve(ResolveError::NotFound) => 404,
            Self::Resolve(ResolveError::MethodNotAllowed { .. }) => 405,
            Self::InvalidRequest | Self::Resolve(ResolveError::InvalidPath) => 400,
            Self::Manifest(_) | Self::Graph(_) => 500,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Resolve(ResolveError::NotFound) => "PLG-RTE-404",
            Self::Resolve(ResolveError::MethodNotAllowed { .. }) => "PLG-RTE-405",
            Self::InvalidRequest | Self::Resolve(ResolveError::InvalidPath) => "PLG-PBOC-400",
            Self::Manifest(_) => "PLG-PBOC-001",
            Self::Graph(_) => "PLG-PBOC-002",
        }
    }

    pub fn allow_header(&self) -> Option<String> {
        match self {
            Self::Resolve(ResolveError::MethodNotAllowed { allowed }) => Some(
                allowed
                    .iter()
                    .map(RouteMethod::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            _ => None,
        }
    }
}

impl Display for PbocRouterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(error) => Display::fmt(error, formatter),
            Self::Graph(error) => write!(formatter, "invalid PBOC route graph: {error}"),
            Self::InvalidRequest => formatter.write_str("invalid request method"),
            Self::Resolve(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for PbocRouterError {}
