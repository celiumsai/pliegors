// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Bounded native HTTP lifecycle for PliegoRS.
//!
//! Axum, Hyper, Tower, and Tokio own transport and execution. PliegoRS owns
//! admission, route identity, cancellation, cleanup, response commitment,
//! diagnostics, and receipts.

mod error;
mod host;
mod limits;
mod middleware;
mod render;
mod scope;
mod telemetry;
mod transport;

pub use error::{
    ErrorBoundaryContext, ErrorBoundaryFuture, PublicError, PublicErrorClass, RuntimeErrorBoundary,
};
pub use host::{
    HandlerError, HandlerFuture, NativeRuntime, NativeRuntimeBuilder, RuntimeBuildError,
    RuntimeHandler,
};
pub use limits::{LimitError, RequestLimits};
pub use middleware::{MiddlewareNext, PreRouteNext, RuntimeMiddleware, RuntimePreRouteMiddleware};
pub use render::{
    AsyncBoundary, BoundaryDocument, BoundaryRenderOptions, CompleteDocument,
    CompleteRenderOptions, DocumentHead, LayoutDocument, LayoutLayer, LayoutStreamDocument,
    OrderedDocument, OrderedRenderOptions, OrderedViewChunk, RenderMode, RenderSeedMode,
    ServerRenderError, render_boundary_document, render_complete_document,
    render_complete_fragment, render_layout_boundary_document, render_layout_document,
    render_layout_ordered_document, render_ordered_document,
};
pub use scope::{
    CancelReason, InMemoryReceiptSink, PreRouteContext, RequestContext, RequestDurationBucket,
    RequestIdentity, RequestOutcome, RequestScope, RequestState, RuntimeDiagnostic, RuntimeReceipt,
    RuntimeReceiptSink, ScopeError,
};
pub use telemetry::{HttpScheme, OpenTelemetryConfig, OpenTelemetryConfigError, RemoteTracePolicy};
pub use transport::{TransportLimitError, TransportLimits};

pub use axum::body::Body;
pub use http::{Request, Response, StatusCode};
pub use pliego_dom::{RenderLimits, View};
pub use pliego_router::{MiddlewareCapabilities, MiddlewareCapability};
