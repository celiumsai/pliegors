// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Bounded native HTTP lifecycle for PliegoRS.
//!
//! Axum, Hyper, Tower, and Tokio own transport and execution. PliegoRS owns
//! admission, route identity, cancellation, cleanup, response commitment,
//! diagnostics, and receipts.

mod host;
mod limits;
mod scope;

pub use host::{
    HandlerError, HandlerFuture, NativeRuntime, NativeRuntimeBuilder, RuntimeBuildError,
    RuntimeHandler,
};
pub use limits::{LimitError, RequestLimits};
pub use scope::{
    CancelReason, InMemoryReceiptSink, RequestContext, RequestIdentity, RequestOutcome,
    RequestScope, RequestState, RuntimeDiagnostic, RuntimeReceipt, RuntimeReceiptSink, ScopeError,
};

pub use axum::body::Body;
pub use http::{Request, Response, StatusCode};
