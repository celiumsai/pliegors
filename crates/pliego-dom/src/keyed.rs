// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use crate::View;

/// Maximum number of rows collected by one keyed view update.
pub const MAX_KEYED_ITEMS: usize = 65_536;
/// Maximum UTF-8 bytes accepted by one textual key.
pub const MAX_KEYED_TEXT_BYTES: usize = 256;
/// Maximum aggregate key material accepted by one keyed view update.
pub const MAX_KEYED_BYTES: usize = 8 * 1024 * 1024;

/// Stable identity used by keyed DOM reconciliation.
///
/// Signed, unsigned, and textual keys are distinct domains. In particular,
/// `1_i64` and `1_u64` do not identify the same row.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum KeyedKey {
    /// A signed integer identity.
    Signed(i128),
    /// An unsigned integer identity.
    Unsigned(u128),
    /// A non-empty, bounded UTF-8 identity.
    Text(String),
}

impl KeyedKey {
    fn validated(self) -> Result<Self, KeyedError> {
        match &self {
            Self::Text(value) if value.is_empty() => Err(KeyedError::EmptyTextKey),
            Self::Text(value) if value.len() > MAX_KEYED_TEXT_BYTES => {
                Err(KeyedError::TextKeyTooLong {
                    bytes: value.len(),
                    limit: MAX_KEYED_TEXT_BYTES,
                })
            }
            _ => Ok(self),
        }
    }

    pub(crate) fn byte_cost(&self) -> usize {
        match self {
            Self::Signed(_) | Self::Unsigned(_) => 16,
            Self::Text(value) => value.len(),
        }
    }
}

impl fmt::Display for KeyedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signed(value) => write!(f, "signed:{value}"),
            Self::Unsigned(value) => write!(f, "unsigned:{value}"),
            Self::Text(value) => write!(f, "text:{value:?}"),
        }
    }
}

/// Conversion into a bounded keyed identity.
pub trait IntoKeyedKey {
    fn into_keyed_key(self) -> Result<KeyedKey, KeyedError>;
}

macro_rules! signed_keys {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl IntoKeyedKey for $ty {
                fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
                    Ok(KeyedKey::Signed(i128::from(self)))
                }
            }
        )+
    };
}

macro_rules! unsigned_keys {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl IntoKeyedKey for $ty {
                fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
                    Ok(KeyedKey::Unsigned(u128::from(self)))
                }
            }
        )+
    };
}

signed_keys!(i8, i16, i32, i64, i128);
unsigned_keys!(u8, u16, u32, u64, u128);

impl IntoKeyedKey for isize {
    fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
        Ok(KeyedKey::Signed(self as i128))
    }
}

impl IntoKeyedKey for usize {
    fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
        Ok(KeyedKey::Unsigned(self as u128))
    }
}

impl IntoKeyedKey for String {
    fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
        KeyedKey::Text(self).validated()
    }
}

impl IntoKeyedKey for &str {
    fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
        KeyedKey::Text(self.to_owned()).validated()
    }
}

impl IntoKeyedKey for KeyedKey {
    fn into_keyed_key(self) -> Result<KeyedKey, KeyedError> {
        self.validated()
    }
}

/// Stage at which a fallible keyed callback rejected an update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyedStage {
    /// Producing the collection failed.
    Collection,
    /// Deriving one stable identity failed.
    Key,
    /// Building one new row failed.
    Row,
}

impl fmt::Display for KeyedStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Collection => "collection",
            Self::Key => "key",
            Self::Row => "row",
        })
    }
}

/// Bounded, deterministic keyed collection failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyedError {
    /// One update exceeded the maximum number of rows.
    ItemLimit { limit: usize },
    /// One update exceeded the aggregate key-material budget.
    KeyBytesLimit { limit: usize },
    /// Text identities must not be empty.
    EmptyTextKey,
    /// A text identity exceeded its per-key byte budget.
    TextKeyTooLong { bytes: usize, limit: usize },
    /// Two rows resolved to the same typed identity.
    DuplicateKey { key: KeyedKey },
    /// Comment boundaries would change parsing under this direct parent.
    UnsupportedParent { tag: String },
    /// A user-provided fallible callback rejected the update.
    Callback { stage: KeyedStage, message: String },
}

impl KeyedError {
    /// Construct a bounded error for a fallible user callback.
    pub fn callback(stage: KeyedStage, message: impl Into<String>) -> Self {
        let mut message = message.into();
        const MAX_MESSAGE_BYTES: usize = 512;
        if message.len() > MAX_MESSAGE_BYTES {
            let mut boundary = MAX_MESSAGE_BYTES;
            while !message.is_char_boundary(boundary) {
                boundary -= 1;
            }
            message.truncate(boundary);
        }
        Self::Callback { stage, message }
    }
}

impl fmt::Display for KeyedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ItemLimit { limit } => write!(f, "keyed collection exceeds {limit} rows"),
            Self::KeyBytesLimit { limit } => {
                write!(f, "keyed collection exceeds {limit} key bytes")
            }
            Self::EmptyTextKey => f.write_str("keyed text keys cannot be empty"),
            Self::TextKeyTooLong { bytes, limit } => {
                write!(f, "keyed text key uses {bytes} bytes; limit is {limit}")
            }
            Self::DuplicateKey { key } => write!(f, "duplicate keyed identity {key}"),
            Self::UnsupportedParent { tag } => {
                write!(f, "keyed views are not supported directly under {tag:?}")
            }
            Self::Callback { stage, message } => {
                write!(f, "keyed {stage} callback failed: {message}")
            }
        }
    }
}

impl std::error::Error for KeyedError {}

pub(crate) struct PendingKeyedRow {
    pub(crate) key: KeyedKey,
    builder: Box<dyn FnOnce() -> Result<View, KeyedError>>,
}

impl PendingKeyedRow {
    pub(crate) fn build(self) -> Result<(KeyedKey, View), KeyedError> {
        let view = (self.builder)()?;
        Ok((self.key, view))
    }
}

#[doc(hidden)]
pub struct KeyedSpec {
    collect: Rc<dyn Fn() -> Result<Vec<PendingKeyedRow>, KeyedError>>,
}

impl KeyedSpec {
    pub(crate) fn collect(&self) -> Result<Vec<PendingKeyedRow>, KeyedError> {
        (self.collect)()
    }
}

/// Build a keyed collection whose callbacks cannot fail explicitly.
///
/// The row builder runs once for each key lifetime. If data associated with a
/// retained key must change, capture reactive values inside the returned view.
pub fn keyed<T, I, K>(
    each: impl Fn() -> I + 'static,
    key: impl Fn(&T) -> K + 'static,
    row: impl Fn(T) -> View + 'static,
) -> View
where
    T: 'static,
    I: IntoIterator<Item = T>,
    K: IntoKeyedKey + 'static,
{
    try_keyed(
        move || Ok(each()),
        move |item| Ok(key(item)),
        move |item| Ok(row(item)),
    )
}

/// Build a keyed collection with explicit fallible collection, key, and row
/// callbacks. Duplicate and oversized keys are rejected before any row builder
/// executes.
pub fn try_keyed<T, I, K>(
    each: impl Fn() -> Result<I, KeyedError> + 'static,
    key: impl Fn(&T) -> Result<K, KeyedError> + 'static,
    row: impl Fn(T) -> Result<View, KeyedError> + 'static,
) -> View
where
    T: 'static,
    I: IntoIterator<Item = T>,
    K: IntoKeyedKey + 'static,
{
    let key = Rc::new(key);
    let row = Rc::new(row);
    let collect = move || {
        let mut seen = HashSet::new();
        let mut keyed_items = Vec::new();
        let mut key_bytes = 0_usize;

        for item in each()?.into_iter() {
            if keyed_items.len() == MAX_KEYED_ITEMS {
                return Err(KeyedError::ItemLimit {
                    limit: MAX_KEYED_ITEMS,
                });
            }
            let resolved = key(&item)?.into_keyed_key()?;
            key_bytes =
                key_bytes
                    .checked_add(resolved.byte_cost())
                    .ok_or(KeyedError::KeyBytesLimit {
                        limit: MAX_KEYED_BYTES,
                    })?;
            if key_bytes > MAX_KEYED_BYTES {
                return Err(KeyedError::KeyBytesLimit {
                    limit: MAX_KEYED_BYTES,
                });
            }
            if !seen.insert(resolved.clone()) {
                return Err(KeyedError::DuplicateKey { key: resolved });
            }
            keyed_items.push((resolved, item));
        }

        Ok(keyed_items
            .into_iter()
            .map(|(key, item)| {
                let row = Rc::clone(&row);
                PendingKeyedRow {
                    key,
                    builder: Box::new(move || row(item)),
                }
            })
            .collect())
    };

    View::Keyed(Rc::new(KeyedSpec {
        collect: Rc::new(collect),
    }))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::{IntoView, RenderError, RenderLimits, el, text, try_render_html};

    #[test]
    fn duplicate_keys_fail_before_any_row_builder_runs() {
        let builds = Rc::new(Cell::new(0));
        let observed = Rc::clone(&builds);
        let view = keyed(
            || [1_u64, 1_u64],
            |item| *item,
            move |item| {
                observed.set(observed.get() + 1);
                text(item.to_string())
            },
        );

        assert!(matches!(
            try_render_html(&view, RenderLimits::default()),
            Err(RenderError::Keyed(KeyedError::DuplicateKey {
                key: KeyedKey::Unsigned(1)
            }))
        ));
        assert_eq!(builds.get(), 0);
    }

    #[test]
    fn signed_unsigned_and_text_keys_are_separate_domains() {
        let keys = [
            KeyedKey::Signed(1),
            KeyedKey::Unsigned(1),
            KeyedKey::Text("1".to_owned()),
        ];
        let view = keyed(
            move || keys.clone(),
            Clone::clone,
            |key| text(key.to_string()),
        );

        assert_eq!(
            try_render_html(&view, RenderLimits::default()).unwrap(),
            "signed:1unsigned:1text:\"1\""
        );
    }

    #[test]
    fn text_and_collection_limits_fail_closed() {
        assert!(matches!("".into_keyed_key(), Err(KeyedError::EmptyTextKey)));
        assert!(matches!(
            "x".repeat(MAX_KEYED_TEXT_BYTES + 1).into_keyed_key(),
            Err(KeyedError::TextKeyTooLong { .. })
        ));

        let view = keyed(
            || 0..=MAX_KEYED_ITEMS,
            |item| *item,
            |item| text(item.to_string()),
        );
        assert!(matches!(
            try_render_html(&view, RenderLimits::default()),
            Err(RenderError::Keyed(KeyedError::ItemLimit { .. }))
        ));
    }

    #[test]
    fn fallible_callback_error_is_bounded_and_preserves_its_stage() {
        let view = try_keyed(
            || -> Result<Vec<u32>, KeyedError> {
                Err(KeyedError::callback(
                    KeyedStage::Collection,
                    "é".repeat(400),
                ))
            },
            |item| Ok(*item),
            |item| Ok(text(item.to_string())),
        );

        let Err(RenderError::Keyed(KeyedError::Callback { stage, message })) =
            try_render_html(&view, RenderLimits::default())
        else {
            panic!("fallible keyed collection was not rejected");
        };
        assert_eq!(stage, KeyedStage::Collection);
        assert_eq!(message.len(), 512);
        assert!(message.is_char_boundary(message.len()));
    }

    #[test]
    fn marker_unsafe_parent_is_rejected_by_ssr() {
        let view = el("pre")
            .child(keyed(
                || [1_u32],
                |item| *item,
                |item| text(item.to_string()),
            ))
            .into_view();

        assert!(matches!(
            try_render_html(&view, RenderLimits::default()),
            Err(RenderError::Keyed(KeyedError::UnsupportedParent { tag })) if tag == "pre"
        ));
    }
}
