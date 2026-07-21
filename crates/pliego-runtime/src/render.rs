// SPDX-License-Identifier: Apache-2.0

use crate::{Body, HandlerError, Response, RuntimeDiagnostic, StatusCode};
use axum::body::Bytes;
use futures_util::stream::FuturesOrdered;
use futures_util::{FutureExt, Stream, StreamExt};
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use pliego_dom::{
    Element, IntoView, RenderLimits, View, try_render_adoptable_html, try_render_html,
};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{collections::BTreeMap, collections::HashSet, collections::VecDeque};

const DOCTYPE: &str = "<!doctype html>";
const DOCUMENT_SUFFIX: &str = "</body></html>";
const MAX_DOCUMENT_ASSETS: usize = 128;
const MAX_DOCUMENT_METADATA_BYTES: usize = 64 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderMode {
    Complete,
    Ordered,
    Boundary,
    Layout,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenderSeedMode {
    #[default]
    Plain,
    Adoptable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompleteRenderOptions {
    limits: RenderLimits,
    seed_mode: RenderSeedMode,
    status: StatusCode,
}

impl CompleteRenderOptions {
    pub fn new(limits: RenderLimits) -> Self {
        Self {
            limits,
            seed_mode: RenderSeedMode::Plain,
            status: StatusCode::OK,
        }
    }

    pub fn adoptable(mut self) -> Self {
        self.seed_mode = RenderSeedMode::Adoptable;
        self
    }

    pub fn limits(self) -> RenderLimits {
        self.limits
    }

    pub fn seed_mode(self) -> RenderSeedMode {
        self.seed_mode
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn response_status(self) -> StatusCode {
        self.status
    }
}

impl Default for CompleteRenderOptions {
    fn default() -> Self {
        Self::new(RenderLimits::default())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderedRenderOptions {
    limits: RenderLimits,
    max_chunks: usize,
    status: StatusCode,
}

impl OrderedRenderOptions {
    pub const DEFAULT_MAX_CHUNKS: usize = 256;
    pub const HARD_MAX_CHUNKS: usize = 4_096;

    pub fn new(limits: RenderLimits) -> Self {
        Self {
            limits,
            max_chunks: Self::DEFAULT_MAX_CHUNKS,
            status: StatusCode::OK,
        }
    }

    pub fn with_max_chunks(mut self, maximum: usize) -> Result<Self, ServerRenderError> {
        if maximum == 0 || maximum > Self::HARD_MAX_CHUNKS {
            return Err(ServerRenderError::InvalidChunkLimit(maximum));
        }
        self.max_chunks = maximum;
        Ok(self)
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

impl Default for OrderedRenderOptions {
    fn default() -> Self {
        Self::new(RenderLimits::default())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundaryRenderOptions {
    limits: RenderLimits,
    max_boundaries: usize,
    max_in_flight: usize,
    timeout: Duration,
    status: StatusCode,
}

impl BoundaryRenderOptions {
    pub const DEFAULT_MAX_BOUNDARIES: usize = 32;
    pub const HARD_MAX_BOUNDARIES: usize = 256;
    pub const DEFAULT_MAX_IN_FLIGHT: usize = 4;
    pub const HARD_MAX_IN_FLIGHT: usize = 32;
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
    pub const HARD_MAX_TIMEOUT: Duration = Duration::from_secs(60);

    pub fn new(limits: RenderLimits) -> Self {
        Self {
            limits,
            max_boundaries: Self::DEFAULT_MAX_BOUNDARIES,
            max_in_flight: Self::DEFAULT_MAX_IN_FLIGHT,
            timeout: Self::DEFAULT_TIMEOUT,
            status: StatusCode::OK,
        }
    }

    pub fn with_max_boundaries(mut self, maximum: usize) -> Result<Self, ServerRenderError> {
        if maximum == 0 || maximum > Self::HARD_MAX_BOUNDARIES {
            return Err(ServerRenderError::InvalidBoundaryLimit {
                field: "max_boundaries",
                value: maximum,
            });
        }
        self.max_boundaries = maximum;
        Ok(self)
    }

    pub fn with_max_in_flight(mut self, maximum: usize) -> Result<Self, ServerRenderError> {
        if maximum == 0 || maximum > Self::HARD_MAX_IN_FLIGHT {
            return Err(ServerRenderError::InvalidBoundaryLimit {
                field: "max_in_flight",
                value: maximum,
            });
        }
        self.max_in_flight = maximum;
        Ok(self)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, ServerRenderError> {
        if timeout < Duration::from_millis(1) || timeout > Self::HARD_MAX_TIMEOUT {
            return Err(ServerRenderError::InvalidBoundaryTimeout(
                timeout.as_millis(),
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

impl Default for BoundaryRenderOptions {
    fn default() -> Self {
        Self::new(RenderLimits::default())
    }
}

type OrderedFactory = Box<dyn FnOnce() -> Result<View, ServerRenderError> + Send + 'static>;

pub struct OrderedViewChunk {
    factory: Option<OrderedFactory>,
}

impl OrderedViewChunk {
    pub fn new<F>(factory: F) -> Self
    where
        F: FnOnce() -> View + Send + 'static,
    {
        Self::try_new(move || Ok(factory()))
    }

    pub fn try_new<F>(factory: F) -> Self
    where
        F: FnOnce() -> Result<View, ServerRenderError> + Send + 'static,
    {
        Self {
            factory: Some(Box::new(factory)),
        }
    }

    fn render(mut self) -> Result<View, ServerRenderError> {
        self.factory
            .take()
            .expect("ordered view factory is consumed exactly once")()
    }
}

type BoundaryFuture =
    Pin<Box<dyn Future<Output = Result<OrderedViewChunk, ServerRenderError>> + Send + 'static>>;

pub struct AsyncBoundary {
    id: String,
    future: BoundaryFuture,
}

impl AsyncBoundary {
    pub fn new<F>(id: impl Into<String>, future: F) -> Result<Self, ServerRenderError>
    where
        F: Future<Output = Result<OrderedViewChunk, ServerRenderError>> + Send + 'static,
    {
        let id = id.into();
        validate_boundary_id(&id)?;
        Ok(Self {
            id,
            future: Box::pin(future),
        })
    }

    pub fn map<F, T, M>(
        id: impl Into<String>,
        future: F,
        render: M,
    ) -> Result<Self, ServerRenderError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        M: FnOnce(T) -> View + Send + 'static,
    {
        Self::new(id, async move {
            let value = future.await;
            Ok(OrderedViewChunk::new(move || render(value)))
        })
    }

    pub fn try_map<F, T, E, M>(
        id: impl Into<String>,
        future: F,
        render: M,
    ) -> Result<Self, ServerRenderError>
    where
        F: Future<Output = Result<T, E>> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
        M: FnOnce(T) -> View + Send + 'static,
    {
        let id = id.into();
        validate_boundary_id(&id)?;
        let failure_id = id.clone();
        Self::new(id, async move {
            let value = future
                .await
                .map_err(|_| ServerRenderError::BoundaryFailed { id: failure_id })?;
            Ok(OrderedViewChunk::new(move || render(value)))
        })
    }
}

#[derive(Clone)]
struct DocumentMetadata {
    language: String,
    title: String,
    description: Option<String>,
    canonical: Option<String>,
    stylesheets: Vec<String>,
    module_scripts: Vec<String>,
}

/// A bounded head contribution owned by a layout or the leaf page.
///
/// Scalar fields use inner-wins semantics. Assets retain root-to-leaf order
/// and exact duplicates are emitted once.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DocumentHead {
    language: Option<String>,
    title: Option<String>,
    description: Option<String>,
    canonical: Option<String>,
    stylesheets: Vec<String>,
    module_scripts: Vec<String>,
}

impl DocumentHead {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn canonical(mut self, canonical: impl Into<String>) -> Self {
        self.canonical = Some(canonical.into());
        self
    }

    pub fn stylesheet(mut self, path: impl Into<String>) -> Self {
        self.stylesheets.push(path.into());
        self
    }

    pub fn module_script(mut self, path: impl Into<String>) -> Self {
        self.module_scripts.push(path.into());
        self
    }
}

/// One layout declaration bound to its sealed route-graph identity.
#[derive(Clone)]
pub struct LayoutLayer {
    id: String,
    operations: Vec<LayoutOperation>,
    head: DocumentHead,
}

#[derive(Clone)]
enum LayoutOperation {
    Before(View),
    After(View),
    Wrap(Element),
}

impl LayoutLayer {
    pub fn new(id: impl Into<String>) -> Result<Self, ServerRenderError> {
        let id = id.into();
        validate_layout_id(&id)?;
        Ok(Self {
            id,
            operations: Vec::new(),
            head: DocumentHead::default(),
        })
    }

    /// Insert one sibling before the owned child frame.
    pub fn before(mut self, view: impl IntoView) -> Self {
        self.operations
            .push(LayoutOperation::Before(view.into_view()));
        self
    }

    /// Insert one sibling after the owned child frame.
    pub fn after(mut self, view: impl IntoView) -> Self {
        self.operations
            .push(LayoutOperation::After(view.into_view()));
        self
    }

    /// Wrap the complete owned child frame in one authored element.
    pub fn wrap(mut self, element: Element) -> Self {
        self.operations.push(LayoutOperation::Wrap(element));
        self
    }

    pub fn head(mut self, head: DocumentHead) -> Self {
        self.head = head;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn compose(&self, child: View) -> View {
        self.operations
            .iter()
            .fold(child, |frame, operation| match operation {
                LayoutOperation::Before(view) => View::Fragment(vec![view.clone(), frame]),
                LayoutOperation::After(view) => View::Fragment(vec![frame, view.clone()]),
                LayoutOperation::Wrap(element) => element.clone().child(frame).into_view(),
            })
    }
}

fn validate_layout_id(id: &str) -> Result<(), ServerRenderError> {
    if id.is_empty()
        || id.len() > pliego_router::MAX_ROUTE_ID_BYTES
        || !id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || id.ends_with('-')
        || id.contains("--")
    {
        return Err(ServerRenderError::InvalidLayoutId(id.to_owned()));
    }
    Ok(())
}

/// A complete document whose layout ownership comes from a sealed route match.
#[derive(Clone)]
pub struct LayoutDocument {
    expected_layouts: Vec<String>,
    layers: BTreeMap<String, LayoutLayer>,
    page_head: DocumentHead,
    body: View,
}

impl LayoutDocument {
    pub fn new(route: &pliego_router::RouteMatch, body: View) -> Self {
        Self {
            expected_layouts: route.layout_ids().to_vec(),
            layers: BTreeMap::new(),
            page_head: DocumentHead::default(),
            body,
        }
    }

    pub fn layout(mut self, layer: LayoutLayer) -> Result<Self, ServerRenderError> {
        if !self.expected_layouts.contains(&layer.id) {
            return Err(ServerRenderError::LayoutNotDeclared(layer.id));
        }
        let id = layer.id.clone();
        if self.layers.insert(id.clone(), layer).is_some() {
            return Err(ServerRenderError::DuplicateLayout(id));
        }
        Ok(self)
    }

    pub fn head(mut self, head: DocumentHead) -> Self {
        self.page_head = head;
        self
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.page_head.language = Some(language.into());
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.page_head.title = Some(title.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.page_head.description = Some(description.into());
        self
    }

    pub fn canonical(mut self, canonical: impl Into<String>) -> Self {
        self.page_head.canonical = Some(canonical.into());
        self
    }

    pub fn stylesheet(mut self, path: impl Into<String>) -> Self {
        self.page_head.stylesheets.push(path.into());
        self
    }

    pub fn module_script(mut self, path: impl Into<String>) -> Self {
        self.page_head.module_scripts.push(path.into());
        self
    }

    fn compose(&self) -> Result<CompleteDocument, ServerRenderError> {
        for id in &self.expected_layouts {
            if !self.layers.contains_key(id) {
                return Err(ServerRenderError::MissingLayout(id.clone()));
            }
        }

        let mut metadata = DocumentMetadata {
            language: "en".to_owned(),
            title: String::new(),
            description: None,
            canonical: None,
            stylesheets: Vec::new(),
            module_scripts: Vec::new(),
        };
        for id in &self.expected_layouts {
            merge_document_head(
                &mut metadata,
                &self
                    .layers
                    .get(id)
                    .expect("every expected layout was admitted")
                    .head,
            );
        }
        merge_document_head(&mut metadata, &self.page_head);
        if metadata.title.is_empty() {
            return Err(ServerRenderError::MissingDocumentTitle);
        }

        let mut body = self.body.clone();
        for id in self.expected_layouts.iter().rev() {
            let layer = self
                .layers
                .get(id)
                .expect("every expected layout was admitted");
            body = layer.compose(body);
        }
        Ok(CompleteDocument { metadata, body })
    }
}

fn merge_document_head(metadata: &mut DocumentMetadata, head: &DocumentHead) {
    if let Some(language) = &head.language {
        metadata.language.clone_from(language);
    }
    if let Some(title) = &head.title {
        metadata.title.clone_from(title);
    }
    if let Some(description) = &head.description {
        metadata.description = Some(description.clone());
    }
    if let Some(canonical) = &head.canonical {
        metadata.canonical = Some(canonical.clone());
    }
    for stylesheet in &head.stylesheets {
        if !metadata.stylesheets.contains(stylesheet) {
            metadata.stylesheets.push(stylesheet.clone());
        }
    }
    for script in &head.module_scripts {
        if !metadata.module_scripts.contains(script) {
            metadata.module_scripts.push(script.clone());
        }
    }
}

impl DocumentMetadata {
    fn new(title: impl Into<String>) -> Self {
        Self {
            language: "en".to_owned(),
            title: title.into(),
            description: None,
            canonical: None,
            stylesheets: Vec::new(),
            module_scripts: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct CompleteDocument {
    metadata: DocumentMetadata,
    body: View,
}

impl CompleteDocument {
    pub fn new(title: impl Into<String>, body: View) -> Self {
        Self {
            metadata: DocumentMetadata::new(title),
            body,
        }
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.metadata.language = language.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = Some(description.into());
        self
    }

    pub fn canonical(mut self, canonical: impl Into<String>) -> Self {
        self.metadata.canonical = Some(canonical.into());
        self
    }

    pub fn stylesheet(mut self, path: impl Into<String>) -> Self {
        self.metadata.stylesheets.push(path.into());
        self
    }

    pub fn module_script(mut self, path: impl Into<String>) -> Self {
        self.metadata.module_scripts.push(path.into());
        self
    }
}

#[derive(Clone)]
pub struct OrderedDocument {
    metadata: DocumentMetadata,
}

impl OrderedDocument {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            metadata: DocumentMetadata::new(title),
        }
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.metadata.language = language.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = Some(description.into());
        self
    }

    pub fn canonical(mut self, canonical: impl Into<String>) -> Self {
        self.metadata.canonical = Some(canonical.into());
        self
    }

    pub fn stylesheet(mut self, path: impl Into<String>) -> Self {
        self.metadata.stylesheets.push(path.into());
        self
    }

    pub fn module_script(mut self, path: impl Into<String>) -> Self {
        self.metadata.module_scripts.push(path.into());
        self
    }
}

#[derive(Clone)]
pub struct BoundaryDocument {
    metadata: DocumentMetadata,
}

impl BoundaryDocument {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            metadata: DocumentMetadata::new(title),
        }
    }

    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.metadata.language = language.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = Some(description.into());
        self
    }

    pub fn canonical(mut self, canonical: impl Into<String>) -> Self {
        self.metadata.canonical = Some(canonical.into());
        self
    }

    pub fn stylesheet(mut self, path: impl Into<String>) -> Self {
        self.metadata.stylesheets.push(path.into());
        self
    }

    pub fn module_script(mut self, path: impl Into<String>) -> Self {
        self.metadata.module_scripts.push(path.into());
        self
    }
}

pub fn render_complete_fragment(
    view: &View,
    options: CompleteRenderOptions,
) -> Result<Response<Body>, HandlerError> {
    let html = render_view(view, options).map_err(ServerRenderError::into_handler_error)?;
    html_response(html, options.response_status()).map_err(ServerRenderError::into_handler_error)
}

pub fn render_complete_document(
    document: &CompleteDocument,
    options: CompleteRenderOptions,
) -> Result<Response<Body>, HandlerError> {
    let prefix =
        document_prefix(&document.metadata).map_err(ServerRenderError::into_handler_error)?;
    let overhead = prefix
        .len()
        .checked_add(DOCUMENT_SUFFIX.len())
        .ok_or(ServerRenderError::OutputLimitTooSmall)
        .map_err(ServerRenderError::into_handler_error)?;
    let available = options
        .limits()
        .max_output_bytes()
        .checked_sub(overhead)
        .ok_or(ServerRenderError::OutputLimitTooSmall)
        .map_err(ServerRenderError::into_handler_error)?;
    let limits = RenderLimits::new(
        options.limits().max_depth(),
        options.limits().max_nodes(),
        available,
    )
    .map_err(ServerRenderError::InvalidLimits)
    .map_err(ServerRenderError::into_handler_error)?;
    let body = render_view(&document.body, CompleteRenderOptions { limits, ..options })
        .map_err(ServerRenderError::into_handler_error)?;
    let mut html = String::with_capacity(overhead + body.len());
    html.push_str(&prefix);
    html.push_str(&body);
    html.push_str(DOCUMENT_SUFFIX);
    html_response(html, options.response_status()).map_err(ServerRenderError::into_handler_error)
}

pub fn render_layout_document(
    document: &LayoutDocument,
    options: CompleteRenderOptions,
) -> Result<Response<Body>, HandlerError> {
    let complete = document
        .compose()
        .map_err(ServerRenderError::into_handler_error)?;
    let mut response = render_complete_document(&complete, options)?;
    response.extensions_mut().insert(RenderMode::Layout);
    Ok(response)
}

pub fn render_ordered_document<S>(
    document: &OrderedDocument,
    chunks: S,
    options: OrderedRenderOptions,
) -> Result<Response<Body>, HandlerError>
where
    S: Stream<Item = OrderedViewChunk> + Send + 'static,
{
    validate_body_status(options.status).map_err(ServerRenderError::into_handler_error)?;
    let prefix =
        document_prefix(&document.metadata).map_err(ServerRenderError::into_handler_error)?;
    let overhead = prefix
        .len()
        .checked_add(DOCUMENT_SUFFIX.len())
        .ok_or(ServerRenderError::OutputLimitTooSmall)
        .map_err(ServerRenderError::into_handler_error)?;
    let remaining = options
        .limits
        .max_output_bytes()
        .checked_sub(overhead)
        .ok_or(ServerRenderError::OutputLimitTooSmall)
        .map_err(ServerRenderError::into_handler_error)?;
    let stream = OrderedBodyStream {
        input: Box::pin(chunks),
        phase: OrderedPhase::Prefix,
        prefix: Some(Bytes::from(prefix)),
        remaining,
        chunks: 0,
        options,
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = options.status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response.extensions_mut().insert(RenderMode::Ordered);
    Ok(response)
}

pub fn render_boundary_document<I>(
    document: &BoundaryDocument,
    boundaries: I,
    options: BoundaryRenderOptions,
) -> Result<Response<Body>, HandlerError>
where
    I: IntoIterator<Item = AsyncBoundary>,
{
    validate_body_status(options.status).map_err(ServerRenderError::into_handler_error)?;
    let prefix =
        document_prefix(&document.metadata).map_err(ServerRenderError::into_handler_error)?;
    let mut pending = VecDeque::new();
    let mut ids = HashSet::new();
    let mut placeholder_bytes = 0usize;
    let mut input = boundaries.into_iter();
    for _ in 0..options.max_boundaries {
        let Some(boundary) = input.next() else {
            break;
        };
        if !ids.insert(boundary.id.clone()) {
            return Err(ServerRenderError::DuplicateBoundaryId(boundary.id).into_handler_error());
        }
        placeholder_bytes = placeholder_bytes
            .checked_add(boundary_placeholder(&boundary.id).len())
            .ok_or(ServerRenderError::OutputLimitTooSmall)
            .map_err(ServerRenderError::into_handler_error)?;
        pending.push_back(boundary);
    }
    if input.next().is_some() {
        return Err(ServerRenderError::TooManyBoundaries {
            maximum: options.max_boundaries,
        }
        .into_handler_error());
    }

    let overhead = prefix
        .len()
        .checked_add(DOCUMENT_SUFFIX.len())
        .and_then(|value| value.checked_add(placeholder_bytes))
        .ok_or(ServerRenderError::OutputLimitTooSmall)
        .map_err(ServerRenderError::into_handler_error)?;
    let remaining = options
        .limits
        .max_output_bytes()
        .checked_sub(overhead)
        .ok_or(ServerRenderError::OutputLimitTooSmall)
        .map_err(ServerRenderError::into_handler_error)?;

    let mut stream = BoundaryBodyStream {
        pending,
        in_flight: FuturesOrdered::new(),
        active_ids: VecDeque::new(),
        phase: BoundaryPhase::Prefix,
        prefix: Some(Bytes::from(prefix)),
        remaining,
        options,
    };
    stream.fill_in_flight();

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = options.status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response.extensions_mut().insert(RenderMode::Boundary);
    Ok(response)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OrderedPhase {
    Prefix,
    Chunks,
    Suffix,
    Done,
}

struct OrderedBodyStream<S> {
    input: Pin<Box<S>>,
    phase: OrderedPhase,
    prefix: Option<Bytes>,
    remaining: usize,
    chunks: usize,
    options: OrderedRenderOptions,
}

impl<S> Stream for OrderedBodyStream<S>
where
    S: Stream<Item = OrderedViewChunk>,
{
    type Item = Result<Bytes, ServerRenderError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let state = self.get_mut();
        loop {
            match state.phase {
                OrderedPhase::Prefix => {
                    state.phase = OrderedPhase::Chunks;
                    return Poll::Ready(state.prefix.take().map(Ok));
                }
                OrderedPhase::Chunks => {
                    let polled =
                        catch_unwind(AssertUnwindSafe(|| state.input.as_mut().poll_next(context)));
                    let chunk = match polled {
                        Ok(Poll::Ready(Some(chunk))) => chunk,
                        Ok(Poll::Ready(None)) => {
                            state.phase = OrderedPhase::Suffix;
                            continue;
                        }
                        Ok(Poll::Pending) => return Poll::Pending,
                        Err(_) => {
                            state.phase = OrderedPhase::Done;
                            return Poll::Ready(Some(Err(ServerRenderError::OrderedStreamPanic)));
                        }
                    };
                    state.chunks += 1;
                    if state.chunks > state.options.max_chunks {
                        state.phase = OrderedPhase::Done;
                        return Poll::Ready(Some(Err(ServerRenderError::TooManyChunks {
                            maximum: state.options.max_chunks,
                        })));
                    }
                    let limits = match RenderLimits::new(
                        state.options.limits.max_depth(),
                        state.options.limits.max_nodes(),
                        state.remaining,
                    ) {
                        Ok(limits) => limits,
                        Err(error) => {
                            state.phase = OrderedPhase::Done;
                            return Poll::Ready(Some(Err(ServerRenderError::InvalidLimits(error))));
                        }
                    };
                    let rendered = catch_unwind(AssertUnwindSafe(|| {
                        let view = chunk.render()?;
                        render_view(&view, CompleteRenderOptions::new(limits))
                    }));
                    let rendered = match rendered {
                        Ok(Ok(rendered)) => rendered,
                        Ok(Err(error)) => {
                            state.phase = OrderedPhase::Done;
                            return Poll::Ready(Some(Err(error)));
                        }
                        Err(_) => {
                            state.phase = OrderedPhase::Done;
                            return Poll::Ready(Some(Err(ServerRenderError::OrderedChunkPanic)));
                        }
                    };
                    state.remaining -= rendered.len();
                    if rendered.is_empty() {
                        continue;
                    }
                    return Poll::Ready(Some(Ok(Bytes::from(rendered))));
                }
                OrderedPhase::Suffix => {
                    state.phase = OrderedPhase::Done;
                    return Poll::Ready(Some(Ok(Bytes::from_static(DOCUMENT_SUFFIX.as_bytes()))));
                }
                OrderedPhase::Done => return Poll::Ready(None),
            }
        }
    }
}

type BoundaryTask = Pin<
    Box<
        dyn Future<Output = (String, Result<OrderedViewChunk, ServerRenderError>)> + Send + 'static,
    >,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoundaryPhase {
    Prefix,
    Placeholder,
    Resolution,
    Suffix,
    Done,
}

struct BoundaryBodyStream {
    pending: VecDeque<AsyncBoundary>,
    in_flight: FuturesOrdered<BoundaryTask>,
    active_ids: VecDeque<String>,
    phase: BoundaryPhase,
    prefix: Option<Bytes>,
    remaining: usize,
    options: BoundaryRenderOptions,
}

impl BoundaryBodyStream {
    fn fill_in_flight(&mut self) {
        while self.active_ids.len() < self.options.max_in_flight {
            let Some(boundary) = self.pending.pop_front() else {
                break;
            };
            let id = boundary.id;
            let timeout = self.options.timeout;
            let future = boundary.future;
            let task_id = id.clone();
            let task = async move {
                let guarded = AssertUnwindSafe(future).catch_unwind();
                let result = match tokio::time::timeout(timeout, guarded).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err(ServerRenderError::BoundaryFuturePanic {
                        id: task_id.clone(),
                    }),
                    Err(_) => Err(ServerRenderError::BoundaryTimeout {
                        id: task_id.clone(),
                        timeout_ms: timeout.as_millis() as u64,
                    }),
                };
                (task_id, result)
            };
            self.active_ids.push_back(id);
            self.in_flight.push_back(Box::pin(task));
        }
    }
}

impl Stream for BoundaryBodyStream {
    type Item = Result<Bytes, ServerRenderError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let state = self.get_mut();
        loop {
            match state.phase {
                BoundaryPhase::Prefix => {
                    state.phase = BoundaryPhase::Placeholder;
                    return Poll::Ready(state.prefix.take().map(Ok));
                }
                BoundaryPhase::Placeholder => {
                    let Some(id) = state.active_ids.front() else {
                        state.phase = BoundaryPhase::Suffix;
                        continue;
                    };
                    state.phase = BoundaryPhase::Resolution;
                    return Poll::Ready(Some(Ok(Bytes::from(boundary_placeholder(id)))));
                }
                BoundaryPhase::Resolution => {
                    let polled = catch_unwind(AssertUnwindSafe(|| {
                        state.in_flight.poll_next_unpin(context)
                    }));
                    let (id, chunk) = match polled {
                        Ok(Poll::Ready(Some(item))) => item,
                        Ok(Poll::Ready(None)) => {
                            state.phase = BoundaryPhase::Done;
                            return Poll::Ready(Some(Err(
                                ServerRenderError::BoundaryStreamEndedEarly,
                            )));
                        }
                        Ok(Poll::Pending) => return Poll::Pending,
                        Err(_) => {
                            state.phase = BoundaryPhase::Done;
                            return Poll::Ready(Some(Err(ServerRenderError::BoundaryStreamPanic)));
                        }
                    };
                    let expected = state
                        .active_ids
                        .pop_front()
                        .expect("a boundary task always has a declared identity");
                    if id != expected {
                        state.phase = BoundaryPhase::Done;
                        return Poll::Ready(Some(Err(ServerRenderError::BoundaryOrderViolation)));
                    }
                    state.fill_in_flight();
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            state.phase = BoundaryPhase::Done;
                            return Poll::Ready(Some(Err(error)));
                        }
                    };
                    let limits = match RenderLimits::new(
                        state.options.limits.max_depth(),
                        state.options.limits.max_nodes(),
                        state.remaining,
                    ) {
                        Ok(limits) => limits,
                        Err(error) => {
                            state.phase = BoundaryPhase::Done;
                            return Poll::Ready(Some(Err(ServerRenderError::InvalidLimits(error))));
                        }
                    };
                    let rendered = catch_unwind(AssertUnwindSafe(|| {
                        let view = chunk.render()?;
                        render_view(&view, CompleteRenderOptions::new(limits))
                    }));
                    let rendered = match rendered {
                        Ok(Ok(rendered)) => rendered,
                        Ok(Err(error)) => {
                            state.phase = BoundaryPhase::Done;
                            return Poll::Ready(Some(Err(error)));
                        }
                        Err(_) => {
                            state.phase = BoundaryPhase::Done;
                            return Poll::Ready(Some(Err(ServerRenderError::BoundaryViewPanic {
                                id,
                            })));
                        }
                    };
                    state.remaining -= rendered.len();
                    state.phase = BoundaryPhase::Placeholder;
                    if rendered.is_empty() {
                        continue;
                    }
                    return Poll::Ready(Some(Ok(Bytes::from(rendered))));
                }
                BoundaryPhase::Suffix => {
                    state.phase = BoundaryPhase::Done;
                    return Poll::Ready(Some(Ok(Bytes::from_static(DOCUMENT_SUFFIX.as_bytes()))));
                }
                BoundaryPhase::Done => return Poll::Ready(None),
            }
        }
    }
}

fn boundary_placeholder(id: &str) -> String {
    format!("<template data-pliego-boundary=\"{id}\"></template>")
}

fn validate_boundary_id(id: &str) -> Result<(), ServerRenderError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ServerRenderError::InvalidBoundaryId(id.to_owned()));
    }
    Ok(())
}

fn document_prefix(document: &DocumentMetadata) -> Result<String, ServerRenderError> {
    validate_document_budget(document)?;
    validate_language(&document.language)?;
    validate_text("title", &document.title, 1_024)?;
    if let Some(description) = &document.description {
        validate_text("description", description, 4_096)?;
    }
    if let Some(canonical) = &document.canonical {
        validate_canonical(canonical)?;
    }
    for path in document
        .stylesheets
        .iter()
        .chain(document.module_scripts.iter())
    {
        validate_asset_path(path)?;
    }

    let mut output = String::from(DOCTYPE);
    output.push_str("<html lang=\"");
    push_escaped_attribute(&mut output, &document.language);
    output.push_str("\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>");
    push_escaped_text(&mut output, &document.title);
    output.push_str("</title>");
    if let Some(description) = &document.description {
        output.push_str("<meta name=\"description\" content=\"");
        push_escaped_attribute(&mut output, description);
        output.push_str("\">");
    }
    if let Some(canonical) = &document.canonical {
        output.push_str("<link rel=\"canonical\" href=\"");
        push_escaped_attribute(&mut output, canonical);
        output.push_str("\">");
    }
    for stylesheet in &document.stylesheets {
        output.push_str("<link rel=\"stylesheet\" href=\"");
        push_escaped_attribute(&mut output, stylesheet);
        output.push_str("\">");
    }
    for script in &document.module_scripts {
        output.push_str("<script type=\"module\" src=\"");
        push_escaped_attribute(&mut output, script);
        output.push_str("\"></script>");
    }
    output.push_str("</head><body>");
    Ok(output)
}

fn validate_document_budget(document: &DocumentMetadata) -> Result<(), ServerRenderError> {
    let asset_count = document
        .stylesheets
        .len()
        .checked_add(document.module_scripts.len())
        .ok_or(ServerRenderError::DocumentMetadataLimit {
            maximum: MAX_DOCUMENT_ASSETS,
        })?;
    if asset_count > MAX_DOCUMENT_ASSETS {
        return Err(ServerRenderError::DocumentMetadataLimit {
            maximum: MAX_DOCUMENT_ASSETS,
        });
    }
    let mut bytes = document
        .language
        .len()
        .checked_add(document.title.len())
        .ok_or(ServerRenderError::DocumentMetadataLimit {
            maximum: MAX_DOCUMENT_METADATA_BYTES,
        })?;
    for value in document
        .description
        .iter()
        .chain(document.canonical.iter())
        .chain(document.stylesheets.iter())
        .chain(document.module_scripts.iter())
    {
        bytes = bytes
            .checked_add(value.len())
            .ok_or(ServerRenderError::DocumentMetadataLimit {
                maximum: MAX_DOCUMENT_METADATA_BYTES,
            })?;
    }
    if bytes > MAX_DOCUMENT_METADATA_BYTES {
        return Err(ServerRenderError::DocumentMetadataLimit {
            maximum: MAX_DOCUMENT_METADATA_BYTES,
        });
    }
    Ok(())
}

fn render_view(view: &View, options: CompleteRenderOptions) -> Result<String, ServerRenderError> {
    match options.seed_mode() {
        RenderSeedMode::Plain => try_render_html(view, options.limits()),
        RenderSeedMode::Adoptable => try_render_adoptable_html(view, options.limits()),
    }
    .map_err(ServerRenderError::Dom)
}

fn validate_language(language: &str) -> Result<(), ServerRenderError> {
    if language.is_empty()
        || language.len() > 64
        || !language
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || language.ends_with('-')
        || language.contains("--")
    {
        return Err(ServerRenderError::InvalidDocumentField("language"));
    }
    Ok(())
}

fn validate_text(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), ServerRenderError> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(ServerRenderError::InvalidDocumentField(field));
    }
    Ok(())
}

fn validate_canonical(value: &str) -> Result<(), ServerRenderError> {
    if value.is_empty()
        || value.len() > 2_048
        || value.chars().any(char::is_control)
        || value.contains('\\')
    {
        return Err(ServerRenderError::InvalidCanonicalUrl);
    }
    let uri = value
        .parse::<http::Uri>()
        .map_err(|_| ServerRenderError::InvalidCanonicalUrl)?;
    if value.starts_with('/') {
        if value.starts_with("//") || uri.authority().is_some() {
            return Err(ServerRenderError::InvalidCanonicalUrl);
        }
        return Ok(());
    }
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
        return Err(ServerRenderError::InvalidCanonicalUrl);
    }
    Ok(())
}

fn validate_asset_path(value: &str) -> Result<(), ServerRenderError> {
    if value.is_empty()
        || value.len() > 2_048
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value.parse::<http::Uri>().is_err()
    {
        return Err(ServerRenderError::InvalidAssetPath);
    }
    Ok(())
}

fn push_escaped_text(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}

fn push_escaped_attribute(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
}

fn html_response(html: String, status: StatusCode) -> Result<Response<Body>, ServerRenderError> {
    validate_body_status(status)?;
    let length = html.len();
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response.headers_mut().insert(
        CONTENT_LENGTH,
        http::HeaderValue::from_str(&length.to_string()).expect("usize is a valid header value"),
    );
    response.extensions_mut().insert(RenderMode::Complete);
    Ok(response)
}

fn validate_body_status(status: StatusCode) -> Result<(), ServerRenderError> {
    if status.is_informational()
        || matches!(
            status,
            StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
        )
    {
        return Err(ServerRenderError::InvalidResponseStatus(status));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerRenderError {
    InvalidDocumentField(&'static str),
    InvalidCanonicalUrl,
    InvalidAssetPath,
    InvalidResponseStatus(StatusCode),
    InvalidChunkLimit(usize),
    InvalidBoundaryLimit { field: &'static str, value: usize },
    InvalidBoundaryTimeout(u128),
    InvalidBoundaryId(String),
    DuplicateBoundaryId(String),
    InvalidLayoutId(String),
    LayoutNotDeclared(String),
    DuplicateLayout(String),
    MissingLayout(String),
    MissingDocumentTitle,
    DocumentMetadataLimit { maximum: usize },
    TooManyChunks { maximum: usize },
    TooManyBoundaries { maximum: usize },
    OrderedChunkPanic,
    OrderedStreamPanic,
    BoundaryTimeout { id: String, timeout_ms: u64 },
    BoundaryFuturePanic { id: String },
    BoundaryFailed { id: String },
    BoundaryViewPanic { id: String },
    BoundaryStreamPanic,
    BoundaryStreamEndedEarly,
    BoundaryOrderViolation,
    OutputLimitTooSmall,
    InvalidLimits(pliego_dom::RenderLimitsError),
    Dom(pliego_dom::RenderError),
}

impl ServerRenderError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocumentField(_) => "PLG-REN-001",
            Self::OutputLimitTooSmall | Self::InvalidLimits(_) => "PLG-REN-002",
            Self::InvalidCanonicalUrl | Self::InvalidAssetPath => "PLG-REN-003",
            Self::InvalidResponseStatus(_) => "PLG-REN-004",
            Self::InvalidChunkLimit(_) => "PLG-REN-005",
            Self::InvalidBoundaryLimit { .. }
            | Self::InvalidBoundaryTimeout(_)
            | Self::InvalidBoundaryId(_)
            | Self::DuplicateBoundaryId(_) => "PLG-REN-007",
            Self::InvalidLayoutId(_)
            | Self::LayoutNotDeclared(_)
            | Self::DuplicateLayout(_)
            | Self::MissingLayout(_)
            | Self::MissingDocumentTitle => "PLG-REN-008",
            Self::DocumentMetadataLimit { .. } => "PLG-REN-006",
            Self::TooManyChunks { .. } => "PLG-REN-202",
            Self::TooManyBoundaries { .. } => "PLG-REN-205",
            Self::OrderedChunkPanic => "PLG-REN-203",
            Self::OrderedStreamPanic => "PLG-REN-204",
            Self::BoundaryTimeout { .. } => "PLG-REN-206",
            Self::BoundaryFuturePanic { .. } => "PLG-REN-207",
            Self::BoundaryFailed { .. } => "PLG-REN-210",
            Self::BoundaryViewPanic { .. } => "PLG-REN-208",
            Self::BoundaryStreamPanic
            | Self::BoundaryStreamEndedEarly
            | Self::BoundaryOrderViolation => "PLG-REN-209",
            Self::Dom(_) => "PLG-REN-201",
        }
    }

    fn into_handler_error(self) -> HandlerError {
        let message = bounded(&self.to_string(), 320);
        let diagnostic =
            RuntimeDiagnostic::new(self.code(), message).expect("render diagnostics are bounded");
        HandlerError::new(StatusCode::INTERNAL_SERVER_ERROR, diagnostic)
    }
}

impl Display for ServerRenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDocumentField(field) => {
                write!(formatter, "invalid complete document field: {field}")
            }
            Self::InvalidCanonicalUrl => {
                formatter.write_str("invalid complete document canonical URL")
            }
            Self::InvalidAssetPath => {
                formatter.write_str("complete document assets require a local absolute path")
            }
            Self::InvalidResponseStatus(status) => {
                write!(formatter, "HTTP status {status} cannot carry rendered HTML")
            }
            Self::InvalidChunkLimit(maximum) => {
                write!(formatter, "invalid ordered render chunk limit: {maximum}")
            }
            Self::InvalidBoundaryLimit { field, value } => {
                write!(formatter, "invalid boundary render {field}: {value}")
            }
            Self::InvalidBoundaryTimeout(milliseconds) => {
                write!(formatter, "invalid boundary timeout: {milliseconds}ms")
            }
            Self::InvalidBoundaryId(id) => {
                write!(formatter, "invalid async boundary identity: {id:?}")
            }
            Self::DuplicateBoundaryId(id) => {
                write!(formatter, "duplicate async boundary identity: {id}")
            }
            Self::InvalidLayoutId(id) => {
                write!(formatter, "invalid layout identity: {id:?}")
            }
            Self::LayoutNotDeclared(id) => {
                write!(formatter, "layout {id} is not owned by the matched route")
            }
            Self::DuplicateLayout(id) => {
                write!(formatter, "layout {id} was supplied more than once")
            }
            Self::MissingLayout(id) => {
                write!(formatter, "matched route requires missing layout {id}")
            }
            Self::MissingDocumentTitle => {
                formatter.write_str("layout document requires a page or layout title")
            }
            Self::DocumentMetadataLimit { maximum } => {
                write!(
                    formatter,
                    "document metadata exceeded bounded limit {maximum}"
                )
            }
            Self::TooManyChunks { maximum } => {
                write!(formatter, "ordered render exceeded {maximum} chunks")
            }
            Self::TooManyBoundaries { maximum } => {
                write!(formatter, "boundary render exceeded {maximum} declarations")
            }
            Self::OrderedChunkPanic => formatter.write_str("ordered render chunk panicked"),
            Self::OrderedStreamPanic => formatter.write_str("ordered render input stream panicked"),
            Self::BoundaryTimeout { id, timeout_ms } => {
                write!(formatter, "async boundary {id} exceeded {timeout_ms}ms")
            }
            Self::BoundaryFuturePanic { id } => {
                write!(formatter, "async boundary {id} panicked while being polled")
            }
            Self::BoundaryFailed { id } => {
                write!(
                    formatter,
                    "async boundary {id} returned an application failure"
                )
            }
            Self::BoundaryViewPanic { id } => {
                write!(formatter, "async boundary {id} panicked while rendering")
            }
            Self::BoundaryStreamPanic => {
                formatter.write_str("boundary scheduler panicked while being polled")
            }
            Self::BoundaryStreamEndedEarly => {
                formatter.write_str("boundary scheduler ended before its declarations")
            }
            Self::BoundaryOrderViolation => {
                formatter.write_str("boundary scheduler violated declaration order")
            }
            Self::OutputLimitTooSmall => {
                formatter.write_str("render output limit cannot contain the HTML doctype")
            }
            Self::InvalidLimits(error) => Display::fmt(error, formatter),
            Self::Dom(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ServerRenderError {}

impl From<ServerRenderError> for HandlerError {
    fn from(error: ServerRenderError) -> Self {
        error.into_handler_error()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use http_body::Body as _;
    use http_body_util::BodyExt;
    use pliego_dom::{IntoView, el, text};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    #[tokio::test]
    async fn complete_document_is_bounded_escaped_and_tagged() {
        let document = CompleteDocument::new("A & B", el("main").child("<safe>").into_view())
            .language("es-CO")
            .description("A \"bounded\" document")
            .canonical("https://pliegors.dev/docs?mode=complete")
            .stylesheet("/assets/site.css")
            .module_script("/assets/site.js");
        let response =
            render_complete_document(&document, CompleteRenderOptions::default()).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.extensions().get::<RenderMode>(),
            Some(&RenderMode::Complete)
        );
        let length = response.headers()[CONTENT_LENGTH]
            .to_str()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        assert_eq!(
            length,
            response.body().size_hint().exact().unwrap() as usize
        );
        let html = axum::body::to_bytes(response.into_body(), 8 * 1024)
            .await
            .unwrap();
        let html = std::str::from_utf8(&html).unwrap();
        assert!(html.starts_with("<!doctype html><html lang=\"es-CO\">"));
        assert!(html.contains("<title>A &amp; B</title>"));
        assert!(html.contains("content=\"A &quot;bounded&quot; document\""));
        assert!(html.contains("<main>&lt;safe&gt;</main>"));
    }

    #[tokio::test]
    async fn layout_document_follows_sealed_ownership_and_merges_head_inner_first() {
        use pliego_router::{
            RouteGraphBuilder, RouteMethod, RouteScopeKind, RouteScopeSpec, RouteSpec,
        };

        let graph = RouteGraphBuilder::new()
            .scope(RouteScopeSpec::new("site-group", RouteScopeKind::Group).unwrap())
            .scope(
                RouteScopeSpec::new("root-layout", RouteScopeKind::Layout)
                    .unwrap()
                    .parent("site-group")
                    .unwrap(),
            )
            .scope(
                RouteScopeSpec::new("docs-layout", RouteScopeKind::Layout)
                    .unwrap()
                    .parent("root-layout")
                    .unwrap(),
            )
            .route(
                RouteSpec::new("guide", RouteMethod::get(), "/guide")
                    .unwrap()
                    .scope("docs-layout")
                    .unwrap(),
            )
            .seal()
            .unwrap();
        let matched = graph.resolve(&RouteMethod::get(), "/guide").unwrap();
        assert_eq!(
            matched.layout_ids(),
            &["root-layout".to_owned(), "docs-layout".to_owned()]
        );

        let root = LayoutLayer::new("root-layout")
            .unwrap()
            .before(el("nav").child("PLIEGO"))
            .wrap(el("div").class("root"))
            .head(
                DocumentHead::new()
                    .language("es-CO")
                    .title("Root fallback")
                    .stylesheet("/assets/root.css"),
            );
        let docs = LayoutLayer::new("docs-layout")
            .unwrap()
            .wrap(el("section").class("docs"))
            .head(
                DocumentHead::new()
                    .title("Docs fallback")
                    .description("Layout description")
                    .module_script("/assets/docs.js"),
            );
        let document = LayoutDocument::new(&matched, el("article").child("Owned page").into_view())
            .layout(root)
            .unwrap()
            .layout(docs)
            .unwrap()
            .title("Guide")
            .stylesheet("/assets/root.css")
            .stylesheet("/assets/page.css");

        let response = render_layout_document(&document, CompleteRenderOptions::default()).unwrap();
        assert_eq!(
            response.extensions().get::<RenderMode>(),
            Some(&RenderMode::Layout)
        );
        let html = axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .unwrap();
        let html = std::str::from_utf8(&html).unwrap();
        assert!(html.starts_with("<!doctype html><html lang=\"es-CO\">"));
        assert!(html.contains("<title>Guide</title>"));
        assert_eq!(html.matches("/assets/root.css").count(), 1);
        assert!(html.contains("/assets/page.css"));
        assert!(html.contains("/assets/docs.js"));
        assert_eq!(html.matches("Owned page").count(), 1);
        assert!(html.contains(
            "<div class=\"root\"><nav>PLIEGO</nav><section class=\"docs\"><article>Owned page</article></section></div>"
        ));
    }

    #[test]
    fn layout_document_rejects_missing_foreign_duplicate_and_invalid_layers_precommit() {
        use pliego_router::{
            RouteGraphBuilder, RouteMethod, RouteScopeKind, RouteScopeSpec, RouteSpec,
        };

        let graph = RouteGraphBuilder::new()
            .scope(RouteScopeSpec::new("app-layout", RouteScopeKind::Layout).unwrap())
            .route(
                RouteSpec::new("home", RouteMethod::get(), "/")
                    .unwrap()
                    .scope("app-layout")
                    .unwrap(),
            )
            .seal()
            .unwrap();
        let matched = graph.resolve(&RouteMethod::get(), "/").unwrap();

        let missing = LayoutDocument::new(&matched, text("page")).title("Home");
        let error = render_layout_document(&missing, CompleteRenderOptions::default()).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-008");

        let foreign = LayoutLayer::new("other-layout").unwrap();
        assert!(matches!(
            LayoutDocument::new(&matched, text("page")).layout(foreign),
            Err(ServerRenderError::LayoutNotDeclared(ref id)) if id == "other-layout"
        ));

        let layer = LayoutLayer::new("app-layout").unwrap();
        let document = LayoutDocument::new(&matched, text("page"))
            .layout(layer.clone())
            .unwrap();
        assert!(matches!(
            document.layout(layer),
            Err(ServerRenderError::DuplicateLayout(ref id)) if id == "app-layout"
        ));

        assert!(matches!(
            LayoutLayer::new("Bad_Layout"),
            Err(ServerRenderError::InvalidLayoutId(ref id)) if id == "Bad_Layout"
        ));

        let untitled = LayoutDocument::new(&matched, text("page"))
            .layout(LayoutLayer::new("app-layout").unwrap())
            .unwrap();
        let error =
            render_layout_document(&untitled, CompleteRenderOptions::default()).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-008");
    }

    #[test]
    fn document_validates_metadata_assets_and_accounts_for_shell() {
        let invalid = CompleteDocument::new("Title", el("main").into_view()).language("bad--lang");
        let error =
            render_complete_document(&invalid, CompleteRenderOptions::default()).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-001");

        let invalid =
            CompleteDocument::new("Title", el("main").into_view()).canonical("javascript:alert(1)");
        let error =
            render_complete_document(&invalid, CompleteRenderOptions::default()).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-003");

        let limits = RenderLimits::new(8, 8, DOCTYPE.len()).unwrap();
        let document = CompleteDocument::new("Title", el("main").into_view());
        let error =
            render_complete_document(&document, CompleteRenderOptions::new(limits)).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-002");

        let options = CompleteRenderOptions::default().status(StatusCode::NO_CONTENT);
        let error = render_complete_document(&document, options).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-004");

        let response = render_complete_document(
            &document,
            CompleteRenderOptions::default().status(StatusCode::NOT_FOUND),
        )
        .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let mut oversized = CompleteDocument::new("Title", el("main").into_view());
        for index in 0..=MAX_DOCUMENT_ASSETS {
            oversized = oversized.stylesheet(format!("/assets/{index}.css"));
        }
        let error =
            render_complete_document(&oversized, CompleteRenderOptions::default()).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-006");
    }

    #[test]
    fn adopts_only_when_explicit_and_maps_dom_failures() {
        let view = el("main").child(text("hello")).into_view();
        let plain = render_view(&view, CompleteRenderOptions::default()).unwrap();
        assert!(!plain.contains("pliego:ssr:v1"));
        let adoptable = render_view(&view, CompleteRenderOptions::default().adoptable()).unwrap();
        assert!(adoptable.starts_with("<!--pliego:ssr:v1-->"));

        let limits = RenderLimits::new(8, 8, 1).unwrap();
        let error =
            render_complete_fragment(&view, CompleteRenderOptions::new(limits)).unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-201");
    }

    #[tokio::test]
    async fn ordered_render_pulls_one_factory_per_body_frame() {
        let rendered = Arc::new(AtomicUsize::new(0));
        let chunks = futures_util::stream::iter((0..2).map({
            let rendered = rendered.clone();
            move |index| {
                let rendered = rendered.clone();
                OrderedViewChunk::new(move || {
                    rendered.fetch_add(1, Ordering::AcqRel);
                    el("p").child(format!("chunk-{index}")).into_view()
                })
            }
        }));
        let document = OrderedDocument::new("Stream").language("en");
        let response =
            render_ordered_document(&document, chunks, OrderedRenderOptions::default()).unwrap();
        assert_eq!(
            response.extensions().get::<RenderMode>(),
            Some(&RenderMode::Ordered)
        );
        assert!(response.headers().get(CONTENT_LENGTH).is_none());
        assert_eq!(rendered.load(Ordering::Acquire), 0);

        let mut body = response.into_body();
        let prefix = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(prefix.starts_with(DOCTYPE.as_bytes()));
        assert_eq!(rendered.load(Ordering::Acquire), 0);
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(first, "<p>chunk-0</p>");
        assert_eq!(rendered.load(Ordering::Acquire), 1);
        let second = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(second, "<p>chunk-1</p>");
        assert_eq!(rendered.load(Ordering::Acquire), 2);
        let suffix = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(suffix, DOCUMENT_SUFFIX);
        assert!(body.frame().await.is_none());
    }

    #[tokio::test]
    async fn ordered_render_bounds_chunks_output_and_panics() {
        let document = OrderedDocument::new("Bounded");
        let chunks = futures_util::stream::repeat_with(|| {
            OrderedViewChunk::new(|| el("span").child("x").into_view())
        });
        let options = OrderedRenderOptions::default().with_max_chunks(1).unwrap();
        let response = render_ordered_document(&document, chunks, options).unwrap();
        assert!(
            axum::body::to_bytes(response.into_body(), 8 * 1024)
                .await
                .is_err()
        );

        let chunks =
            futures_util::stream::iter([OrderedViewChunk::new(|| panic!("ordered factory panic"))]);
        let response =
            render_ordered_document(&document, chunks, OrderedRenderOptions::default()).unwrap();
        assert!(
            axum::body::to_bytes(response.into_body(), 8 * 1024)
                .await
                .is_err()
        );

        assert!(
            OrderedRenderOptions::default()
                .with_max_chunks(OrderedRenderOptions::HARD_MAX_CHUNKS + 1)
                .is_err()
        );
    }

    #[tokio::test]
    async fn boundary_render_starts_bounded_work_and_preserves_document_order() {
        let first_gate = Arc::new(Notify::new());
        let second_started = Arc::new(Notify::new());
        let boundaries = vec![
            AsyncBoundary::map(
                "profile",
                {
                    let first_gate = first_gate.clone();
                    async move {
                        first_gate.notified().await;
                        "first"
                    }
                },
                |value| el("p").child(value).into_view(),
            )
            .unwrap(),
            AsyncBoundary::map(
                "activity",
                {
                    let second_started = second_started.clone();
                    async move {
                        second_started.notify_one();
                        "second"
                    }
                },
                |value| el("p").child(value).into_view(),
            )
            .unwrap(),
        ];
        let document = BoundaryDocument::new("Async").language("en");
        let response = render_boundary_document(
            &document,
            boundaries,
            BoundaryRenderOptions::default()
                .with_max_in_flight(2)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            response.extensions().get::<RenderMode>(),
            Some(&RenderMode::Boundary)
        );
        assert!(response.headers().get(CONTENT_LENGTH).is_none());

        let mut body = response.into_body();
        let prefix = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(prefix.starts_with(DOCTYPE.as_bytes()));
        let placeholder = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(
            placeholder,
            "<template data-pliego-boundary=\"profile\"></template>"
        );

        let first_resolution = tokio::spawn(async move {
            let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
            (body, frame)
        });
        tokio::time::timeout(Duration::from_secs(1), second_started.notified())
            .await
            .expect("the second declared future should start while the first is pending");
        assert!(!first_resolution.is_finished());
        first_gate.notify_one();
        let (mut body, first) = first_resolution.await.unwrap();
        assert_eq!(first, "<p>first</p>");
        let placeholder = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(
            placeholder,
            "<template data-pliego-boundary=\"activity\"></template>"
        );
        let second = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(second, "<p>second</p>");
        let suffix = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(suffix, DOCUMENT_SUFFIX);
        assert!(body.frame().await.is_none());
    }

    #[tokio::test]
    async fn boundary_render_rejects_invalid_declarations_before_commitment() {
        let invalid = AsyncBoundary::map("bad id", async {}, |_| el("p").into_view());
        assert_eq!(invalid.err().unwrap().code(), "PLG-REN-007");

        let boundaries = vec![
            AsyncBoundary::map("same", async {}, |_| el("p").into_view()).unwrap(),
            AsyncBoundary::map("same", async {}, |_| el("p").into_view()).unwrap(),
        ];
        let error = render_boundary_document(
            &BoundaryDocument::new("Duplicate"),
            boundaries,
            BoundaryRenderOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-007");

        let boundaries = vec![
            AsyncBoundary::map("one", async {}, |_| el("p").into_view()).unwrap(),
            AsyncBoundary::map("two", async {}, |_| el("p").into_view()).unwrap(),
        ];
        let options = BoundaryRenderOptions::default()
            .with_max_boundaries(1)
            .unwrap();
        let error =
            render_boundary_document(&BoundaryDocument::new("Bounded"), boundaries, options)
                .unwrap_err();
        assert_eq!(error.diagnostic().code, "PLG-REN-205");
    }

    #[tokio::test]
    async fn boundary_render_enforces_the_in_flight_ceiling() {
        let first_gate = Arc::new(Notify::new());
        let second_started = Arc::new(Notify::new());
        let boundaries = [
            AsyncBoundary::map(
                "first",
                {
                    let first_gate = first_gate.clone();
                    async move { first_gate.notified().await }
                },
                |_| el("p").child("first").into_view(),
            )
            .unwrap(),
            AsyncBoundary::map(
                "second",
                {
                    let second_started = second_started.clone();
                    async move { second_started.notify_one() }
                },
                |_| el("p").child("second").into_view(),
            )
            .unwrap(),
        ];
        let response = render_boundary_document(
            &BoundaryDocument::new("Serial"),
            boundaries,
            BoundaryRenderOptions::default()
                .with_max_in_flight(1)
                .unwrap(),
        )
        .unwrap();
        let mut body = response.into_body();
        let _prefix = body.frame().await.unwrap().unwrap();
        let _first_placeholder = body.frame().await.unwrap().unwrap();
        let first_resolution = tokio::spawn(async move {
            let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
            (body, frame)
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(10), second_started.notified())
                .await
                .is_err()
        );
        first_gate.notify_one();
        let (mut body, first) = first_resolution.await.unwrap();
        assert_eq!(first, "<p>first</p>");
        let _second_placeholder = body.frame().await.unwrap().unwrap();
        let second = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(second, "<p>second</p>");
        tokio::time::timeout(Duration::from_secs(1), second_started.notified())
            .await
            .expect("the second future starts only after the first leaves the active set");
    }

    #[tokio::test]
    async fn boundary_render_terminates_after_timeout_or_panic() {
        let options = BoundaryRenderOptions::default()
            .with_timeout(Duration::from_millis(1))
            .unwrap();
        let boundary = AsyncBoundary::map("slow", futures_util::future::pending::<()>(), |_| {
            el("p").into_view()
        })
        .unwrap();
        let response =
            render_boundary_document(&BoundaryDocument::new("Timeout"), [boundary], options)
                .unwrap();
        assert!(
            axum::body::to_bytes(response.into_body(), 8 * 1024)
                .await
                .is_err()
        );

        let boundary =
            AsyncBoundary::map("panic", async { panic!("boundary future panic") }, |_| {
                el("p").into_view()
            })
            .unwrap();
        let response = render_boundary_document(
            &BoundaryDocument::new("Panic"),
            [boundary],
            BoundaryRenderOptions::default(),
        )
        .unwrap();
        assert!(
            axum::body::to_bytes(response.into_body(), 8 * 1024)
                .await
                .is_err()
        );

        let boundary = AsyncBoundary::try_map(
            "loader",
            async { Err::<(), _>("secret upstream detail") },
            |_| el("p").into_view(),
        )
        .unwrap();
        let error = boundary.future.await.err().unwrap();
        assert_eq!(error.code(), "PLG-REN-210");
        assert!(!error.to_string().contains("secret upstream detail"));
    }
}
