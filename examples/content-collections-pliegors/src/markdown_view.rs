use pliego_content::{MarkdownDocument, MarkdownEvent, MarkdownTag, MarkdownTagEnd};
use pliego_dom::{IntoView, View, el, text};

enum Frame {
    Element {
        tag: &'static str,
        end: MarkdownTagEnd,
        attributes: Vec<(&'static str, String)>,
        children: Vec<View>,
    },
    Image {
        end: MarkdownTagEnd,
        source: String,
        title: String,
        alt: String,
    },
}

impl Frame {
    fn end(&self) -> MarkdownTagEnd {
        match self {
            Self::Element { end, .. } | Self::Image { end, .. } => *end,
        }
    }

    fn push(&mut self, view: View) -> Result<(), String> {
        match self {
            Self::Element { children, .. } => {
                children.push(view);
                Ok(())
            }
            Self::Image { .. } => Err("image alternative text cannot contain a block view".into()),
        }
    }

    fn push_text(&mut self, value: &str) {
        match self {
            Self::Element { children, .. } => children.push(text(value)),
            Self::Image { alt, .. } => alt.push_str(value),
        }
    }

    fn finish(self) -> View {
        match self {
            Self::Element {
                tag,
                attributes,
                children,
                ..
            } => {
                let mut element = el(tag);
                for (name, value) in attributes {
                    element = element.attr(name, value);
                }
                for child in children {
                    element = element.child(child);
                }
                element.into_view()
            }
            Self::Image {
                source, title, alt, ..
            } => {
                let mut image = el("img")
                    .attr("src", source)
                    .attr("alt", alt)
                    .attr("loading", "lazy")
                    .attr("decoding", "async");
                if !title.is_empty() {
                    image = image.attr("title", title);
                }
                image.into_view()
            }
        }
    }
}

pub fn render(document: &MarkdownDocument) -> Result<View, String> {
    let mut stack = Vec::<Frame>::new();
    let mut roots = Vec::<View>::new();

    for spanned in document.ast().events() {
        match spanned.event() {
            MarkdownEvent::Start(tag) => stack.push(frame_for(tag)?),
            MarkdownEvent::End(end) => {
                let frame = stack
                    .pop()
                    .ok_or_else(|| format!("Markdown closed {end:?} without an open container"))?;
                if frame.end() != *end {
                    return Err(format!(
                        "Markdown closed {end:?} while {:?} was open",
                        frame.end()
                    ));
                }
                push_view(frame.finish(), &mut stack, &mut roots)?;
            }
            MarkdownEvent::Text(value) => push_text(value, &mut stack, &mut roots),
            MarkdownEvent::Code(value) => push_view(
                el("code").child(value.clone()).into_view(),
                &mut stack,
                &mut roots,
            )?,
            MarkdownEvent::RawHtml { .. } => {
                return Err("safe Markdown unexpectedly contained raw HTML".into());
            }
            MarkdownEvent::SoftBreak => push_text("\n", &mut stack, &mut roots),
            MarkdownEvent::HardBreak => push_view(el("br").into_view(), &mut stack, &mut roots)?,
            MarkdownEvent::Rule => push_view(el("hr").into_view(), &mut stack, &mut roots)?,
            _ => return Err("unsupported Markdown event".into()),
        }
    }

    if !stack.is_empty() {
        return Err("Markdown ended with an open container".into());
    }
    Ok(View::Fragment(roots))
}

fn frame_for(tag: &MarkdownTag) -> Result<Frame, String> {
    let element = |tag, end, attributes| Frame::Element {
        tag,
        end,
        attributes,
        children: Vec::new(),
    };
    Ok(match tag {
        MarkdownTag::Paragraph => element("p", MarkdownTagEnd::Paragraph, vec![]),
        MarkdownTag::Heading { level } => element(
            match level {
                1 => "h1",
                2 => "h2",
                3 => "h3",
                4 => "h4",
                5 => "h5",
                _ => "h6",
            },
            MarkdownTagEnd::Heading { level: *level },
            vec![],
        ),
        MarkdownTag::BlockQuote => element("blockquote", MarkdownTagEnd::BlockQuote, vec![]),
        MarkdownTag::CodeBlock(_) => element(
            "pre",
            MarkdownTagEnd::CodeBlock,
            vec![("class", "markdown-code".into())],
        ),
        MarkdownTag::HtmlBlock => return Err("safe Markdown cannot open an HTML block".into()),
        MarkdownTag::List { start } => {
            let attributes = start
                .map(|value| vec![("start", value.to_string())])
                .unwrap_or_default();
            element(
                if start.is_some() { "ol" } else { "ul" },
                MarkdownTagEnd::List {
                    ordered: start.is_some(),
                },
                attributes,
            )
        }
        MarkdownTag::Item => element("li", MarkdownTagEnd::Item, vec![]),
        MarkdownTag::Emphasis => element("em", MarkdownTagEnd::Emphasis, vec![]),
        MarkdownTag::Strong => element("strong", MarkdownTagEnd::Strong, vec![]),
        MarkdownTag::Link {
            destination, title, ..
        } => {
            let mut attributes = vec![("href", destination.clone())];
            if !title.is_empty() {
                attributes.push(("title", title.clone()));
            }
            element("a", MarkdownTagEnd::Link, attributes)
        }
        MarkdownTag::Image {
            destination, title, ..
        } => Frame::Image {
            end: MarkdownTagEnd::Image,
            source: destination.clone(),
            title: title.clone(),
            alt: String::new(),
        },
        _ => return Err("unsupported Markdown container".into()),
    })
}

fn push_view(view: View, stack: &mut [Frame], roots: &mut Vec<View>) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.push(view)
    } else {
        roots.push(view);
        Ok(())
    }
}

fn push_text(value: &str, stack: &mut [Frame], roots: &mut Vec<View>) {
    if let Some(parent) = stack.last_mut() {
        parent.push_text(value);
    } else {
        roots.push(text(value));
    }
}
