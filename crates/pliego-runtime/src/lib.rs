// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Bounded native HTTP lifecycle for PliegoRS.
//!
//! Axum, Hyper, Tower, and Tokio own transport and execution. PliegoRS owns
//! admission, route identity, cancellation, cleanup, response commitment,
//! diagnostics, and receipts.

mod action;
mod error;
mod host;
mod limits;
mod middleware;
mod render;
mod scope;
mod session;
mod telemetry;
mod transport;
mod upload;

pub use action::{
    ActionRequestSecurity, SessionCsrfContext, action_failure_to_handler_error,
    decode_action_request, decode_multipart_action_request, decode_session_action_request,
    decode_session_multipart_action_request, progressive_action_response,
};
pub use error::{
    ErrorBoundaryContext, ErrorBoundaryFuture, PublicError, PublicErrorClass, RuntimeErrorBoundary,
};
pub use host::{
    ActionContractManifest, ActionInvalidationManifest, CacheContractManifest,
    ContractResourceRequirement, HandlerError, HandlerFuture, LoaderContractManifest,
    NativeRuntime, NativeRuntimeBuilder, RuntimeBuildError, RuntimeContractManifest,
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
pub use session::{expire_session_cookie_header, read_session_token, session_cookie_header};
pub use telemetry::{HttpScheme, OpenTelemetryConfig, OpenTelemetryConfigError, RemoteTracePolicy};
pub use transport::{TransportLimitError, TransportLimits};
pub use upload::{
    MultipartFieldKind, MultipartForm, MultipartPart, MultipartPolicy, UploadError, UploadFile,
};

pub use axum::body::Body;
pub use http::{Request, Response, StatusCode};
pub use pliego_data::{
    Action, ActionAdmission, ActionCommitHandle, ActionCommitState, ActionContentEncoding,
    ActionContext, ActionFailure, ActionFuture, ActionIdempotency, ActionInvalidationIntent,
    ActionMediaType, ActionNavigation, ActionPolicy, ActionResponse, CacheDomain, CacheError,
    CacheKey, CacheKeyInput, CacheLookup, CacheManager, CacheOutcome, CachePartition, CachePolicy,
    CacheReceipt, CacheSizeBucket, CacheStore, CacheTag, CapabilitySet, CreatedSession,
    CsrfManager, CsrfPolicy, CsrfToken, DataCancelReason, DataContext, DataError, DataIdentity,
    DataOperation, DataOutcome, DataReceipt, IdempotencyDecision, IdempotencyError, IdempotencyKey,
    IdempotencyManager, IdempotencyPartition, IdempotencyPermit, IdempotencyPolicy,
    IdempotencyStore, IdempotencyStoreRecord, InMemoryCacheStore, InMemoryIdempotencyStore,
    InMemoryInvalidationCoordinator, InMemorySessionStore, InvalidationConsistency,
    InvalidationEvent, InvalidationTargetKind, LoadedSession, Loader, LoaderContext, LoaderFuture,
    LoaderPolicy, OriginPolicy, ResourceGrant, ResourceLease, ResourceRegistry,
    ResourceRegistryBuilder, ResourceRequirement, ResourceSpec, SameSitePolicy, SecretHandle,
    SessionCookie, SessionCookiePolicy, SessionError, SessionManager, SessionPolicy, SessionStore,
    SessionToken,
};
pub use pliego_dom::{RenderLimits, View};
pub use pliego_router::{MiddlewareCapabilities, MiddlewareCapability};
