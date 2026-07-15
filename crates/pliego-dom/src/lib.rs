// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! pliego-dom — PliegoRS's renderer (M4, docs/00 §2.3).
//!
//! The tachys lesson, our shape: **views are data**, and two walkers consume the
//! same tree:
//!
//! - [`try_render_html`] (native + wasm): walk the view once, evaluate dynamic
//!   parts untracked, validate parser-sensitive structure, and emit bounded
//!   HTML. This is an **SSR seed**, not proof that a browser adopted the same
//!   tree. R4 adoption performs a strict DOM preflight before reusing it.
//! - `mount` (wasm only): build real DOM once; every dynamic part mounts its own
//!   [`pliego_reactive::Effect`] that patches **exactly its node** (a dynamic
//!   text patches one `Text` node; a dynamic attribute sets one attribute; a
//!   dynamic subtree rebuilds only between its two comment markers). Components
//!   never re-run; there is no virtual DOM.
//!
//! Deliberately NOT generic over a renderer trait — the Leptos study showed the
//! generic-renderer path exploding compile times; two concrete walkers over one
//! validated data tree keep the implementation direct.

use std::rc::Rc;

use pliego_reactive::untrack;

mod name;

#[cfg(target_arch = "wasm32")]
mod mount;

pub use name::{
    AttributeName, ElementNamespace, EventName, MAX_DOM_NAME_BYTES, NameError, NameKind,
    NameViolation, TagName,
};

#[cfg(target_arch = "wasm32")]
pub use mount::{
    MountDiagnostic, MountError, MountOperation, MountScope, MountStructureViolation, MountedRoot,
    mount, mount_to, mount_to_body,
};

// On the browser target listeners receive the real event; natively (SSR/tests)
// they are inert and never called.
#[cfg(target_arch = "wasm32")]
pub type DomEvent = web_sys::Event;
#[cfg(not(target_arch = "wasm32"))]
pub type DomEvent = ();

type Listener = Rc<dyn Fn(DomEvent)>;

/// An attribute value: fixed, or a reactive closure (one effect per binding).
#[derive(Clone)]
pub enum AttrValue {
    Static(String),
    Dyn(Rc<dyn Fn() -> Result<String, DomError>>),
}

/// A view — the data both walkers consume.
#[derive(Clone)]
pub enum View {
    /// Static text.
    Text(String),
    /// Reactive text: mounts one effect patching one DOM `Text` node.
    DynText(Rc<dyn Fn() -> String>),
    /// An element with attributes, listeners and children.
    Element(Element),
    /// A sequence of sibling views.
    Fragment(Vec<View>),
    /// A reactive subtree: re-built (only between its markers) when its
    /// dependencies change. `<Show>`/`<For>` sugar composes on this.
    DynView(Rc<dyn Fn() -> View>),
}

/// An element under construction / in the tree.
#[derive(Clone)]
pub struct Element {
    tag: TagName,
    attrs: Vec<(AttributeName, AttrValue)>,
    listeners: Vec<(EventName, Listener)>,
    children: Vec<View>,
}

/// An error produced while constructing a safe DOM tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomError {
    InvalidName(NameError),
    ForbiddenElement {
        tag: String,
    },
    InlineEventAttribute {
        name: String,
    },
    ForbiddenAttribute {
        name: String,
    },
    DuplicateAttribute {
        name: String,
    },
    InvalidAttributeValue {
        name: String,
        violation: AttributeValueViolation,
    },
}

/// Why a generic attribute value was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributeValueViolation {
    ParserNormalizedCharacter { index: usize, character: char },
    DisallowedUrlScheme { scheme: String },
}

impl std::fmt::Display for DomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName(error) => error.fmt(f),
            Self::ForbiddenElement { tag } => write!(
                f,
                "element {tag:?} requires a future trusted/parser-aware API"
            ),
            Self::InlineEventAttribute { name } => {
                write!(f, "inline event attribute {name:?} is forbidden; use .on()")
            }
            Self::ForbiddenAttribute { name } => {
                write!(f, "attribute {name:?} requires a future trusted API")
            }
            Self::DuplicateAttribute { name } => {
                write!(f, "duplicate attribute {name:?}")
            }
            Self::InvalidAttributeValue { name, violation } => match violation {
                AttributeValueViolation::ParserNormalizedCharacter { index, character } => write!(
                    f,
                    "attribute {name:?} contains parser-normalized character {character:?} at byte {index}"
                ),
                AttributeValueViolation::DisallowedUrlScheme { scheme } => write!(
                    f,
                    "attribute {name:?} uses disallowed URL scheme {scheme:?}"
                ),
            },
        }
    }
}

impl std::error::Error for DomError {}

impl From<NameError> for DomError {
    fn from(error: NameError) -> Self {
        Self::InvalidName(error)
    }
}

/// Fallibly start building an element from a validated, inert tag name.
pub fn try_el(tag: impl AsRef<str>) -> Result<Element, DomError> {
    let tag = TagName::new(tag)?;
    if is_forbidden_element(tag.as_str()) {
        return Err(DomError::ForbiddenElement {
            tag: tag.to_string(),
        });
    }
    Ok(Element {
        tag,
        attrs: Vec::new(),
        listeners: Vec::new(),
        children: Vec::new(),
    })
}

/// Start building an element: `el("div").class("x").child(...)`.
pub fn el(tag: impl AsRef<str>) -> Element {
    try_el(tag).unwrap_or_else(|error| panic!("{error}"))
}

/// A static text view.
pub fn text(s: impl Into<String>) -> View {
    View::Text(s.into())
}

/// A reactive text view — reads inside subscribe; the produced effect patches
/// exactly one DOM text node.
pub fn dyn_text(f: impl Fn() -> String + 'static) -> View {
    View::DynText(Rc::new(f))
}

/// A reactive subtree — rebuilt between its markers when dependencies change.
pub fn dyn_view(f: impl Fn() -> View + 'static) -> View {
    View::DynView(Rc::new(f))
}

/// Conditional sugar: `show(when, then, otherwise)`.
pub fn show(
    when: impl Fn() -> bool + 'static,
    then: impl Fn() -> View + 'static,
    otherwise: impl Fn() -> View + 'static,
) -> View {
    dyn_view(move || if when() { then() } else { otherwise() })
}

/// Anything that can become a `View` (what `.child()` accepts).
pub trait IntoView {
    fn into_view(self) -> View;
}
impl IntoView for View {
    fn into_view(self) -> View {
        self
    }
}
impl IntoView for Element {
    fn into_view(self) -> View {
        View::Element(self)
    }
}
impl IntoView for &str {
    fn into_view(self) -> View {
        View::Text(self.to_string())
    }
}
impl IntoView for String {
    fn into_view(self) -> View {
        View::Text(self)
    }
}
impl IntoView for Vec<View> {
    fn into_view(self) -> View {
        View::Fragment(self)
    }
}

impl Element {
    /// The validated spelling used by SSR and DOM mounting.
    #[must_use]
    pub fn tag_name(&self) -> &TagName {
        &self.tag
    }

    /// Fallibly set a static attribute.
    pub fn try_attr(
        mut self,
        name: impl AsRef<str>,
        value: impl Into<String>,
    ) -> Result<Self, DomError> {
        let name = self.validate_new_attribute(name.as_ref())?;
        let value = value.into();
        validate_attribute_value(&name, &value)?;
        self.attrs.push((name, AttrValue::Static(value)));
        Ok(self)
    }

    /// Set a static attribute.
    #[must_use]
    pub fn attr(self, name: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.try_attr(name, value)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Fallibly bind a reactive attribute.
    pub fn try_attr_dyn(
        mut self,
        name: impl AsRef<str>,
        f: impl Fn() -> String + 'static,
    ) -> Result<Self, DomError> {
        let name = self.validate_new_attribute(name.as_ref())?;
        let guarded_name = name.clone();
        let guarded = move || {
            let value = f();
            validate_attribute_value(&guarded_name, &value)?;
            Ok(value)
        };
        self.attrs.push((name, AttrValue::Dyn(Rc::new(guarded))));
        Ok(self)
    }

    /// Bind a reactive attribute (one effect; sets exactly this attribute).
    #[must_use]
    pub fn attr_dyn(self, name: impl AsRef<str>, f: impl Fn() -> String + 'static) -> Self {
        self.try_attr_dyn(name, f)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// `class="…"` shorthand.
    #[must_use]
    pub fn class(self, value: impl Into<String>) -> Self {
        self.attr("class", value)
    }

    /// `id="…"` shorthand.
    #[must_use]
    pub fn id(self, value: impl Into<String>) -> Self {
        self.attr("id", value)
    }

    /// Fallibly append a child. Namespace-sensitive content models are checked
    /// by [`try_render_html`] and the R4 DOM adoption preflight.
    pub fn try_child(mut self, child: impl IntoView) -> Result<Self, DomError> {
        self.children.push(child.into_view());
        Ok(self)
    }

    /// Append a child.
    #[must_use]
    pub fn child(self, child: impl IntoView) -> Self {
        self.try_child(child)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Fallibly attach an event listener using a validated event name.
    pub fn try_on(
        mut self,
        event: impl AsRef<str>,
        handler: impl Fn(DomEvent) + 'static,
    ) -> Result<Self, DomError> {
        self.listeners
            .push((EventName::new(event)?, Rc::new(handler)));
        Ok(self)
    }

    /// Attach an event listener (inert in SSR; real on the browser target).
    #[must_use]
    pub fn on(self, event: impl AsRef<str>, handler: impl Fn(DomEvent) + 'static) -> Self {
        self.try_on(event, handler)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn validate_new_attribute(&self, name: &str) -> Result<AttributeName, DomError> {
        let name = AttributeName::new(name)?;
        if name
            .as_str()
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("on"))
        {
            return Err(DomError::InlineEventAttribute {
                name: name.to_string(),
            });
        }
        if name.as_str().eq_ignore_ascii_case("srcdoc") {
            return Err(DomError::ForbiddenAttribute {
                name: name.to_string(),
            });
        }
        if self
            .attrs
            .iter()
            .any(|(existing, _)| existing.as_str().eq_ignore_ascii_case(name.as_str()))
        {
            return Err(DomError::DuplicateAttribute {
                name: name.to_string(),
            });
        }
        Ok(name)
    }
}

const FORBIDDEN_ELEMENTS: &[&str] = &[
    "script",
    "style",
    "iframe",
    "fencedframe",
    "portal",
    "object",
    "embed",
    "applet",
    "xmp",
    "plaintext",
    "noembed",
    "noframes",
    "noscript",
    "template",
    "listing",
    "frameset",
    "frame",
    "isindex",
    "basefont",
    "bgsound",
    "keygen",
    "math",
    "base",
    "link",
    "meta",
    // SVG animation can assign executable URL-valued attributes indirectly
    // through `attributeName`/`values`. Keep it out of the authored surface
    // until PliegoRS has a parser-aware, attribute-specific animation API.
    "animate",
    "animateColor",
    "set",
    "animateMotion",
    "animateTransform",
    "discard",
];

fn is_forbidden_element(tag: &str) -> bool {
    FORBIDDEN_ELEMENTS
        .iter()
        .any(|forbidden| tag.eq_ignore_ascii_case(forbidden))
}

fn validate_attribute_value(name: &AttributeName, value: &str) -> Result<(), DomError> {
    if let Some((index, character)) = value
        .char_indices()
        .find(|(_, character)| matches!(character, '\0' | '\r'))
    {
        return Err(DomError::InvalidAttributeValue {
            name: name.to_string(),
            violation: AttributeValueViolation::ParserNormalizedCharacter { index, character },
        });
    }

    if name.as_str().eq_ignore_ascii_case("srcset") {
        for candidate in value.split(',') {
            if let Some(url) = candidate.split_whitespace().next() {
                validate_url_scheme(name, url)?;
            }
        }
    } else if is_url_attribute(name.as_str()) {
        validate_url_scheme(name, value)?;
    }
    Ok(())
}

fn is_url_attribute(name: &str) -> bool {
    [
        "href",
        "src",
        "action",
        "formaction",
        "poster",
        "cite",
        "background",
        "xlink:href",
    ]
    .iter()
    .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

fn validate_url_scheme(name: &AttributeName, value: &str) -> Result<(), DomError> {
    let Some(scheme) = explicit_url_scheme(value) else {
        return Ok(());
    };
    if ["http", "https", "mailto", "tel"]
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
    {
        return Ok(());
    }
    Err(DomError::InvalidAttributeValue {
        name: name.to_string(),
        violation: AttributeValueViolation::DisallowedUrlScheme { scheme },
    })
}

fn explicit_url_scheme(value: &str) -> Option<String> {
    let mut scheme = String::with_capacity(16);
    for character in value.chars() {
        if character == ' ' || character.is_ascii_control() {
            continue;
        }
        if character == ':' {
            return (!scheme.is_empty()).then_some(scheme);
        }
        if matches!(character, '/' | '?' | '#') {
            return None;
        }
        let valid = if scheme.is_empty() {
            character.is_ascii_alphabetic()
        } else {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        };
        if !valid {
            return None;
        }
        if scheme.len() < 32 {
            scheme.push(character.to_ascii_lowercase());
        }
    }
    None
}

// ───────────────────────── walker 1: bounded SSR seed (native + wasm) ─────────────────────────

pub const DEFAULT_RENDER_MAX_DEPTH: usize = 128;
pub const DEFAULT_RENDER_MAX_NODES: usize = 100_000;
pub const DEFAULT_RENDER_MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

const HARD_RENDER_MAX_DEPTH: usize = 256;
const HARD_RENDER_MAX_NODES: usize = 1_000_000;
const HARD_RENDER_MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// A resource controlled by the bounded SSR walker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderLimitKind {
    Depth,
    Nodes,
    OutputBytes,
}

impl std::fmt::Display for RenderLimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Depth => "depth",
            Self::Nodes => "nodes",
            Self::OutputBytes => "output bytes",
        })
    }
}

/// Invalid caller-provided render limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderLimitsError {
    pub kind: RenderLimitKind,
    pub requested: usize,
    pub hard_maximum: usize,
}

impl std::fmt::Display for RenderLimitsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "requested render {} {} exceeds hard maximum {}",
            self.kind, self.requested, self.hard_maximum
        )
    }
}

impl std::error::Error for RenderLimitsError {}

/// Explicit budgets for one SSR traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderLimits {
    max_depth: usize,
    max_nodes: usize,
    max_output_bytes: usize,
}

impl RenderLimits {
    pub fn new(
        max_depth: usize,
        max_nodes: usize,
        max_output_bytes: usize,
    ) -> Result<Self, RenderLimitsError> {
        for (kind, requested, hard_maximum) in [
            (RenderLimitKind::Depth, max_depth, HARD_RENDER_MAX_DEPTH),
            (RenderLimitKind::Nodes, max_nodes, HARD_RENDER_MAX_NODES),
            (
                RenderLimitKind::OutputBytes,
                max_output_bytes,
                HARD_RENDER_MAX_OUTPUT_BYTES,
            ),
        ] {
            if requested > hard_maximum {
                return Err(RenderLimitsError {
                    kind,
                    requested,
                    hard_maximum,
                });
            }
        }
        Ok(Self {
            max_depth,
            max_nodes,
            max_output_bytes,
        })
    }

    #[must_use]
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    #[must_use]
    pub const fn max_nodes(self) -> usize {
        self.max_nodes
    }

    #[must_use]
    pub const fn max_output_bytes(self) -> usize {
        self.max_output_bytes
    }
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_RENDER_MAX_DEPTH,
            max_nodes: DEFAULT_RENDER_MAX_NODES,
            max_output_bytes: DEFAULT_RENDER_MAX_OUTPUT_BYTES,
        }
    }
}

/// A bounded SSR failure. No partial output is returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    LimitExceeded {
        kind: RenderLimitKind,
        limit: usize,
    },
    InvalidAttribute(DomError),
    ParserNormalizedText {
        index: usize,
        character: char,
    },
    VoidElementChild {
        tag: String,
    },
    ParserRepair {
        parent: String,
        child: Option<String>,
        rule: &'static str,
    },
    ForbiddenElement {
        tag: String,
    },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitExceeded { kind, limit } => {
                write!(f, "render {kind} exceeded limit {limit}")
            }
            Self::InvalidAttribute(error) => error.fmt(f),
            Self::ParserNormalizedText { index, character } => write!(
                f,
                "text contains parser-normalized character {character:?} at byte {index}"
            ),
            Self::VoidElementChild { tag } => {
                write!(f, "HTML void element {tag:?} cannot have children")
            }
            Self::ParserRepair {
                parent,
                child,
                rule,
            } => {
                write!(
                    f,
                    "browser would repair child {child:?} under {parent:?}: {rule}"
                )
            }
            Self::ForbiddenElement { tag } => write!(
                f,
                "element {tag:?} requires a future trusted/parser-aware API"
            ),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<DomError> for RenderError {
    fn from(error: DomError) -> Self {
        Self::InvalidAttribute(error)
    }
}

/// Render one validated view under explicit resource budgets.
///
/// Dynamic parts are evaluated exactly once and untracked. The output has
/// passed conservative parser-repair checks, but R4 DOM adoption still performs
/// a strict preflight before reusing server nodes.
pub fn try_render_html(view: &View, limits: RenderLimits) -> Result<String, RenderError> {
    let mut state = RenderState { limits, nodes: 0 };
    let mut output = BoundedHtml::new(limits.max_output_bytes);
    let mut direct = DirectChildState::default();
    write_html(
        view,
        ElementNamespace::Html,
        1,
        None,
        &mut direct,
        &mut state,
        &mut output,
    )?;
    Ok(output.finish())
}

/// Render an authored view with the default bounded policy.
///
/// Runtime-controlled trees should call [`try_render_html`] and handle errors.
#[must_use]
pub fn render_html(view: &View) -> String {
    try_render_html(view, RenderLimits::default())
        .unwrap_or_else(|error| panic!("SSR validation failed: {error}"))
}

struct RenderState {
    limits: RenderLimits,
    nodes: usize,
}

impl RenderState {
    fn visit(&mut self, depth: usize) -> Result<(), RenderError> {
        if depth > self.limits.max_depth {
            return Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::Depth,
                limit: self.limits.max_depth,
            });
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or(RenderError::LimitExceeded {
                kind: RenderLimitKind::Nodes,
                limit: self.limits.max_nodes,
            })?;
        if self.nodes > self.limits.max_nodes {
            return Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::Nodes,
                limit: self.limits.max_nodes,
            });
        }
        Ok(())
    }
}

struct BoundedHtml {
    value: String,
    maximum: usize,
}

impl BoundedHtml {
    fn new(maximum: usize) -> Self {
        Self {
            value: String::with_capacity(maximum.min(4_096)),
            maximum,
        }
    }

    fn push_str(&mut self, value: &str) -> Result<(), RenderError> {
        let length =
            self.value
                .len()
                .checked_add(value.len())
                .ok_or(RenderError::LimitExceeded {
                    kind: RenderLimitKind::OutputBytes,
                    limit: self.maximum,
                })?;
        if length > self.maximum {
            return Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::OutputBytes,
                limit: self.maximum,
            });
        }
        self.value.push_str(value);
        Ok(())
    }

    fn push_char(&mut self, value: char) -> Result<(), RenderError> {
        let mut encoded = [0; 4];
        self.push_str(value.encode_utf8(&mut encoded))
    }

    fn push_escaped(&mut self, value: &str, attribute: bool) -> Result<(), RenderError> {
        for character in value.chars() {
            match character {
                '&' => self.push_str("&amp;")?,
                '<' => self.push_str("&lt;")?,
                '>' => self.push_str("&gt;")?,
                '"' if attribute => self.push_str("&quot;")?,
                _ => self.push_char(character)?,
            }
        }
        Ok(())
    }

    fn ensure_minimum_input_fits(&self, input_bytes: usize) -> Result<(), RenderError> {
        let minimum =
            self.value
                .len()
                .checked_add(input_bytes)
                .ok_or(RenderError::LimitExceeded {
                    kind: RenderLimitKind::OutputBytes,
                    limit: self.maximum,
                })?;
        if minimum > self.maximum {
            return Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::OutputBytes,
                limit: self.maximum,
            });
        }
        Ok(())
    }

    fn finish(self) -> String {
        self.value
    }
}

#[derive(Default)]
struct DirectChildState {
    has_serialized_content: bool,
}

#[derive(Clone, Copy)]
struct ParentContext<'a> {
    tag: &'a TagName,
    namespace: ElementNamespace,
    parser: ParserContext,
}

/// Parser state that survives through transparent descendants.
///
/// This is intentionally smaller than a full HTML tree builder. It records
/// only the open-element conditions that can make a later start tag close,
/// ignore, or reparent authored nodes. Both SSR and browser mounting consume
/// this state so they accept the same topology.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ParserContext {
    in_p: bool,
    in_button: bool,
    in_anchor: bool,
    in_form: bool,
    in_list_item: bool,
    in_definition_item: bool,
    in_nobr: bool,
    ruby: RubyParserState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum RubyParserState {
    #[default]
    None,
    Ruby,
    Rtc,
    Descendant,
}

impl ParserContext {
    pub(crate) fn descend(
        mut self,
        tag: &TagName,
        namespace: ElementNamespace,
        child_namespace: ElementNamespace,
    ) -> Self {
        if namespace != ElementNamespace::Html {
            if child_namespace == ElementNamespace::Html {
                // SVG HTML integration points are scope boundaries. The form
                // pointer is global, however, so an outer <form> still makes a
                // nested form token disappear inside foreignObject/desc/title.
                return Self {
                    in_form: self.in_form,
                    ..Self::default()
                };
            }
            return self;
        }

        let name = tag.as_str();
        if is_standard_scope_boundary(name) {
            self.in_p = false;
            self.in_button = false;
            self.in_anchor = false;
            self.in_nobr = false;
            self.ruby = RubyParserState::None;
        }
        if name.eq_ignore_ascii_case("button") {
            self.in_p = false;
        }
        if is_list_scan_barrier(name) {
            self.in_list_item = false;
            self.in_definition_item = false;
        }

        self.in_p |= name.eq_ignore_ascii_case("p");
        self.in_button |= name.eq_ignore_ascii_case("button");
        self.in_anchor |= name.eq_ignore_ascii_case("a");
        self.in_form |= name.eq_ignore_ascii_case("form");
        self.in_list_item |= name.eq_ignore_ascii_case("li");
        self.in_definition_item |= ["dt", "dd"]
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate));
        self.in_nobr |= name.eq_ignore_ascii_case("nobr");
        self.ruby = if name.eq_ignore_ascii_case("ruby") {
            RubyParserState::Ruby
        } else {
            match self.ruby {
                RubyParserState::None => RubyParserState::None,
                _ if name.eq_ignore_ascii_case("rtc") => RubyParserState::Rtc,
                _ => RubyParserState::Descendant,
            }
        };
        self
    }
}

fn write_html(
    view: &View,
    inherited_namespace: ElementNamespace,
    depth: usize,
    parent: Option<ParentContext<'_>>,
    direct: &mut DirectChildState,
    state: &mut RenderState,
    output: &mut BoundedHtml,
) -> Result<(), RenderError> {
    state.visit(depth)?;
    match view {
        View::Text(value) => write_text(value, parent, direct, output),
        View::DynText(f) => {
            let value = untrack(|| f());
            write_text(&value, parent, direct, output)
        }
        View::Fragment(children) => {
            for child in children {
                write_html(
                    child,
                    inherited_namespace,
                    depth + 1,
                    parent,
                    direct,
                    state,
                    output,
                )?;
            }
            Ok(())
        }
        View::DynView(f) => {
            let inner = untrack(|| f());
            write_html(
                &inner,
                inherited_namespace,
                depth + 1,
                parent,
                direct,
                state,
                output,
            )
        }
        View::Element(element) => {
            if is_forbidden_element(element.tag.as_str()) {
                return Err(RenderError::ForbiddenElement {
                    tag: element.tag.to_string(),
                });
            }
            let namespace = inherited_namespace.for_element(&element.tag);
            validate_parser_adjusted_svg_tag(parent, &element.tag, namespace)?;
            validate_direct_element(parent, &element.tag, namespace)?;
            direct.has_serialized_content = true;
            output.push_char('<')?;
            output.push_str(element.tag.as_str())?;
            for (name, value) in &element.attrs {
                validate_parser_adjusted_svg_attribute(&element.tag, name, namespace)?;
                let resolved;
                let value = match value {
                    AttrValue::Static(value) => value.as_str(),
                    AttrValue::Dyn(f) => {
                        resolved = untrack(|| f())?;
                        resolved.as_str()
                    }
                };
                output.ensure_minimum_input_fits(value.len())?;
                validate_attribute_value(name, value)?;
                output.push_char(' ')?;
                output.push_str(name.as_str())?;
                output.push_str("=\"")?;
                output.push_escaped(value, true)?;
                output.push_char('"')?;
            }
            output.push_char('>')?;

            if element.tag.is_void_in(namespace) {
                if !element.children.is_empty() {
                    return Err(RenderError::VoidElementChild {
                        tag: element.tag.to_string(),
                    });
                }
                return Ok(());
            }

            let child_namespace = namespace.for_children(&element.tag);
            let child_parent = ParentContext {
                tag: &element.tag,
                namespace,
                parser: parent
                    .map(|context| context.parser)
                    .unwrap_or_default()
                    .descend(&element.tag, namespace, child_namespace),
            };
            let mut child_state = DirectChildState::default();
            for child in &element.children {
                write_html(
                    child,
                    child_namespace,
                    depth + 1,
                    Some(child_parent),
                    &mut child_state,
                    state,
                    output,
                )?;
            }
            output.push_str("</")?;
            output.push_str(element.tag.as_str())?;
            output.push_char('>')
        }
    }
}

fn write_text(
    value: &str,
    parent: Option<ParentContext<'_>>,
    direct: &mut DirectChildState,
    output: &mut BoundedHtml,
) -> Result<(), RenderError> {
    output.ensure_minimum_input_fits(value.len())?;
    if let Some((index, character)) = value
        .char_indices()
        .find(|(_, character)| matches!(character, '\0' | '\r'))
    {
        return Err(RenderError::ParserNormalizedText { index, character });
    }
    validate_direct_text(parent, value, direct)?;
    output.push_escaped(value, false)?;
    if !value.is_empty() {
        direct.has_serialized_content = true;
    }
    Ok(())
}

fn validate_direct_element(
    parent: Option<ParentContext<'_>>,
    child: &TagName,
    child_namespace: ElementNamespace,
) -> Result<(), RenderError> {
    let child_name = child.as_str();
    if child_namespace == ElementNamespace::Html && child_name.eq_ignore_ascii_case("image") {
        return Err(RenderError::ParserRepair {
            parent: parent
                .map(|context| context.tag.to_string())
                .unwrap_or_else(|| "#root".to_owned()),
            child: Some(child.to_string()),
            rule: "the HTML parser rewrites <image> to <img>",
        });
    }

    if child_namespace == ElementNamespace::Html
        && ["html", "head", "body"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
    {
        return Err(RenderError::ParserRepair {
            parent: parent
                .map(|context| context.tag.to_string())
                .unwrap_or_else(|| "#root".to_owned()),
            child: Some(child.to_string()),
            rule: "document structure tags are handled specially by the HTML parser",
        });
    }

    if child_namespace == ElementNamespace::Html {
        let required_parent = if ["caption", "colgroup", "thead", "tbody", "tfoot"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
        {
            Some(("table", "table structure must be a direct child of <table>"))
        } else if child_name.eq_ignore_ascii_case("col") {
            Some(("colgroup", "<col> must be a direct child of <colgroup>"))
        } else if child_name.eq_ignore_ascii_case("tr") {
            Some((
                "table section",
                "<tr> must be a direct child of <thead>, <tbody>, or <tfoot>",
            ))
        } else if ["td", "th"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
        {
            Some(("tr", "table cells must be direct children of <tr>"))
        } else {
            None
        };

        if let Some((expected, rule)) = required_parent {
            let valid = match expected {
                "table section" => parent.is_some_and(|context| {
                    context.namespace == ElementNamespace::Html
                        && ["thead", "tbody", "tfoot"]
                            .iter()
                            .any(|tag| context.tag.as_str().eq_ignore_ascii_case(tag))
                }),
                expected => parent.is_some_and(|context| {
                    context.namespace == ElementNamespace::Html
                        && context.tag.as_str().eq_ignore_ascii_case(expected)
                }),
            };
            if !valid {
                return Err(RenderError::ParserRepair {
                    parent: parent
                        .map(|context| context.tag.to_string())
                        .unwrap_or_else(|| "#root".to_owned()),
                    child: Some(child.to_string()),
                    rule,
                });
            }
        }

        if let Some(context) = parent {
            let parser = context.parser;
            let ancestor_repair = if parser.in_p && is_p_closing_block(child_name) {
                Some(("p", "a block element implicitly closes an open <p>"))
            } else if parser.in_button && child_name.eq_ignore_ascii_case("button") {
                Some((
                    "button",
                    "a nested <button> implicitly closes the open button",
                ))
            } else if parser.in_anchor && child_name.eq_ignore_ascii_case("a") {
                Some(("a", "a nested <a> invokes the adoption agency algorithm"))
            } else if parser.in_form && child_name.eq_ignore_ascii_case("form") {
                Some(("form", "a nested <form> start tag is ignored"))
            } else if parser.in_list_item && child_name.eq_ignore_ascii_case("li") {
                Some(("li", "a new <li> implicitly closes the open list item"))
            } else if parser.in_definition_item
                && ["dt", "dd"]
                    .iter()
                    .any(|tag| child_name.eq_ignore_ascii_case(tag))
            {
                Some((
                    "dt/dd",
                    "a new definition item implicitly closes the open item",
                ))
            } else if parser.in_nobr && child_name.eq_ignore_ascii_case("nobr") {
                Some((
                    "nobr",
                    "a nested <nobr> invokes the adoption agency algorithm",
                ))
            } else if parser.ruby != RubyParserState::None
                && ruby_direct_start_tag_repairs(context.tag.as_str(), child_name)
            {
                Some((
                    "ruby",
                    "ruby text/base start tags implicitly close the current ruby segment",
                ))
            } else {
                None
            };
            if let Some((ancestor, rule)) = ancestor_repair {
                return Err(RenderError::ParserRepair {
                    parent: ancestor.to_owned(),
                    child: Some(child.to_string()),
                    rule,
                });
            }
        }
    }

    let Some(parent) = parent else {
        return Ok(());
    };
    if parent.namespace == ElementNamespace::Svg
        && child_namespace == ElementNamespace::Svg
        && is_svg_html_breakout(child.as_str())
    {
        return Err(RenderError::ParserRepair {
            parent: parent.tag.to_string(),
            child: Some(child.to_string()),
            rule: "the HTML parser exits SVG foreign content before this element",
        });
    }
    if parent.namespace != ElementNamespace::Html {
        return Ok(());
    }

    let parent_name = parent.tag.as_str();
    let invalid = if ["textarea", "title"]
        .iter()
        .any(|tag| parent_name.eq_ignore_ascii_case(tag))
    {
        Some("RCDATA elements may contain text only")
    } else if ["h1", "h2", "h3", "h4", "h5", "h6"]
        .iter()
        .any(|tag| parent_name.eq_ignore_ascii_case(tag))
        && ["h1", "h2", "h3", "h4", "h5", "h6"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
    {
        Some("a heading start tag implicitly closes the current heading")
    } else if parent_name.eq_ignore_ascii_case("table")
        && !["caption", "colgroup", "thead", "tbody", "tfoot"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
    {
        Some("table children require an explicit caption/colgroup/section")
    } else if ["thead", "tbody", "tfoot"]
        .iter()
        .any(|tag| parent_name.eq_ignore_ascii_case(tag))
        && !child_name.eq_ignore_ascii_case("tr")
    {
        Some("table sections may contain only <tr>")
    } else if parent_name.eq_ignore_ascii_case("tr")
        && !["th", "td"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
    {
        Some("table rows may contain only <th> or <td>")
    } else if parent_name.eq_ignore_ascii_case("colgroup")
        && !child_name.eq_ignore_ascii_case("col")
    {
        Some("colgroup may contain only <col>")
    } else if parent_name.eq_ignore_ascii_case("select")
        && !["option", "optgroup", "hr"]
            .iter()
            .any(|tag| child_name.eq_ignore_ascii_case(tag))
    {
        Some("select insertion mode ignores or reparents this child")
    } else if parent_name.eq_ignore_ascii_case("optgroup")
        && !child_name.eq_ignore_ascii_case("option")
    {
        Some("optgroup may contain only <option>")
    } else if parent_name.eq_ignore_ascii_case("option") {
        Some("option elements may contain text only")
    } else {
        None
    };

    if let Some(rule) = invalid {
        return Err(RenderError::ParserRepair {
            parent: parent_name.to_owned(),
            child: Some(child_name.to_owned()),
            rule,
        });
    }
    Ok(())
}

fn ruby_direct_start_tag_repairs(parent: &str, child: &str) -> bool {
    let parent_closes_for_any_segment = ["rb", "rt", "rp"]
        .iter()
        .any(|tag| parent.eq_ignore_ascii_case(tag));
    let child_is_segment = ["rb", "rt", "rp", "rtc"]
        .iter()
        .any(|tag| child.eq_ignore_ascii_case(tag));
    (parent_closes_for_any_segment && child_is_segment)
        || (parent.eq_ignore_ascii_case("rtc")
            && ["rb", "rtc"]
                .iter()
                .any(|tag| child.eq_ignore_ascii_case(tag)))
}

fn validate_parser_adjusted_svg_tag(
    parent: Option<ParentContext<'_>>,
    tag: &TagName,
    namespace: ElementNamespace,
) -> Result<(), RenderError> {
    if namespace != ElementNamespace::Svg {
        return Ok(());
    }
    let authored = tag.as_str();
    let canonical = canonical_svg_tag(authored);
    if canonical.map_or_else(
        || !authored.bytes().any(|byte| byte.is_ascii_uppercase()),
        |expected| authored == expected,
    ) {
        return Ok(());
    }
    Err(RenderError::ParserRepair {
        parent: parent
            .map(|context| context.tag.to_string())
            .unwrap_or_else(|| "#root".to_owned()),
        child: Some(tag.to_string()),
        rule: "the HTML parser adjusts this SVG tag to its canonical spelling",
    })
}

fn canonical_svg_tag(tag: &str) -> Option<&'static str> {
    [
        "altGlyph",
        "altGlyphDef",
        "altGlyphItem",
        "animateColor",
        "animateMotion",
        "animateTransform",
        "clipPath",
        "feBlend",
        "feColorMatrix",
        "feComponentTransfer",
        "feComposite",
        "feConvolveMatrix",
        "feDiffuseLighting",
        "feDisplacementMap",
        "feDistantLight",
        "feDropShadow",
        "feFlood",
        "feFuncA",
        "feFuncB",
        "feFuncG",
        "feFuncR",
        "feGaussianBlur",
        "feImage",
        "feMerge",
        "feMergeNode",
        "feMorphology",
        "feOffset",
        "fePointLight",
        "feSpecularLighting",
        "feSpotLight",
        "feTile",
        "feTurbulence",
        "foreignObject",
        "glyphRef",
        "linearGradient",
        "radialGradient",
        "textPath",
    ]
    .into_iter()
    .find(|canonical| tag.eq_ignore_ascii_case(canonical))
}

fn validate_parser_adjusted_svg_attribute(
    element: &TagName,
    name: &AttributeName,
    namespace: ElementNamespace,
) -> Result<(), RenderError> {
    if namespace != ElementNamespace::Svg {
        return Ok(());
    }

    let authored = name.as_str();
    let canonical = canonical_svg_attribute(authored);
    let foreign = [
        "xlink:actuate",
        "xlink:arcrole",
        "xlink:href",
        "xlink:role",
        "xlink:show",
        "xlink:title",
        "xlink:type",
        "xml:base",
        "xml:lang",
        "xml:space",
        "xmlns",
        "xmlns:xlink",
    ]
    .into_iter()
    .find(|candidate| authored.eq_ignore_ascii_case(candidate));

    let canonical_spelling = canonical.or(foreign);
    let is_canonical = canonical_spelling.map_or_else(
        || !authored.bytes().any(|byte| byte.is_ascii_uppercase()) && !authored.contains(':'),
        |expected| authored == expected,
    );
    if is_canonical {
        return Ok(());
    }

    Err(RenderError::ParserRepair {
        parent: element.to_string(),
        child: Some(format!("@{authored}")),
        rule: "the HTML parser adjusts this SVG attribute name or namespace",
    })
}

fn canonical_svg_attribute(name: &str) -> Option<&'static str> {
    [
        "attributeName",
        "attributeType",
        "baseFrequency",
        "baseProfile",
        "calcMode",
        "clipPathUnits",
        "diffuseConstant",
        "edgeMode",
        "filterUnits",
        "glyphRef",
        "gradientTransform",
        "gradientUnits",
        "kernelMatrix",
        "kernelUnitLength",
        "keyPoints",
        "keySplines",
        "keyTimes",
        "lengthAdjust",
        "limitingConeAngle",
        "markerHeight",
        "markerUnits",
        "markerWidth",
        "maskContentUnits",
        "maskUnits",
        "numOctaves",
        "pathLength",
        "patternContentUnits",
        "patternTransform",
        "patternUnits",
        "pointsAtX",
        "pointsAtY",
        "pointsAtZ",
        "preserveAlpha",
        "preserveAspectRatio",
        "primitiveUnits",
        "refX",
        "refY",
        "repeatCount",
        "repeatDur",
        "requiredExtensions",
        "requiredFeatures",
        "specularConstant",
        "specularExponent",
        "spreadMethod",
        "startOffset",
        "stdDeviation",
        "stitchTiles",
        "surfaceScale",
        "systemLanguage",
        "tableValues",
        "targetX",
        "targetY",
        "textLength",
        "viewBox",
        "viewTarget",
        "xChannelSelector",
        "yChannelSelector",
        "zoomAndPan",
    ]
    .into_iter()
    .find(|canonical| name.eq_ignore_ascii_case(canonical))
}

fn validate_direct_text(
    parent: Option<ParentContext<'_>>,
    value: &str,
    direct: &DirectChildState,
) -> Result<(), RenderError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    if parent.namespace != ElementNamespace::Html {
        return Ok(());
    }
    let parent_name = parent.tag.as_str();
    if !direct.has_serialized_content
        && value.starts_with('\n')
        && ["pre", "textarea"]
            .iter()
            .any(|tag| parent_name.eq_ignore_ascii_case(tag))
    {
        return Err(RenderError::ParserRepair {
            parent: parent_name.to_owned(),
            child: None,
            rule: "the HTML parser strips the first line feed",
        });
    }
    if is_table_container(parent_name) && !value.chars().all(char::is_whitespace) {
        return Err(RenderError::ParserRepair {
            parent: parent_name.to_owned(),
            child: None,
            rule: "non-whitespace table text is foster-parented",
        });
    }
    Ok(())
}

fn is_table_container(tag: &str) -> bool {
    ["table", "thead", "tbody", "tfoot", "tr", "colgroup"]
        .iter()
        .any(|candidate| tag.eq_ignore_ascii_case(candidate))
}

fn is_svg_html_breakout(tag: &str) -> bool {
    [
        "b",
        "big",
        "blockquote",
        "body",
        "br",
        "center",
        "code",
        "dd",
        "div",
        "dl",
        "dt",
        "em",
        "embed",
        "font",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "hr",
        "i",
        "img",
        "li",
        "listing",
        "menu",
        "meta",
        "nobr",
        "ol",
        "p",
        "pre",
        "ruby",
        "s",
        "small",
        "span",
        "strike",
        "strong",
        "sub",
        "sup",
        "table",
        "tt",
        "u",
        "ul",
        "var",
    ]
    .iter()
    .any(|candidate| tag.eq_ignore_ascii_case(candidate))
}

fn is_p_closing_block(tag: &str) -> bool {
    [
        "address",
        "article",
        "aside",
        "blockquote",
        "details",
        "dialog",
        "div",
        "dd",
        "dt",
        "center",
        "dir",
        "dl",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "header",
        "hgroup",
        "hr",
        "main",
        "li",
        "listing",
        "menu",
        "nav",
        "ol",
        "p",
        "pre",
        "search",
        "section",
        "summary",
        "table",
        "ul",
    ]
    .iter()
    .any(|candidate| tag.eq_ignore_ascii_case(candidate))
}

fn is_standard_scope_boundary(tag: &str) -> bool {
    [
        "applet", "caption", "html", "marquee", "object", "table", "td", "template", "th",
    ]
    .iter()
    .any(|candidate| tag.eq_ignore_ascii_case(candidate))
}

fn is_list_scan_barrier(tag: &str) -> bool {
    is_html_special_element(tag)
        && !["address", "div", "p"]
            .iter()
            .any(|candidate| tag.eq_ignore_ascii_case(candidate))
}

fn is_html_special_element(tag: &str) -> bool {
    [
        "address",
        "applet",
        "area",
        "article",
        "aside",
        "base",
        "basefont",
        "bgsound",
        "blockquote",
        "body",
        "br",
        "button",
        "caption",
        "center",
        "col",
        "colgroup",
        "dd",
        "details",
        "dir",
        "div",
        "dl",
        "dt",
        "embed",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "frame",
        "frameset",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hgroup",
        "hr",
        "html",
        "iframe",
        "img",
        "input",
        "keygen",
        "li",
        "link",
        "listing",
        "main",
        "marquee",
        "menu",
        "meta",
        "nav",
        "noembed",
        "noframes",
        "noscript",
        "object",
        "ol",
        "p",
        "param",
        "plaintext",
        "pre",
        "script",
        "search",
        "section",
        "select",
        "source",
        "style",
        "summary",
        "table",
        "tbody",
        "td",
        "template",
        "textarea",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "track",
        "ul",
        "wbr",
        "xmp",
    ]
    .iter()
    .any(|candidate| tag.eq_ignore_ascii_case(candidate))
}

// ───────────────────────── tests (the M4 gate, native) ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_reactive::Signal;

    /// THE M4 GATE 1: the builder renders correct HTML (the SSR seed).
    #[test]
    fn gate_builder_renders_html() {
        let v = el("section")
            .class("card")
            .id("hero")
            .child(el("h1").child("Pliego"))
            .child(el("p").child(text("folds & logs")))
            .child(el("br"))
            .into_view();
        assert_eq!(
            render_html(&v),
            r#"<section class="card" id="hero"><h1>Pliego</h1><p>folds &amp; logs</p><br></section>"#
        );
    }

    /// THE M4 GATE 2: dynamic parts fold CURRENT reactive state into markup —
    /// the same tree renders differently as the state advances (SSR of a live
    /// fold, deterministic).
    #[test]
    fn gate_dynamic_html_tracks_state() {
        let count = Signal::new(1);
        let v = el("div")
            .attr_dyn("data-count", move || count.get().to_string())
            .child(dyn_text(move || format!("count is {}", count.get())))
            .into_view();
        assert_eq!(render_html(&v), r#"<div data-count="1">count is 1</div>"#);
        count.set(7);
        assert_eq!(render_html(&v), r#"<div data-count="7">count is 7</div>"#);
    }

    /// THE M4 GATE 3: `show` composes over the same reactive state.
    #[test]
    fn gate_show_branches() {
        let logged_in = Signal::new(false);
        let v = show(
            move || logged_in.get(),
            || el("main").child("welcome back").into_view(),
            || el("a").attr("href", "/login").child("log in").into_view(),
        );
        assert_eq!(render_html(&v), r#"<a href="/login">log in</a>"#);
        logged_in.set(true);
        assert_eq!(render_html(&v), "<main>welcome back</main>");
    }

    /// Escaping: content and attributes are safe by default.
    #[test]
    fn escaping_is_on_by_default() {
        let v = el("p")
            .attr("title", r#"a "quote" & <tag>"#)
            .child(text("<script>alert(1)</script>"))
            .into_view();
        assert_eq!(
            render_html(&v),
            r#"<p title="a &quot;quote&quot; &amp; &lt;tag&gt;">&lt;script&gt;alert(1)&lt;/script&gt;</p>"#
        );
    }

    #[test]
    fn fallible_builders_reject_name_injection_and_inline_handlers() {
        assert!(matches!(
            try_el("div><script>alert(1)</script>"),
            Err(DomError::InvalidName(NameError {
                kind: NameKind::Tag,
                ..
            }))
        ));

        let inline = el("button").try_attr("OnClIcK", "alert(1)");
        assert!(matches!(
            inline,
            Err(DomError::InlineEventAttribute { name }) if name == "OnClIcK"
        ));

        let event = el("button").try_on("click onclick=alert(1)", |_| {});
        assert!(matches!(
            event,
            Err(DomError::InvalidName(NameError {
                kind: NameKind::Event,
                ..
            }))
        ));
    }

    #[test]
    fn attributes_are_unique_across_static_dynamic_and_ascii_case() {
        let duplicate = el("svg")
            .attr("viewBox", "0 0 10 10")
            .try_attr_dyn("viewbox", || "0 0 20 20".to_owned());
        assert!(matches!(
            duplicate,
            Err(DomError::DuplicateAttribute { name }) if name == "viewbox"
        ));
    }

    #[test]
    fn html_void_elements_reject_children_and_render_without_end_tags() {
        let invalid = el("img").child("not allowed").into_view();
        assert!(matches!(
            try_render_html(&invalid, RenderLimits::default()),
            Err(RenderError::VoidElementChild { tag }) if tag == "img"
        ));
        assert_eq!(render_html(&el("BR").into_view()), "<BR>");

        let svg = el("svg")
            .child(el("source").child(el("circle")))
            .into_view();
        assert_eq!(
            render_html(&svg),
            "<svg><source><circle></circle></source></svg>"
        );
    }

    #[test]
    fn active_elements_srcdoc_and_unsafe_url_schemes_fail_closed() {
        for tag in [
            "script",
            "STYLE",
            "iframe",
            "fencedframe",
            "portal",
            "object",
            "embed",
            "applet",
            "basefont",
            "bgsound",
            "keygen",
            "animate",
            "animateColor",
            "SET",
            "animateMotion",
            "animateTransform",
            "discard",
        ] {
            assert!(matches!(
                try_el(tag),
                Err(DomError::ForbiddenElement { tag: rejected }) if rejected.eq_ignore_ascii_case(tag)
            ));
        }
        assert!(matches!(
            el("div").try_attr("srcdoc", "<img onerror=alert(1)>") ,
            Err(DomError::ForbiddenAttribute { name }) if name == "srcdoc"
        ));

        for value in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "java\tscript:alert(1)",
            "v b s c r i p t:msgbox(1)",
            "data:text/html,<script>alert(1)</script>",
            "blob:https://example.test/id",
            "ftp://example.test/file",
        ] {
            assert!(
                matches!(
                    el("a").try_attr("href", value),
                    Err(DomError::InvalidAttributeValue {
                        violation: AttributeValueViolation::DisallowedUrlScheme { .. },
                        ..
                    })
                ),
                "accepted URL {value:?}"
            );
        }

        for value in [
            "https://example.test/path",
            "http://example.test",
            "mailto:hello@pliegors.dev",
            "tel:+15551234567",
            "/relative/path",
            "../relative/path",
            "#fragment",
            "//cdn.example.test/file.js",
        ] {
            assert!(
                el("a").try_attr("href", value).is_ok(),
                "rejected URL {value:?}"
            );
        }

        assert!(matches!(
            el("img").try_attr("srcset", "safe.png 1x, javascript:alert(1) 2x"),
            Err(DomError::InvalidAttributeValue {
                violation: AttributeValueViolation::DisallowedUrlScheme { .. },
                ..
            })
        ));
    }

    #[test]
    fn dynamic_url_validation_returns_a_render_error() {
        let view = el("a")
            .attr_dyn("href", || "java\nscript:alert(1)".to_owned())
            .child("unsafe")
            .into_view();
        assert!(matches!(
            try_render_html(&view, RenderLimits::default()),
            Err(RenderError::InvalidAttribute(
                DomError::InvalidAttributeValue {
                    violation: AttributeValueViolation::DisallowedUrlScheme { .. },
                    ..
                }
            ))
        ));
    }

    #[test]
    fn render_limits_bound_depth_nodes_and_output() {
        let depth = View::Fragment(vec![View::Fragment(vec![text("deep")])]);
        let depth_limits = RenderLimits::new(2, 10, 100).unwrap();
        assert!(matches!(
            try_render_html(&depth, depth_limits),
            Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::Depth,
                limit: 2
            })
        ));

        let nodes = View::Fragment(vec![text("a"), text("b")]);
        let node_limits = RenderLimits::new(10, 2, 100).unwrap();
        assert!(matches!(
            try_render_html(&nodes, node_limits),
            Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::Nodes,
                limit: 2
            })
        ));

        let output_limits = RenderLimits::new(10, 10, 3).unwrap();
        assert!(matches!(
            try_render_html(&text("<&"), output_limits),
            Err(RenderError::LimitExceeded {
                kind: RenderLimitKind::OutputBytes,
                limit: 3
            })
        ));
        assert!(RenderLimits::new(HARD_RENDER_MAX_DEPTH + 1, 1, 1).is_err());
    }

    #[test]
    fn parser_normalized_characters_are_rejected() {
        assert!(matches!(
            try_render_html(&text("left\rright"), RenderLimits::default()),
            Err(RenderError::ParserNormalizedText {
                character: '\r',
                ..
            })
        ));
        assert!(matches!(
            try_render_html(&text("left\0right"), RenderLimits::default()),
            Err(RenderError::ParserNormalizedText {
                character: '\0',
                ..
            })
        ));
        assert!(matches!(
            el("div").try_attr("title", "left\rright"),
            Err(DomError::InvalidAttributeValue {
                violation: AttributeValueViolation::ParserNormalizedCharacter {
                    character: '\r',
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn parser_repair_prone_structures_are_rejected() {
        let paragraph = el("p").child(el("div").child("block")).into_view();
        assert!(matches!(
            try_render_html(&paragraph, RenderLimits::default()),
            Err(RenderError::ParserRepair { ref parent, .. }) if parent == "p"
        ));

        let nested_paragraph = el("p")
            .child(el("span").child(el("div").child("block")))
            .into_view();
        assert!(matches!(
            try_render_html(&nested_paragraph, RenderLimits::default()),
            Err(RenderError::ParserRepair { ref parent, .. }) if parent == "p"
        ));

        for (outer, inner) in [
            ("button", "button"),
            ("a", "a"),
            ("form", "form"),
            ("li", "li"),
            ("dt", "dd"),
            ("nobr", "nobr"),
        ] {
            let view = el(outer)
                .child(el("span").child(el(inner).child("nested")))
                .into_view();
            assert!(
                matches!(
                    try_render_html(&view, RenderLimits::default()),
                    Err(RenderError::ParserRepair { .. })
                ),
                "accepted parser-repaired {outer} > span > {inner}"
            );
        }

        let nested_heading = el("h1").child(el("h2").child("heading")).into_view();
        assert!(matches!(
            try_render_html(&nested_heading, RenderLimits::default()),
            Err(RenderError::ParserRepair { .. })
        ));

        for parent in ["rb", "rt", "rp", "rtc"] {
            for child in ["rb", "rt", "rp", "rtc"] {
                let view = el("ruby")
                    .child(el(parent).child(el(child).child("nested")))
                    .into_view();
                let remains_nested = parent.eq_ignore_ascii_case("rtc")
                    && ["rt", "rp"]
                        .iter()
                        .any(|tag| child.eq_ignore_ascii_case(tag));
                assert_eq!(
                    try_render_html(&view, RenderLimits::default()).is_ok(),
                    remains_nested,
                    "wrong direct Ruby result for {parent} > {child}"
                );

                let transparent = el("ruby")
                    .child(el(parent).child(el("span").child(el(child).child("nested"))))
                    .into_view();
                assert!(
                    try_render_html(&transparent, RenderLimits::default()).is_ok(),
                    "rejected parser-stable Ruby descendant {parent} > span > {child}"
                );
            }
        }

        let valid_ruby = el("ruby")
            .child(el("rb").child("base"))
            .child(
                el("rtc")
                    .child(el("rt").child("reading"))
                    .child(el("rp").child("(")),
            )
            .into_view();
        assert!(try_render_html(&valid_ruby, RenderLimits::default()).is_ok());

        // A button is a button-scope boundary for an outer paragraph, and a
        // table cell is a standard scope boundary for an outer button.
        let scoped_paragraph = el("p")
            .child(el("button").child(el("div").child("stable")))
            .into_view();
        assert!(try_render_html(&scoped_paragraph, RenderLimits::default()).is_ok());
        let scoped_button = el("button")
            .child(el("table").child(
                el("tbody").child(el("tr").child(el("td").child(el("button").child("stable")))),
            ))
            .into_view();
        assert!(try_render_html(&scoped_button, RenderLimits::default()).is_ok());

        for view in [
            el("div").child(el("tr")).into_view(),
            el("table").child(el("td")).into_view(),
            el("select").child(el("div").child("ignored")).into_view(),
            el("select")
                .child(el("option").child(el("span").child("flattened")))
                .into_view(),
        ] {
            assert!(matches!(
                try_render_html(&view, RenderLimits::default()),
                Err(RenderError::ParserRepair { .. })
            ));
        }

        let direct_row = el("table")
            .child(el("tr").child(el("td").child("cell")))
            .into_view();
        assert!(matches!(
            try_render_html(&direct_row, RenderLimits::default()),
            Err(RenderError::ParserRepair { ref parent, .. }) if parent == "table"
        ));

        let table_text = el("table").child("foster me").into_view();
        assert!(matches!(
            try_render_html(&table_text, RenderLimits::default()),
            Err(RenderError::ParserRepair { ref parent, .. }) if parent == "table"
        ));

        for tag in ["pre", "textarea"] {
            let view = el(tag).child("\nfirst line").into_view();
            assert!(matches!(
                try_render_html(&view, RenderLimits::default()),
                Err(RenderError::ParserRepair { ref parent, .. }) if parent == tag
            ));
        }
        let textarea_element = el("textarea").child(el("b").child("bold")).into_view();
        assert!(matches!(
            try_render_html(&textarea_element, RenderLimits::default()),
            Err(RenderError::ParserRepair { ref parent, .. }) if parent == "textarea"
        ));

        for breakout in ["div", "BR", "span", "font"] {
            let view = el("svg").child(el(breakout)).into_view();
            assert!(matches!(
                try_render_html(&view, RenderLimits::default()),
                Err(RenderError::ParserRepair { ref parent, ref child, .. })
                    if parent == "svg" && child.as_deref() == Some(breakout)
            ));
        }

        for integration_point in ["foreignObject", "desc", "title"] {
            let view = el("svg")
                .child(el(integration_point).child(el("div").child("HTML")))
                .into_view();
            assert!(try_render_html(&view, RenderLimits::default()).is_ok());
        }

        for adjusted in [
            "lineargradient",
            "foreignobject",
            "FEGAUSSIANBLUR",
            "myShape",
        ] {
            let view = el("svg").child(el(adjusted)).into_view();
            assert!(matches!(
                try_render_html(&view, RenderLimits::default()),
                Err(RenderError::ParserRepair { ref parent, ref child, .. })
                    if parent == "svg" && child.as_deref() == Some(adjusted)
            ));
        }

        for adjusted in ["viewbox", "DATA-route", "xlink:custom", "XMLNS"] {
            let view = el("svg").attr(adjusted, "value").into_view();
            assert!(matches!(
                try_render_html(&view, RenderLimits::default()),
                Err(RenderError::ParserRepair { ref parent, ref child, .. })
                    if parent == "svg" && child.as_deref() == Some(&format!("@{adjusted}"))
            ));
        }
    }

    #[test]
    fn explicit_table_sections_serialize_without_parser_repairs() {
        let table = el("table")
            .child(
                el("tbody").child(
                    el("tr")
                        .child(el("th").child("Name"))
                        .child(el("td").child("Pliego")),
                ),
            )
            .into_view();
        assert_eq!(
            try_render_html(&table, RenderLimits::default()).unwrap(),
            "<table><tbody><tr><th>Name</th><td>Pliego</td></tr></tbody></table>"
        );
    }

    #[test]
    fn valid_svg_custom_data_and_aria_names_preserve_their_spelling() {
        let v = el("pliego-card")
            .attr("data-route-id", "intro")
            .attr("aria-labelledby", "title")
            .child(
                el("svg")
                    .attr("xmlns", ElementNamespace::SVG_URI)
                    .attr("viewBox", "0 0 10 10")
                    .attr("xlink:href", "#shape")
                    .child(el("linearGradient")),
            )
            .into_view();
        assert_eq!(
            render_html(&v),
            r##"<pliego-card data-route-id="intro" aria-labelledby="title"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" xlink:href="#shape"><linearGradient></linearGradient></svg></pliego-card>"##
        );
    }

    /// SSR renders untracked: rendering must not subscribe (no stray edges).
    #[test]
    fn render_html_does_not_subscribe() {
        use pliego_reactive::{Effect, Memo};
        use std::cell::Cell;
        use std::rc::Rc;

        let s = Signal::new(0);
        let v = el("span")
            .child(dyn_text(move || s.get().to_string()))
            .into_view();

        // render inside a memo: if render_html tracked, the memo would depend on s
        let renders = Rc::new(Cell::new(0));
        let m = {
            let renders = renders.clone();
            let v = v.clone();
            Memo::new(move || {
                renders.set(renders.get() + 1);
                render_html(&v)
            })
        };
        Effect::new(move || {
            let _ = m.get();
        });
        assert_eq!(renders.get(), 1);
        s.set(99);
        assert_eq!(
            renders.get(),
            1,
            "render_html must not create reactive edges"
        );
    }
}
