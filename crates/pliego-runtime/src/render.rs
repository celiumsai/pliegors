// SPDX-License-Identifier: Apache-2.0

use crate::{Body, HandlerError, Response, RuntimeDiagnostic, StatusCode};
use axum::body::Bytes;
use futures_util::Stream;
use http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use pliego_dom::{RenderLimits, View, try_render_adoptable_html, try_render_html};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

const DOCTYPE: &str = "<!doctype html>";
const DOCUMENT_SUFFIX: &str = "</body></html>";
const MAX_DOCUMENT_ASSETS: usize = 128;
const MAX_DOCUMENT_METADATA_BYTES: usize = 64 * 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderMode {
    Complete,
    Ordered,
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

#[derive(Clone)]
struct DocumentMetadata {
    language: String,
    title: String,
    description: Option<String>,
    canonical: Option<String>,
    stylesheets: Vec<String>,
    module_scripts: Vec<String>,
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
    DocumentMetadataLimit { maximum: usize },
    TooManyChunks { maximum: usize },
    OrderedChunkPanic,
    OrderedStreamPanic,
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
            Self::DocumentMetadataLimit { .. } => "PLG-REN-006",
            Self::TooManyChunks { .. } => "PLG-REN-202",
            Self::OrderedChunkPanic => "PLG-REN-203",
            Self::OrderedStreamPanic => "PLG-REN-204",
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
            Self::DocumentMetadataLimit { maximum } => {
                write!(
                    formatter,
                    "document metadata exceeded bounded limit {maximum}"
                )
            }
            Self::TooManyChunks { maximum } => {
                write!(formatter, "ordered render exceeded {maximum} chunks")
            }
            Self::OrderedChunkPanic => formatter.write_str("ordered render chunk panicked"),
            Self::OrderedStreamPanic => formatter.write_str("ordered render input stream panicked"),
            Self::OutputLimitTooSmall => {
                formatter.write_str("render output limit cannot contain the HTML doctype")
            }
            Self::InvalidLimits(error) => Display::fmt(error, formatter),
            Self::Dom(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ServerRenderError {}

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
}
