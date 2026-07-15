// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Bounded, deterministic state codecs for projection snapshots.

use std::error::Error;
use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// Maximum canonical state payload accepted by the built-in codec (8 MiB).
pub const MAX_CANONICAL_STATE_BYTES: usize = 8 * 1024 * 1024;

/// A stable codec identifier embedded in every projection snapshot.
pub const CANONICAL_JSON_CODEC_ID: &str = "pliego/canonical-json/1";

/// A state encoding or decoding failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The codec limit was zero or exceeded the framework ceiling.
    InvalidLimit { requested: usize, maximum: usize },
    /// Canonical state exceeded the configured bound.
    TooLarge { actual: usize, maximum: usize },
    /// Serialization failed.
    Encode(String),
    /// Deserialization failed.
    Decode(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit { requested, maximum } => write!(
                f,
                "invalid state-codec limit {requested}; expected 1..={maximum} bytes"
            ),
            Self::TooLarge { actual, maximum } => {
                write!(f, "canonical state is {actual} bytes; limit is {maximum}")
            }
            Self::Encode(message) => write!(f, "state encoding failed: {message}"),
            Self::Decode(message) => write!(f, "state decoding failed: {message}"),
        }
    }
}

impl Error for CodecError {}

/// Deterministic state encoding used by [`crate::ProjectionSnapshot`].
///
/// Implementations must return the same bytes for equal logical state. Restore
/// always decodes and re-encodes, then requires byte-for-byte equality; a codec
/// that cannot satisfy that contract is rejected rather than trusted.
pub trait StateCodec<S>: 'static {
    /// Stable identifier for the encoding contract, including its revision.
    fn id(&self) -> &str;

    /// Encode state to canonical, bounded bytes.
    fn encode(&self, state: &S) -> Result<Vec<u8>, CodecError>;

    /// Decode canonical bytes. Callers still re-encode to prove canonicality.
    fn decode(&self, bytes: &[u8]) -> Result<S, CodecError>;
}

/// Compact JSON with recursively sorted object keys and a strict byte bound.
///
/// This is Pliego's canonical JSON profile, not a claim of RFC 8785
/// compatibility. It uses `serde_json`'s normalized compact representation and
/// recursively orders every object before emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalJsonCodec {
    max_bytes: usize,
}

impl CanonicalJsonCodec {
    /// Construct a codec with a custom limit no larger than the snapshot ceiling.
    pub fn with_max_bytes(max_bytes: usize) -> Result<Self, CodecError> {
        if !(1..=MAX_CANONICAL_STATE_BYTES).contains(&max_bytes) {
            return Err(CodecError::InvalidLimit {
                requested: max_bytes,
                maximum: MAX_CANONICAL_STATE_BYTES,
            });
        }
        Ok(Self { max_bytes })
    }

    /// Configured maximum canonical payload size.
    #[must_use]
    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    fn check_len(&self, len: usize) -> Result<(), CodecError> {
        if len > self.max_bytes {
            return Err(CodecError::TooLarge {
                actual: len,
                maximum: self.max_bytes,
            });
        }
        Ok(())
    }
}

impl Default for CanonicalJsonCodec {
    fn default() -> Self {
        Self {
            max_bytes: MAX_CANONICAL_STATE_BYTES,
        }
    }
}

impl<S> StateCodec<S> for CanonicalJsonCodec
where
    S: Serialize + DeserializeOwned + 'static,
{
    fn id(&self) -> &str {
        CANONICAL_JSON_CODEC_ID
    }

    fn encode(&self, state: &S) -> Result<Vec<u8>, CodecError> {
        let bytes = encode_canonical_json(state)?;
        self.check_len(bytes.len())?;
        Ok(bytes)
    }

    fn decode(&self, bytes: &[u8]) -> Result<S, CodecError> {
        self.check_len(bytes.len())?;
        serde_json::from_slice(bytes).map_err(|error| CodecError::Decode(error.to_string()))
    }
}

pub(crate) fn encode_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let value =
        serde_json::to_value(value).map_err(|error| CodecError::Encode(error.to_string()))?;
    let canonical = canonicalize(value);
    serde_json::to_vec(&canonical).map_err(|error| CodecError::Encode(error.to_string()))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut entries: Vec<_> = values.into_iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            let mut sorted = Map::new();
            for (key, value) in entries {
                sorted.insert(key, canonicalize(value));
            }
            Value::Object(sorted)
        }
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct State {
        labels: HashMap<String, u64>,
    }

    #[test]
    fn canonical_json_sorts_maps_and_round_trips() {
        let state = State {
            labels: HashMap::from([("z".into(), 1), ("a".into(), 2)]),
        };
        let codec = CanonicalJsonCodec::default();
        let bytes = codec.encode(&state).unwrap();
        assert_eq!(bytes, br#"{"labels":{"a":2,"z":1}}"#);
        assert_eq!(codec.decode(&bytes), Ok(state));
    }

    #[test]
    fn canonical_json_enforces_bounds_before_decode() {
        let codec = CanonicalJsonCodec::with_max_bytes(4).unwrap();
        assert_eq!(
            <CanonicalJsonCodec as StateCodec<String>>::decode(&codec, br#""large""#),
            Err(CodecError::TooLarge {
                actual: 7,
                maximum: 4,
            })
        );
    }
}
