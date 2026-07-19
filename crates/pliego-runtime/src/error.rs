// SPDX-License-Identifier: Apache-2.0

use crate::{Body, HandlerError, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicErrorClass {
    NotFound,
    UnauthorizedOrForbidden,
    InvalidRequest,
    InternalFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicError {
    class: PublicErrorClass,
    status: StatusCode,
    code: String,
}

impl PublicError {
    pub(crate) fn new(
        class: PublicErrorClass,
        status: StatusCode,
        code: impl Into<String>,
    ) -> Self {
        Self {
            class,
            status,
            code: code.into(),
        }
    }

    pub fn class(&self) -> PublicErrorClass {
        self.class
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

#[derive(Clone)]
pub struct ErrorBoundaryContext {
    route_id: Option<String>,
}

impl ErrorBoundaryContext {
    pub(crate) fn new(route_id: Option<String>) -> Self {
        Self { route_id }
    }

    pub fn route_id(&self) -> Option<&str> {
        self.route_id.as_deref()
    }
}

pub type ErrorBoundaryFuture =
    Pin<Box<dyn Future<Output = Result<Response<Body>, HandlerError>> + Send + 'static>>;

pub trait RuntimeErrorBoundary: Send + Sync + 'static {
    fn call(&self, context: ErrorBoundaryContext, error: PublicError) -> ErrorBoundaryFuture;
}

impl<F, Fut> RuntimeErrorBoundary for F
where
    F: Fn(ErrorBoundaryContext, PublicError) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response<Body>, HandlerError>> + Send + 'static,
{
    fn call(&self, context: ErrorBoundaryContext, error: PublicError) -> ErrorBoundaryFuture {
        Box::pin(self(context, error))
    }
}
