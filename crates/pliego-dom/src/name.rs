// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

use std::error::Error;
use std::fmt;

/// Maximum encoded size of a DOM name accepted by the renderer.
///
/// The bound keeps attacker-controlled builders from turning a name into an
/// unbounded allocation while remaining far above names used by HTML, SVG,
/// custom elements, `data-*`, and `aria-*` attributes.
pub const MAX_DOM_NAME_BYTES: usize = 256;
const NAME_ERROR_PREVIEW_BYTES: usize = 64;

/// The kind of DOM name that failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameKind {
    Tag,
    Attribute,
    Event,
}

impl fmt::Display for NameKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tag => "tag",
            Self::Attribute => "attribute",
            Self::Event => "event",
        })
    }
}

/// The precise reason a DOM name was rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NameViolation {
    Empty,
    TooLong { actual: usize, maximum: usize },
    InvalidStart { index: usize, character: char },
    InvalidCharacter { index: usize, character: char },
    InvalidNamespaceSeparator { index: usize },
    UnsupportedNamespacePrefix { prefix: String },
}

/// A structured DOM-name validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameError {
    pub kind: NameKind,
    pub value_preview: String,
    pub input_bytes: usize,
    pub preview_truncated: bool,
    pub violation: NameViolation,
}

impl NameError {
    fn rejected(kind: NameKind, value: &str, violation: NameViolation) -> Self {
        let mut preview_end = value.len().min(NAME_ERROR_PREVIEW_BYTES);
        while !value.is_char_boundary(preview_end) {
            preview_end -= 1;
        }
        Self {
            kind,
            value_preview: value[..preview_end].to_owned(),
            input_bytes: value.len(),
            preview_truncated: preview_end < value.len(),
            violation,
        }
    }
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {} name {:?}", self.kind, self.value_preview)?;
        if self.preview_truncated {
            write!(f, " (truncated from {} bytes)", self.input_bytes)?;
        }
        f.write_str(": ")?;
        match self.violation {
            NameViolation::Empty => f.write_str("the name is empty"),
            NameViolation::TooLong { actual, maximum } => {
                write!(f, "{actual} bytes exceeds the {maximum}-byte limit")
            }
            NameViolation::InvalidStart { index, character } => write!(
                f,
                "character {character:?} at byte {index} cannot start the name"
            ),
            NameViolation::InvalidCharacter { index, character } => {
                write!(f, "character {character:?} at byte {index} is not allowed")
            }
            NameViolation::InvalidNamespaceSeparator { index } => write!(
                f,
                "namespace separator at byte {index} does not separate two names"
            ),
            NameViolation::UnsupportedNamespacePrefix { ref prefix } => {
                write!(f, "namespace prefix {prefix:?} is not supported")
            }
        }
    }
}

impl Error for NameError {}

/// Namespace used when materializing validated elements in a browser DOM.
///
/// Namespace selection is structural: tag strings never carry a namespace
/// prefix. Attributes may use a single qualified separator (`xlink:href`,
/// `xml:space`, or `xmlns:xlink`). This makes both SSR and DOM construction use
/// the same validated spelling without accepting markup delimiters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ElementNamespace {
    #[default]
    Html,
    Svg,
}

impl ElementNamespace {
    pub const HTML_URI: &'static str = "http://www.w3.org/1999/xhtml";
    pub const SVG_URI: &'static str = "http://www.w3.org/2000/svg";

    #[must_use]
    pub const fn uri(self) -> &'static str {
        match self {
            Self::Html => Self::HTML_URI,
            Self::Svg => Self::SVG_URI,
        }
    }

    /// Namespace of `tag` when its parent uses `self`.
    #[must_use]
    pub fn for_element(self, tag: &TagName) -> Self {
        if self == Self::Html && tag.as_str().eq_ignore_ascii_case("svg") {
            Self::Svg
        } else {
            self
        }
    }

    /// Namespace inherited by the children of `tag`.
    ///
    /// `foreignObject`, `desc`, and `title` are the SVG HTML integration
    /// points defined by the HTML parser. Direct browser mounting must make
    /// the same transition so an SSR seed and its live DOM use equal nodes.
    #[must_use]
    pub fn for_children(self, tag: &TagName) -> Self {
        if self == Self::Svg
            && ["foreignObject", "desc", "title"]
                .iter()
                .any(|candidate| tag.as_str().eq_ignore_ascii_case(candidate))
        {
            Self::Html
        } else {
            self
        }
    }
}

macro_rules! validated_name {
    ($name:ident, $kind:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl AsRef<str>) -> Result<Self, NameError> {
                let value = value.as_ref();
                if let Err(violation) = validate(value, $kind) {
                    return Err(NameError::rejected($kind, value, violation));
                }
                Ok(Self(value.to_owned()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = NameError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                if let Err(violation) = validate(&value, $kind) {
                    return Err(NameError::rejected($kind, &value, violation));
                }
                Ok(Self(value))
            }
        }

        impl TryFrom<&str> for $name {
            type Error = NameError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

validated_name!(TagName, NameKind::Tag);
validated_name!(AttributeName, NameKind::Attribute);
validated_name!(EventName, NameKind::Event);

impl TagName {
    #[must_use]
    pub fn is_void_in(&self, namespace: ElementNamespace) -> bool {
        namespace == ElementNamespace::Html
            && HTML_VOID_TAGS
                .iter()
                .any(|tag| self.as_str().eq_ignore_ascii_case(tag))
    }
}

const HTML_VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn validate(value: &str, kind: NameKind) -> Result<(), NameViolation> {
    if value.is_empty() {
        return Err(NameViolation::Empty);
    }
    if value.len() > MAX_DOM_NAME_BYTES {
        return Err(NameViolation::TooLong {
            actual: value.len(),
            maximum: MAX_DOM_NAME_BYTES,
        });
    }

    let mut chars = value.char_indices();
    let (_, first) = chars.next().expect("non-empty name");
    if !is_name_start(first) {
        return Err(NameViolation::InvalidStart {
            index: 0,
            character: first,
        });
    }

    let mut separators = 0;
    let mut previous_was_separator = false;
    for (index, character) in chars {
        if character == ':' {
            if kind == NameKind::Tag {
                return Err(NameViolation::InvalidCharacter { index, character });
            }
            separators += 1;
            if (kind == NameKind::Attribute && separators > 1)
                || previous_was_separator
                || index + character.len_utf8() == value.len()
            {
                return Err(NameViolation::InvalidNamespaceSeparator { index });
            }
            if kind == NameKind::Attribute {
                let prefix = &value[..index];
                if !["xlink", "xml", "xmlns"].contains(&prefix) {
                    return Err(NameViolation::UnsupportedNamespacePrefix {
                        prefix: prefix.to_owned(),
                    });
                }
            }
            previous_was_separator = true;
        } else if !is_name_continue(character) {
            return Err(NameViolation::InvalidCharacter { index, character });
        } else if previous_was_separator && !is_name_start(character) {
            return Err(NameViolation::InvalidStart { index, character });
        } else {
            previous_was_separator = false;
        }
    }

    Ok(())
}

fn is_name_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

fn is_name_continue(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '-' | '_' | '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_html_svg_custom_and_accessibility_names() {
        for tag in ["main", "linearGradient", "foreignObject", "pliego-card"] {
            TagName::new(tag).unwrap();
        }
        for attribute in [
            "class",
            "viewBox",
            "data-route-id",
            "aria-labelledby",
            "xlink:href",
            "xml:space",
            "xmlns:xlink",
        ] {
            AttributeName::new(attribute).unwrap();
        }
        for event in [
            "click",
            "DOMContentLoaded",
            "pliego:update",
            "pliego:route:before",
            "route.before",
        ] {
            EventName::new(event).unwrap();
        }
    }

    #[test]
    fn rejects_markup_delimiters_whitespace_controls_and_bad_qualifiers() {
        for candidate in [
            "", " div", "di v", "div\n", "div\0", "div>", "div/", "div=", "div\"", "div'", "div<",
            "svg:path",
        ] {
            assert!(
                TagName::new(candidate).is_err(),
                "accepted tag {candidate:?}"
            );
        }
        for candidate in [
            "data-x=y",
            "xlink::href",
            "xlink:",
            ":href",
            "aria:label",
            "XLINK:href",
            "xlink:1href",
            "title\t",
        ] {
            assert!(
                AttributeName::new(candidate).is_err(),
                "accepted attribute {candidate:?}"
            );
        }
        for candidate in [
            "mouse down",
            "click/on",
            "route::before",
            "route:1before",
            "load\r",
        ] {
            assert!(
                EventName::new(candidate).is_err(),
                "accepted event {candidate:?}"
            );
        }
    }

    #[test]
    fn enforces_encoded_name_bound() {
        let maximum = "a".repeat(MAX_DOM_NAME_BYTES);
        assert!(TagName::new(maximum).is_ok());

        let too_long = "a".repeat(MAX_DOM_NAME_BYTES * 1_024);
        let error = TagName::new(too_long).unwrap_err();
        assert!(matches!(error.violation, NameViolation::TooLong { .. }));
        assert_eq!(error.value_preview.len(), NAME_ERROR_PREVIEW_BYTES);
        assert!(error.preview_truncated);
    }

    #[test]
    fn owned_try_from_reuses_the_validated_string() {
        let owned = String::from("pliego-card");
        let original = owned.as_ptr();
        let name = TagName::try_from(owned).unwrap();
        assert_eq!(name.as_str().as_ptr(), original);
    }

    #[test]
    fn namespace_policy_matches_svg_html_integration_points() {
        let svg = TagName::new("svg").unwrap();
        assert_eq!(
            ElementNamespace::Html.for_element(&svg),
            ElementNamespace::Svg
        );
        for integration_point in ["foreignObject", "desc", "title"] {
            let tag = TagName::new(integration_point).unwrap();
            assert_eq!(
                ElementNamespace::Svg.for_children(&tag),
                ElementNamespace::Html,
                "wrong child namespace for {integration_point}"
            );
        }
    }
}
