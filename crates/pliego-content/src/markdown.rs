use crate::SourceSpan;
use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Parser, Tag, TagEnd, html,
};
use std::fmt;
use std::ops::Range;

/// Security policy applied when accepting or rendering Markdown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkdownPolicy {
    /// Reject raw HTML and URI schemes other than HTTP(S), mail, and telephone links.
    #[default]
    Safe,
    /// Preserve CommonMark raw HTML and arbitrary URI schemes.
    ///
    /// This is an explicit author-trust decision; it does not create a trusted DOM type.
    Trusted,
}

/// Link syntax represented without exposing the Markdown parser's types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MarkdownLinkKind {
    Inline,
    Reference,
    ReferenceUnknown,
    Collapsed,
    CollapsedUnknown,
    Shortcut,
    ShortcutUnknown,
    Autolink,
    Email,
}

/// CommonMark code block form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkdownCodeBlock {
    Indented,
    Fenced(String),
}

/// An owned CommonMark container start.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MarkdownTag {
    Paragraph,
    Heading {
        level: u8,
    },
    BlockQuote,
    CodeBlock(MarkdownCodeBlock),
    HtmlBlock,
    List {
        start: Option<u64>,
    },
    Item,
    Emphasis,
    Strong,
    Link {
        kind: MarkdownLinkKind,
        destination: String,
        title: String,
        id: String,
    },
    Image {
        kind: MarkdownLinkKind,
        destination: String,
        title: String,
        id: String,
    },
}

/// An owned CommonMark container end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MarkdownTagEnd {
    Paragraph,
    Heading { level: u8 },
    BlockQuote,
    CodeBlock,
    HtmlBlock,
    List { ordered: bool },
    Item,
    Emphasis,
    Strong,
    Link,
    Image,
}

/// Framework-neutral, owned event in a CommonMark preorder traversal.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MarkdownEvent {
    Start(MarkdownTag),
    End(MarkdownTagEnd),
    Text(String),
    Code(String),
    RawHtml { block: bool, value: String },
    SoftBreak,
    HardBreak,
    Rule,
}

/// A neutral event and its byte position in the Markdown body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpannedMarkdownEvent {
    event: MarkdownEvent,
    span: SourceSpan,
}

impl SpannedMarkdownEvent {
    pub fn event(&self) -> &MarkdownEvent {
        &self.event
    }

    pub const fn span(&self) -> SourceSpan {
        self.span
    }
}

/// Parser-independent CommonMark event stream.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MarkdownAst {
    events: Vec<SpannedMarkdownEvent>,
}

impl MarkdownAst {
    pub fn events(&self) -> &[SpannedMarkdownEvent] {
        &self.events
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &SpannedMarkdownEvent> {
        self.events.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Markdown source plus its authoritative, owned CommonMark event stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownDocument {
    source: String,
    ast: MarkdownAst,
}

impl MarkdownDocument {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub const fn ast(&self) -> &MarkdownAst {
        &self.ast
    }

    /// Render the owned event stream to HTML under an explicit policy.
    pub fn render_html(&self, policy: MarkdownPolicy) -> Result<String, MarkdownRenderError> {
        validate_ast(&self.ast, policy)?;
        let mut output = String::new();
        html::push_html(
            &mut output,
            self.ast
                .events
                .iter()
                .map(|event| to_parser_event(&event.event)),
        );
        Ok(output)
    }
}

/// Why Markdown was rejected by the safe renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MarkdownSecurityIssue {
    RawHtml,
    DisallowedUri,
}

/// Policy failure with an exact span in the Markdown body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownRenderError {
    issue: MarkdownSecurityIssue,
    span: SourceSpan,
    message: String,
}

impl MarkdownRenderError {
    pub const fn issue(&self) -> MarkdownSecurityIssue {
        self.issue
    }

    pub const fn span(&self) -> SourceSpan {
        self.span
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for MarkdownRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Markdown at {}:{}: {}",
            self.span.line, self.span.column, self.message
        )
    }
}

impl std::error::Error for MarkdownRenderError {}

pub(crate) fn parse_markdown(
    source: String,
    policy: MarkdownPolicy,
) -> Result<MarkdownDocument, MarkdownRenderError> {
    let events = Parser::new(&source)
        .into_offset_iter()
        .map(|(event, range)| SpannedMarkdownEvent {
            event: from_parser_event(event),
            span: span_for_range(&source, range),
        })
        .collect();
    let document = MarkdownDocument {
        source,
        ast: MarkdownAst { events },
    };
    validate_ast(&document.ast, policy)?;
    Ok(document)
}

fn validate_ast(ast: &MarkdownAst, policy: MarkdownPolicy) -> Result<(), MarkdownRenderError> {
    if policy == MarkdownPolicy::Trusted {
        return Ok(());
    }
    for event in &ast.events {
        match &event.event {
            MarkdownEvent::RawHtml { .. } | MarkdownEvent::Start(MarkdownTag::HtmlBlock) => {
                return Err(MarkdownRenderError {
                    issue: MarkdownSecurityIssue::RawHtml,
                    span: event.span,
                    message: "raw HTML requires MarkdownPolicy::Trusted".to_owned(),
                });
            }
            MarkdownEvent::Start(MarkdownTag::Link { destination, .. })
            | MarkdownEvent::Start(MarkdownTag::Image { destination, .. })
                if !safe_destination(destination) =>
            {
                return Err(MarkdownRenderError {
                    issue: MarkdownSecurityIssue::DisallowedUri,
                    span: event.span,
                    message: format!(
                        "URI scheme in {destination:?} requires MarkdownPolicy::Trusted"
                    ),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn safe_destination(destination: &str) -> bool {
    let trimmed = destination.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('?')
    {
        return true;
    }

    let boundary = trimmed.find(['/', '?', '#']).unwrap_or(trimmed.len());
    let Some(colon) = trimmed[..boundary].find(':') else {
        return true;
    };
    let scheme: String = trimmed[..colon]
        .chars()
        .filter(|character| !character.is_ascii_control() && !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel")
}

fn span_for_range(source: &str, range: Range<usize>) -> SourceSpan {
    SourceSpan::from_range(source, range.start, range.end)
}

fn from_parser_event(event: Event<'_>) -> MarkdownEvent {
    match event {
        Event::Start(tag) => MarkdownEvent::Start(from_parser_tag(tag)),
        Event::End(tag) => MarkdownEvent::End(from_parser_end(tag)),
        Event::Text(value) => MarkdownEvent::Text(value.into_string()),
        Event::Code(value) => MarkdownEvent::Code(value.into_string()),
        Event::Html(value) => MarkdownEvent::RawHtml {
            block: true,
            value: value.into_string(),
        },
        Event::InlineHtml(value) => MarkdownEvent::RawHtml {
            block: false,
            value: value.into_string(),
        },
        Event::SoftBreak => MarkdownEvent::SoftBreak,
        Event::HardBreak => MarkdownEvent::HardBreak,
        Event::Rule => MarkdownEvent::Rule,
        Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::FootnoteReference(_)
        | Event::TaskListMarker(_) => {
            unreachable!("CommonMark extensions are disabled for Pliego content")
        }
    }
}

fn from_parser_tag(tag: Tag<'_>) -> MarkdownTag {
    match tag {
        Tag::Paragraph => MarkdownTag::Paragraph,
        Tag::Heading { level, .. } => MarkdownTag::Heading {
            level: heading_to_u8(level),
        },
        Tag::BlockQuote(None) => MarkdownTag::BlockQuote,
        Tag::CodeBlock(kind) => MarkdownTag::CodeBlock(match kind {
            CodeBlockKind::Indented => MarkdownCodeBlock::Indented,
            CodeBlockKind::Fenced(info) => MarkdownCodeBlock::Fenced(info.into_string()),
        }),
        Tag::HtmlBlock => MarkdownTag::HtmlBlock,
        Tag::List(start) => MarkdownTag::List { start },
        Tag::Item => MarkdownTag::Item,
        Tag::Emphasis => MarkdownTag::Emphasis,
        Tag::Strong => MarkdownTag::Strong,
        Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        } => MarkdownTag::Link {
            kind: from_link_type(link_type),
            destination: dest_url.into_string(),
            title: title.into_string(),
            id: id.into_string(),
        },
        Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        } => MarkdownTag::Image {
            kind: from_link_type(link_type),
            destination: dest_url.into_string(),
            title: title.into_string(),
            id: id.into_string(),
        },
        Tag::BlockQuote(Some(_))
        | Tag::FootnoteDefinition(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::Table(_)
        | Tag::TableHead
        | Tag::TableRow
        | Tag::TableCell
        | Tag::Strikethrough
        | Tag::Superscript
        | Tag::Subscript
        | Tag::MetadataBlock(_) => {
            unreachable!("CommonMark extensions are disabled for Pliego content")
        }
    }
}

fn from_parser_end(tag: TagEnd) -> MarkdownTagEnd {
    match tag {
        TagEnd::Paragraph => MarkdownTagEnd::Paragraph,
        TagEnd::Heading(level) => MarkdownTagEnd::Heading {
            level: heading_to_u8(level),
        },
        TagEnd::BlockQuote(None) => MarkdownTagEnd::BlockQuote,
        TagEnd::CodeBlock => MarkdownTagEnd::CodeBlock,
        TagEnd::HtmlBlock => MarkdownTagEnd::HtmlBlock,
        TagEnd::List(ordered) => MarkdownTagEnd::List { ordered },
        TagEnd::Item => MarkdownTagEnd::Item,
        TagEnd::Emphasis => MarkdownTagEnd::Emphasis,
        TagEnd::Strong => MarkdownTagEnd::Strong,
        TagEnd::Link => MarkdownTagEnd::Link,
        TagEnd::Image => MarkdownTagEnd::Image,
        TagEnd::BlockQuote(Some(_))
        | TagEnd::FootnoteDefinition
        | TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition
        | TagEnd::Table
        | TagEnd::TableHead
        | TagEnd::TableRow
        | TagEnd::TableCell
        | TagEnd::Strikethrough
        | TagEnd::Superscript
        | TagEnd::Subscript
        | TagEnd::MetadataBlock(_) => {
            unreachable!("CommonMark extensions are disabled for Pliego content")
        }
    }
}

fn to_parser_event(event: &MarkdownEvent) -> Event<'static> {
    match event {
        MarkdownEvent::Start(tag) => Event::Start(to_parser_tag(tag)),
        MarkdownEvent::End(tag) => Event::End(to_parser_end(*tag)),
        MarkdownEvent::Text(value) => Event::Text(owned(value)),
        MarkdownEvent::Code(value) => Event::Code(owned(value)),
        MarkdownEvent::RawHtml { block: true, value } => Event::Html(owned(value)),
        MarkdownEvent::RawHtml {
            block: false,
            value,
        } => Event::InlineHtml(owned(value)),
        MarkdownEvent::SoftBreak => Event::SoftBreak,
        MarkdownEvent::HardBreak => Event::HardBreak,
        MarkdownEvent::Rule => Event::Rule,
    }
}

fn to_parser_tag(tag: &MarkdownTag) -> Tag<'static> {
    match tag {
        MarkdownTag::Paragraph => Tag::Paragraph,
        MarkdownTag::Heading { level } => Tag::Heading {
            level: u8_to_heading(*level),
            id: None,
            classes: Vec::new(),
            attrs: Vec::new(),
        },
        MarkdownTag::BlockQuote => Tag::BlockQuote(None),
        MarkdownTag::CodeBlock(MarkdownCodeBlock::Indented) => {
            Tag::CodeBlock(CodeBlockKind::Indented)
        }
        MarkdownTag::CodeBlock(MarkdownCodeBlock::Fenced(info)) => {
            Tag::CodeBlock(CodeBlockKind::Fenced(owned(info)))
        }
        MarkdownTag::HtmlBlock => Tag::HtmlBlock,
        MarkdownTag::List { start } => Tag::List(*start),
        MarkdownTag::Item => Tag::Item,
        MarkdownTag::Emphasis => Tag::Emphasis,
        MarkdownTag::Strong => Tag::Strong,
        MarkdownTag::Link {
            kind,
            destination,
            title,
            id,
        } => Tag::Link {
            link_type: to_link_type(*kind),
            dest_url: owned(destination),
            title: owned(title),
            id: owned(id),
        },
        MarkdownTag::Image {
            kind,
            destination,
            title,
            id,
        } => Tag::Image {
            link_type: to_link_type(*kind),
            dest_url: owned(destination),
            title: owned(title),
            id: owned(id),
        },
    }
}

fn to_parser_end(tag: MarkdownTagEnd) -> TagEnd {
    match tag {
        MarkdownTagEnd::Paragraph => TagEnd::Paragraph,
        MarkdownTagEnd::Heading { level } => TagEnd::Heading(u8_to_heading(level)),
        MarkdownTagEnd::BlockQuote => TagEnd::BlockQuote(None),
        MarkdownTagEnd::CodeBlock => TagEnd::CodeBlock,
        MarkdownTagEnd::HtmlBlock => TagEnd::HtmlBlock,
        MarkdownTagEnd::List { ordered } => TagEnd::List(ordered),
        MarkdownTagEnd::Item => TagEnd::Item,
        MarkdownTagEnd::Emphasis => TagEnd::Emphasis,
        MarkdownTagEnd::Strong => TagEnd::Strong,
        MarkdownTagEnd::Link => TagEnd::Link,
        MarkdownTagEnd::Image => TagEnd::Image,
    }
}

fn from_link_type(kind: LinkType) -> MarkdownLinkKind {
    match kind {
        LinkType::Inline => MarkdownLinkKind::Inline,
        LinkType::Reference => MarkdownLinkKind::Reference,
        LinkType::ReferenceUnknown => MarkdownLinkKind::ReferenceUnknown,
        LinkType::Collapsed => MarkdownLinkKind::Collapsed,
        LinkType::CollapsedUnknown => MarkdownLinkKind::CollapsedUnknown,
        LinkType::Shortcut => MarkdownLinkKind::Shortcut,
        LinkType::ShortcutUnknown => MarkdownLinkKind::ShortcutUnknown,
        LinkType::Autolink => MarkdownLinkKind::Autolink,
        LinkType::Email => MarkdownLinkKind::Email,
        LinkType::WikiLink { .. } => {
            unreachable!("wiki links are disabled for Pliego content")
        }
    }
}

fn to_link_type(kind: MarkdownLinkKind) -> LinkType {
    match kind {
        MarkdownLinkKind::Inline => LinkType::Inline,
        MarkdownLinkKind::Reference => LinkType::Reference,
        MarkdownLinkKind::ReferenceUnknown => LinkType::ReferenceUnknown,
        MarkdownLinkKind::Collapsed => LinkType::Collapsed,
        MarkdownLinkKind::CollapsedUnknown => LinkType::CollapsedUnknown,
        MarkdownLinkKind::Shortcut => LinkType::Shortcut,
        MarkdownLinkKind::ShortcutUnknown => LinkType::ShortcutUnknown,
        MarkdownLinkKind::Autolink => LinkType::Autolink,
        MarkdownLinkKind::Email => LinkType::Email,
    }
}

fn heading_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn u8_to_heading(level: u8) -> HeadingLevel {
    match level {
        1 => HeadingLevel::H1,
        2 => HeadingLevel::H2,
        3 => HeadingLevel::H3,
        4 => HeadingLevel::H4,
        5 => HeadingLevel::H5,
        6 => HeadingLevel::H6,
        _ => unreachable!("Pliego Markdown heading levels come from CommonMark"),
    }
}

fn owned(value: &str) -> CowStr<'static> {
    CowStr::Boxed(value.to_owned().into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_destinations_are_an_allowlist() {
        for allowed in [
            "post/one",
            "../post",
            "/post",
            "#part",
            "?page=2",
            "https://example.com",
            "HTTP://example.com",
            "mailto:hello@example.com",
            "tel:+12025550123",
        ] {
            assert!(safe_destination(allowed), "{allowed}");
        }
        for rejected in [
            "javascript:alert(1)",
            "java\nscript:alert(1)",
            "data:text/html,bad",
            "file:///secret",
            "custom:action",
        ] {
            assert!(!safe_destination(rejected), "{rejected}");
        }
    }

    #[test]
    fn neutral_events_round_trip_to_commonmark_html() {
        let document = parse_markdown(
            "# Hello\n\nA **bold** move.\n".to_owned(),
            MarkdownPolicy::Safe,
        )
        .expect("safe Markdown");
        assert_eq!(
            document.render_html(MarkdownPolicy::Safe).unwrap(),
            "<h1>Hello</h1>\n<p>A <strong>bold</strong> move.</p>\n"
        );
        assert!(!document.ast().is_empty());
    }
}
