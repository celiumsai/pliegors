// SPDX-License-Identifier: Apache-2.0

use crate::{Body, HandlerError, HandlerFuture, Request, RequestContext, Response};
use std::future::Future;

type NextFn = Box<dyn FnOnce(Request<Body>) -> HandlerFuture + Send + 'static>;

pub struct MiddlewareNext {
    next: Option<NextFn>,
}

impl MiddlewareNext {
    pub(crate) fn new(next: NextFn) -> Self {
        Self { next: Some(next) }
    }

    pub fn run(mut self, request: Request<Body>) -> HandlerFuture {
        self.next
            .take()
            .expect("middleware next is consumed exactly once")(request)
    }
}

pub trait RuntimeMiddleware: Send + Sync + 'static {
    fn call(
        &self,
        context: RequestContext,
        request: Request<Body>,
        next: MiddlewareNext,
    ) -> HandlerFuture;
}

impl<F, Fut> RuntimeMiddleware for F
where
    F: Fn(RequestContext, Request<Body>, MiddlewareNext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response<Body>, HandlerError>> + Send + 'static,
{
    fn call(
        &self,
        context: RequestContext,
        request: Request<Body>,
        next: MiddlewareNext,
    ) -> HandlerFuture {
        Box::pin(self(context, request, next))
    }
}
