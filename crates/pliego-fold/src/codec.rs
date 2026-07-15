// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Bounded, deterministic state codecs for projection snapshots.

use std::error::Error;
use std::fmt;
use std::io::{self, Write};

use serde::Serialize;
use serde::de::{DeserializeOwned, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::snapshot::CodecIdentity;

/// Maximum canonical state payload accepted by the built-in codec (8 MiB).
pub const MAX_CANONICAL_STATE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum nesting depth admitted before decoding snapshot state.
pub const MAX_CANONICAL_STATE_DEPTH: usize = 64;
/// Maximum JSON values admitted before decoding snapshot state.
pub const MAX_CANONICAL_STATE_NODES: usize = 262_144;

/// A stable codec identifier embedded in every projection snapshot.
pub const CANONICAL_JSON_CODEC_ID: &str = "pliego/canonical-json/1";

/// A state encoding or decoding failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The codec limit was zero or exceeded the framework ceiling.
    InvalidLimit { requested: usize, maximum: usize },
    /// Canonical state exceeded the configured bound.
    TooLarge { actual: usize, maximum: usize },
    /// Canonical state nesting exceeded the structural bound.
    TooDeep { actual: usize, maximum: usize },
    /// Canonical state contained too many structural values.
    TooManyNodes { actual: usize, maximum: usize },
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
            Self::TooDeep { actual, maximum } => {
                write!(f, "canonical state depth is {actual}; limit is {maximum}")
            }
            Self::TooManyNodes { actual, maximum } => {
                write!(f, "canonical state has {actual} nodes; limit is {maximum}")
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
    /// Stable encoding identity, semantic revision, and configuration digest.
    fn identity(&self) -> CodecIdentity;

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
    fn identity(&self) -> CodecIdentity {
        CodecIdentity::from_serializable_config(
            CANONICAL_JSON_CODEC_ID,
            1,
            &CanonicalJsonConfig {
                max_bytes: self.max_bytes,
                max_depth: MAX_CANONICAL_STATE_DEPTH,
                max_nodes: MAX_CANONICAL_STATE_NODES,
            },
        )
        .expect("built-in codec identity is valid")
    }

    fn encode(&self, state: &S) -> Result<Vec<u8>, CodecError> {
        encode_canonical_json_bounded(state, self.max_bytes)
    }

    fn decode(&self, bytes: &[u8]) -> Result<S, CodecError> {
        self.check_len(bytes.len())?;
        preflight_json(bytes, CodecOperation::Decode)?;
        serde_json::from_slice(bytes).map_err(|error| CodecError::Decode(error.to_string()))
    }
}

#[derive(Serialize)]
struct CanonicalJsonConfig {
    max_bytes: usize,
    max_depth: usize,
    max_nodes: usize,
}

pub(crate) fn encode_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    encode_canonical_json_bounded(value, MAX_CANONICAL_STATE_BYTES)
}

fn encode_canonical_json_bounded<T: Serialize>(
    value: &T,
    max_bytes: usize,
) -> Result<Vec<u8>, CodecError> {
    let source = serialize_bounded(value, max_bytes)?;
    preflight_json(&source, CodecOperation::Encode)?;
    let value =
        serde_json::from_slice(&source).map_err(|error| CodecError::Encode(error.to_string()))?;
    let canonical = canonicalize(value);
    serialize_bounded(&canonical, max_bytes)
}

#[derive(Clone, Copy)]
enum CodecOperation {
    Encode,
    Decode,
}

#[derive(Clone, Copy)]
enum ShapeViolation {
    Depth(usize),
    Nodes(usize),
}

struct JsonBudget {
    nodes: usize,
    violation: Option<ShapeViolation>,
}

fn preflight_json(bytes: &[u8], operation: CodecOperation) -> Result<(), CodecError> {
    let mut budget = JsonBudget {
        nodes: 0,
        violation: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let result = JsonSeed {
        budget: &mut budget,
        depth: 1,
    }
    .deserialize(&mut deserializer)
    .and_then(|()| deserializer.end());
    if let Some(violation) = budget.violation {
        return Err(match violation {
            ShapeViolation::Depth(actual) => CodecError::TooDeep {
                actual,
                maximum: MAX_CANONICAL_STATE_DEPTH,
            },
            ShapeViolation::Nodes(actual) => CodecError::TooManyNodes {
                actual,
                maximum: MAX_CANONICAL_STATE_NODES,
            },
        });
    }
    result.map_err(|error| match operation {
        CodecOperation::Encode => CodecError::Encode(error.to_string()),
        CodecOperation::Decode => CodecError::Decode(error.to_string()),
    })
}

struct JsonSeed<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for JsonSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.depth > MAX_CANONICAL_STATE_DEPTH {
            self.budget.violation = Some(ShapeViolation::Depth(self.depth));
            return Err(serde::de::Error::custom(
                "canonical state exceeds depth limit",
            ));
        }
        self.budget.nodes = self.budget.nodes.saturating_add(1);
        if self.budget.nodes > MAX_CANONICAL_STATE_NODES {
            self.budget.violation = Some(ShapeViolation::Nodes(self.budget.nodes));
            return Err(serde::de::Error::custom(
                "canonical state exceeds node limit",
            ));
        }
        deserializer.deserialize_any(JsonVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct JsonVisitor<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> Visitor<'de> for JsonVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON state")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_string<E>(self, _value: String) -> Result<(), E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        JsonSeed {
            budget: self.budget,
            depth: self.depth.saturating_add(1),
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence
            .next_element_seed(JsonSeed {
                budget: self.budget,
                depth: self.depth.saturating_add(1),
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A>(self, mut object: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        while object.next_key::<IgnoredAny>()?.is_some() {
            object.next_value_seed(JsonSeed {
                budget: self.budget,
                depth: self.depth.saturating_add(1),
            })?;
        }
        Ok(())
    }
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
        Value::Number(number)
            if json_number_is_float(&number.to_string())
                && json_number_is_zero(&number.to_string()) =>
        {
            Value::Number(
                serde_json::Number::from_f64(0.0).expect("positive zero is a finite JSON number"),
            )
        }
        scalar => scalar,
    }
}

fn json_number_is_float(number: &str) -> bool {
    number
        .bytes()
        .any(|byte| matches!(byte, b'.' | b'e' | b'E'))
}

fn json_number_is_zero(number: &str) -> bool {
    let mantissa = number
        .strip_prefix('-')
        .unwrap_or(number)
        .split_once(['e', 'E'])
        .map_or_else(
            || number.strip_prefix('-').unwrap_or(number),
            |(head, _)| head,
        );
    mantissa.bytes().all(|byte| byte == b'0' || byte == b'.')
}

fn serialize_bounded<T: Serialize>(value: &T, max_bytes: usize) -> Result<Vec<u8>, CodecError> {
    let mut writer = BoundedWriter::new(max_bytes);
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        if let Some(actual) = writer.attempted_size {
            return Err(CodecError::TooLarge {
                actual,
                maximum: max_bytes,
            });
        }
        return Err(CodecError::Encode(error.to_string()));
    }
    Ok(writer.bytes)
}

struct BoundedWriter {
    bytes: Vec<u8>,
    maximum: usize,
    attempted_size: Option<usize>,
}

impl BoundedWriter {
    fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum,
            attempted_size: None,
        }
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let attempted = self
            .bytes
            .len()
            .checked_add(buffer.len())
            .unwrap_or(usize::MAX);
        if attempted > self.maximum {
            self.attempted_size = Some(attempted);
            return Err(io::Error::other("canonical state exceeds byte limit"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
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

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct FloatState {
        value: f64,
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
        assert!(matches!(
            <CanonicalJsonCodec as StateCodec<String>>::encode(&codec, &"large".to_owned()),
            Err(CodecError::TooLarge { maximum: 4, .. })
        ));
    }

    #[test]
    fn canonical_json_preflights_depth_and_nodes() {
        let codec = CanonicalJsonCodec::default();
        let deep = format!(
            "{}0{}",
            "[".repeat(MAX_CANONICAL_STATE_DEPTH),
            "]".repeat(MAX_CANONICAL_STATE_DEPTH)
        );
        assert!(matches!(
            <CanonicalJsonCodec as StateCodec<Value>>::decode(&codec, deep.as_bytes()),
            Err(CodecError::TooDeep { .. })
        ));

        let wide = format!("[{}]", "0,".repeat(MAX_CANONICAL_STATE_NODES) + "0");
        assert!(matches!(
            <CanonicalJsonCodec as StateCodec<Value>>::decode(&codec, wide.as_bytes()),
            Err(CodecError::TooManyNodes { .. })
        ));
    }

    #[test]
    fn canonical_json_normalizes_negative_zero() {
        let codec = CanonicalJsonCodec::default();
        let negative = codec.encode(&FloatState { value: -0.0 }).unwrap();
        let positive = codec.encode(&FloatState { value: 0.0 }).unwrap();
        assert_eq!(negative, positive);
        assert_eq!(positive, br#"{"value":0.0}"#);
        assert_eq!(codec.decode(&negative), Ok(FloatState { value: 0.0 }));
        let value = serde_json::json!({ "value": -0.0 });
        let value_bytes = codec.encode(&value).unwrap();
        assert_eq!(value_bytes, br#"{"value":0.0}"#);
        assert_eq!(
            <CanonicalJsonCodec as StateCodec<Value>>::decode(&codec, &value_bytes),
            Ok(serde_json::json!({ "value": 0.0 }))
        );
    }
}
