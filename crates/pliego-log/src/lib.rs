// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![forbid(unsafe_code)]

//! Typed, versioned, hash-chained application history for PliegoRS.
//!
//! The crate owns four trust boundaries:
//!
//! - [`CanonicalJson`] admits bounded JSON while rejecting duplicate object keys;
//! - [`Log`] appends typed `app_*` events and verifies the complete hash chain;
//! - [`LogCursor`] binds a position to the exact history head at that position;
//! - [`SealedEventCatalog`] admits exact schema versions and applies only explicit,
//!   adjacent upcasters before producing typed reducer input.
//!
//! Projection/fold behavior deliberately lives in `pliego-fold`, not here.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Hash domain for version 2 log entries.
pub const EVENT_HASH_DOMAIN: &[u8] = b"pliego-log/2";
/// Hash domain for sealed schema catalogs.
pub const SCHEMA_SET_HASH_DOMAIN: &[u8] = b"pliego-log/2/schema-set";
/// Maximum accepted source or canonical JSON payload size.
pub const MAX_JSON_BYTES: usize = 256 * 1024;
/// Maximum nesting depth accepted by canonical JSON parsing.
pub const MAX_JSON_DEPTH: usize = 64;
/// Maximum number of JSON values accepted in one payload.
pub const MAX_JSON_NODES: usize = 65_536;
/// Maximum application event kind length.
pub const MAX_KIND_BYTES: usize = 128;
/// Maximum stable schema or upcaster identifier length.
pub const MAX_CONTRACT_ID_BYTES: usize = 256;
/// Maximum numeric event schema version admitted by the local contract.
pub const MAX_SCHEMA_VERSION: u32 = 4_096;
/// Maximum distinct event kinds in one sealed catalog.
pub const MAX_CATALOG_KINDS: usize = 1_024;
/// Maximum admitted schema versions for one event kind.
pub const MAX_SCHEMAS_PER_KIND: usize = 1_024;
/// Maximum remembered input/output pairs for one mapper or upcaster.
///
/// The cache is a bounded runtime tripwire for transforms that change their
/// result across repeated decodes of the same canonical input. It supplements,
/// but cannot replace, the requirement that application transforms are pure.
pub const MAX_TRANSFORM_OBSERVATIONS: usize = 256;

/// A 32-byte SHA-256 digest.
pub type Hash = [u8; 32];

const GENESIS_HASH: Hash = [0; 32];
const SERDE_ARBITRARY_PRECISION_NUMBER_KEY: &str = "$serde_json::private::Number";

/// Stable schema identity for an application payload type.
///
/// Implementations are validated when registered or appended. `KIND` must be a
/// bounded `app_*` identifier, `VERSION` must be positive, and `SCHEMA_ID` must
/// be a stable portable identifier. Rust type names are intentionally excluded
/// from the wire and digest contracts.
pub trait EventSchema {
    /// Stable application event discriminant.
    const KIND: &'static str;
    /// Positive schema version.
    const VERSION: u32;
    /// Stable identifier for this exact schema definition.
    const SCHEMA_ID: &'static str;
}

/// A bounded, deterministic JSON encoding.
///
/// Decimal numbers are normalized from their exact JSON lexeme without a
/// binary floating-point conversion. Equivalent decimal spellings converge,
/// while distinct arbitrary-precision values remain distinct.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalJson(Vec<u8>);

impl CanonicalJson {
    /// Parse JSON, rejecting oversized input, excessive structure, trailing
    /// input, and duplicate keys at every object depth.
    pub fn parse(raw: impl AsRef<[u8]>) -> Result<Self, CanonicalJsonError> {
        let raw = raw.as_ref();
        if raw.len() > MAX_JSON_BYTES {
            return Err(CanonicalJsonError::TooLarge {
                actual: raw.len(),
                limit: MAX_JSON_BYTES,
            });
        }
        reject_reserved_number_keys(raw)?;

        let mut deserializer = serde_json::Deserializer::from_slice(raw);
        NoDuplicates::deserialize(&mut deserializer)
            .map_err(|error| classify_json_error(error, raw.len()))?;
        deserializer
            .end()
            .map_err(|error| CanonicalJsonError::Invalid(error.to_string()))?;
        let value: Value = serde_json::from_slice(raw)
            .map_err(|error| CanonicalJsonError::Invalid(error.to_string()))?;
        validate_json_shape(&value)?;

        let mut bytes = Vec::with_capacity(raw.len());
        write_canonical(&value, &mut bytes)?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(CanonicalJsonError::TooLarge {
                actual: bytes.len(),
                limit: MAX_JSON_BYTES,
            });
        }
        Ok(Self(bytes))
    }

    /// Serialize a Rust value and admit the result through the same duplicate-
    /// aware, bounded canonicalization path used for untrusted source bytes.
    pub fn from_serialize<T: Serialize + ?Sized>(value: &T) -> Result<Self, CanonicalJsonError> {
        let mut writer = BoundedWriter::new(MAX_JSON_BYTES);
        if let Err(error) = serde_json::to_writer(&mut writer, value) {
            if let Some(actual) = writer.attempted_size {
                return Err(CanonicalJsonError::TooLarge {
                    actual,
                    limit: MAX_JSON_BYTES,
                });
            }
            return Err(CanonicalJsonError::Invalid(error.to_string()));
        }
        Self::parse(writer.bytes)
    }

    /// Canonical UTF-8 JSON bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Canonical JSON as UTF-8 text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Canonical bytes are emitted only from validated JSON strings.
        std::str::from_utf8(&self.0).expect("canonical JSON is UTF-8")
    }

    /// Decode the canonical payload into a typed schema value.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, CanonicalJsonError> {
        serde_json::from_slice(&self.0)
            .map_err(|error| CanonicalJsonError::Invalid(error.to_string()))
    }

    /// Consume the value and return its canonical bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for CanonicalJson {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CanonicalJson")
            .field(&self.as_str())
            .finish()
    }
}

impl fmt::Display for CanonicalJson {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for CanonicalJson {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value: Value = serde_json::from_slice(&self.0).map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }
}

/// Canonical JSON admission failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalJsonError {
    /// Source or canonical output exceeded the byte limit.
    TooLarge { actual: usize, limit: usize },
    /// An object repeated a key.
    DuplicateKey(String),
    /// Nesting exceeded [`MAX_JSON_DEPTH`].
    TooDeep { actual: usize, limit: usize },
    /// Structural value count exceeded [`MAX_JSON_NODES`].
    TooManyNodes { actual: usize, limit: usize },
    /// JSON syntax, number, or typed decoding was invalid.
    Invalid(String),
}

impl fmt::Display for CanonicalJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { actual, limit } => {
                write!(formatter, "JSON has {actual} bytes; limit is {limit}")
            }
            Self::DuplicateKey(key) => write!(formatter, "duplicate JSON object key `{key}`"),
            Self::TooDeep { actual, limit } => {
                write!(formatter, "JSON depth {actual} exceeds limit {limit}")
            }
            Self::TooManyNodes { actual, limit } => {
                write!(formatter, "JSON node count {actual} exceeds limit {limit}")
            }
            Self::Invalid(reason) => write!(formatter, "invalid JSON: {reason}"),
        }
    }
}

impl Error for CanonicalJsonError {}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
    attempted_size: Option<usize>,
}

impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
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
        if attempted > self.limit {
            self.attempted_size = Some(attempted);
            return Err(io::Error::other("canonical JSON exceeds byte limit"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct NoDuplicates(Value);

impl<'de> Deserialize<'de> for NoDuplicates {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DuplicateAwareVisitor;

        impl<'de> Visitor<'de> for DuplicateAwareVisitor {
            type Value = Value;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value without duplicate object keys")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(Value::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(Value::Number(value.into()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(Value::Number(value.into()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Value::String(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(Value::String(value))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(Value::Null)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(Value::Null)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                NoDuplicates::deserialize(deserializer).map(|value| value.0)
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(NoDuplicates(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(Value::Array(values))
            }

            fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = Map::new();
                while let Some(key) = object.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate JSON object key `{key}`"
                        )));
                    }
                    let NoDuplicates(value) = object.next_value()?;
                    values.insert(key, value);
                }
                Ok(Value::Object(values))
            }
        }

        deserializer
            .deserialize_any(DuplicateAwareVisitor)
            .map(Self)
    }
}

fn classify_json_error(error: serde_json::Error, raw_len: usize) -> CanonicalJsonError {
    let message = error.to_string();
    if let Some(rest) = message.strip_prefix("duplicate JSON object key `") {
        if let Some((key, _)) = rest.split_once('`') {
            return CanonicalJsonError::DuplicateKey(key.to_owned());
        }
    }
    if error.is_io() && raw_len >= MAX_JSON_BYTES {
        return CanonicalJsonError::TooLarge {
            actual: raw_len,
            limit: MAX_JSON_BYTES,
        };
    }
    CanonicalJsonError::Invalid(message)
}

fn reject_reserved_number_keys(raw: &[u8]) -> Result<(), CanonicalJsonError> {
    let mut index = 0;
    while index < raw.len() {
        if raw[index] != b'"' {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < raw.len() && raw[index] != b'"' {
            if raw[index] == b'\\' {
                index += 1;
            }
            index += 1;
        }

        let end = index;
        index += 1;
        let mut next = index;
        while next < raw.len() && raw[next].is_ascii_whitespace() {
            next += 1;
        }
        if next >= raw.len() || raw[next] != b':' {
            continue;
        }

        let key: String = serde_json::from_slice(&raw[start..=end])
            .map_err(|error| CanonicalJsonError::Invalid(error.to_string()))?;
        if key == SERDE_ARBITRARY_PRECISION_NUMBER_KEY {
            return Err(CanonicalJsonError::Invalid(format!(
                "object key `{SERDE_ARBITRARY_PRECISION_NUMBER_KEY}` is reserved"
            )));
        }
    }
    Ok(())
}

fn validate_json_shape(root: &Value) -> Result<(), CanonicalJsonError> {
    let mut stack = vec![(root, 1usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > MAX_JSON_NODES {
            return Err(CanonicalJsonError::TooManyNodes {
                actual: nodes,
                limit: MAX_JSON_NODES,
            });
        }
        if depth > MAX_JSON_DEPTH {
            return Err(CanonicalJsonError::TooDeep {
                actual: depth,
                limit: MAX_JSON_DEPTH,
            });
        }
        match value {
            Value::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth.saturating_add(1))));
            }
            Value::Object(values) => {
                stack.extend(
                    values
                        .values()
                        .map(|value| (value, depth.saturating_add(1))),
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn canonical_number(number: &serde_json::Number) -> Result<String, CanonicalJsonError> {
    let raw = number.to_string();
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map_or((false, raw.as_str()), |value| (true, value));
    let (mantissa, explicit_exponent) =
        unsigned.find(['e', 'E']).map_or((unsigned, None), |index| {
            (&unsigned[..index], Some(&unsigned[index + 1..]))
        });
    let (integer, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |(integer, fraction)| (integer, fraction));
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(CanonicalJsonError::Invalid(
            "number has an invalid decimal significand".to_owned(),
        ));
    }

    let mut digits = String::with_capacity(integer.len().saturating_add(fraction.len()));
    digits.push_str(integer);
    digits.push_str(fraction);
    let Some(first_nonzero) = digits.bytes().position(|byte| byte != b'0') else {
        return Ok("0".to_owned());
    };
    let last_nonzero = digits
        .bytes()
        .rposition(|byte| byte != b'0')
        .expect("a nonzero digit was found");
    let trailing_zeros = digits.len() - last_nonzero - 1;
    let significant = &digits[first_nonzero..=last_nonzero];

    let (exponent_negative, exponent_magnitude) = parse_decimal_exponent(explicit_exponent)?;
    let adjustment =
        -(fraction.len() as i64) + trailing_zeros as i64 + (significant.len() - 1) as i64;
    let (scientific_negative, scientific_magnitude) =
        add_signed_decimal(exponent_negative, &exponent_magnitude, adjustment);
    let small_exponent = small_signed_decimal(scientific_negative, &scientific_magnitude);

    let mut output = String::with_capacity(raw.len());
    if negative {
        output.push('-');
    }
    match small_exponent {
        Some(exponent @ 0..=20) => {
            let point = exponent as usize + 1;
            if point >= significant.len() {
                output.push_str(significant);
                output.extend(std::iter::repeat_n('0', point - significant.len()));
            } else {
                output.push_str(&significant[..point]);
                output.push('.');
                output.push_str(&significant[point..]);
            }
        }
        Some(exponent @ -6..=-1) => {
            output.push_str("0.");
            output.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
            output.push_str(significant);
        }
        _ => {
            output.push(significant.as_bytes()[0] as char);
            if significant.len() > 1 {
                output.push('.');
                output.push_str(&significant[1..]);
            }
            output.push('e');
            output.push(if scientific_negative { '-' } else { '+' });
            output.push_str(&scientific_magnitude);
        }
    }
    Ok(output)
}

fn parse_decimal_exponent(raw: Option<&str>) -> Result<(bool, String), CanonicalJsonError> {
    let Some(raw) = raw else {
        return Ok((false, "0".to_owned()));
    };
    let (negative, digits) = if let Some(digits) = raw.strip_prefix('-') {
        (true, digits)
    } else {
        (false, raw.strip_prefix('+').unwrap_or(raw))
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CanonicalJsonError::Invalid(
            "number has an invalid decimal exponent".to_owned(),
        ));
    }
    let magnitude = digits.trim_start_matches('0');
    if magnitude.is_empty() {
        Ok((false, "0".to_owned()))
    } else {
        Ok((negative, magnitude.to_owned()))
    }
}

fn add_signed_decimal(negative: bool, magnitude: &str, adjustment: i64) -> (bool, String) {
    if adjustment == 0 {
        return (negative, magnitude.to_owned());
    }
    let adjustment_negative = adjustment.is_negative();
    let adjustment_magnitude = adjustment.unsigned_abs();
    if negative == adjustment_negative {
        return (
            negative,
            add_decimal_magnitude(magnitude, adjustment_magnitude),
        );
    }

    match compare_decimal_magnitude_to_small(magnitude, adjustment_magnitude) {
        std::cmp::Ordering::Greater => (
            negative,
            subtract_small_from_decimal_magnitude(magnitude, adjustment_magnitude),
        ),
        std::cmp::Ordering::Equal => (false, "0".to_owned()),
        std::cmp::Ordering::Less => {
            let parsed = magnitude
                .parse::<u64>()
                .expect("a magnitude smaller than the small adjustment fits u64");
            (
                adjustment_negative,
                (adjustment_magnitude - parsed).to_string(),
            )
        }
    }
}

fn compare_decimal_magnitude_to_small(magnitude: &str, small: u64) -> std::cmp::Ordering {
    let small = small.to_string();
    magnitude
        .len()
        .cmp(&small.len())
        .then_with(|| magnitude.cmp(&small))
}

fn add_decimal_magnitude(magnitude: &str, small: u64) -> String {
    let mut bytes = magnitude.as_bytes().to_vec();
    let mut carry = small;
    for byte in bytes.iter_mut().rev() {
        if carry == 0 {
            break;
        }
        let sum = u64::from(*byte - b'0') + carry % 10;
        *byte = b'0' + (sum % 10) as u8;
        carry = carry / 10 + sum / 10;
    }
    let mut prefix = Vec::new();
    while carry != 0 {
        prefix.push(b'0' + (carry % 10) as u8);
        carry /= 10;
    }
    prefix.reverse();
    prefix.extend(bytes);
    String::from_utf8(prefix).expect("decimal magnitude remains ASCII")
}

fn subtract_small_from_decimal_magnitude(magnitude: &str, small: u64) -> String {
    let mut bytes = magnitude.as_bytes().to_vec();
    let mut borrow = small;
    for byte in bytes.iter_mut().rev() {
        if borrow == 0 {
            break;
        }
        let subtrahend = (borrow % 10) as u8;
        borrow /= 10;
        let digit = *byte - b'0';
        if digit < subtrahend {
            *byte = b'0' + 10 + digit - subtrahend;
            borrow += 1;
        } else {
            *byte = b'0' + digit - subtrahend;
        }
    }
    debug_assert_eq!(borrow, 0);
    let first_nonzero = bytes
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(bytes.len() - 1);
    String::from_utf8(bytes[first_nonzero..].to_vec()).expect("decimal magnitude remains ASCII")
}

fn small_signed_decimal(negative: bool, magnitude: &str) -> Option<i32> {
    let parsed = magnitude.parse::<i32>().ok()?;
    Some(if negative { -parsed } else { parsed })
}

fn write_canonical(value: &Value, output: &mut Vec<u8>) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => output.extend_from_slice(canonical_number(number)?.as_bytes()),
        Value::String(string) => serde_json::to_writer(output, string)
            .map_err(|error| CanonicalJsonError::Invalid(error.to_string()))?,
        Value::Array(values) => {
            output.push(b'[');
            for (index, item) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_canonical(item, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key)
                    .map_err(|error| CanonicalJsonError::Invalid(error.to_string()))?;
                output.push(b':');
                write_canonical(item, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// One immutable typed event in a version 2 log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// Zero-based durable sequence.
    seq: u64,
    /// Validated `app_*` event kind.
    kind: String,
    /// Positive payload schema version.
    schema_version: u32,
    /// Canonical application payload.
    payload: CanonicalJson,
    /// Hash of the preceding event, or zero for genesis.
    prev_hash: Hash,
    /// Hash binding every preceding field under [`EVENT_HASH_DOMAIN`].
    hash: Hash,
}

impl Event {
    /// Zero-based durable sequence.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Validated application event kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Positive source schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Canonical source payload.
    #[must_use]
    pub const fn payload(&self) -> &CanonicalJson {
        &self.payload
    }

    /// Previous event hash, or genesis for the first event.
    #[must_use]
    pub const fn prev_hash(&self) -> &Hash {
        &self.prev_hash
    }

    /// This event's version 2 chain hash.
    #[must_use]
    pub const fn hash(&self) -> &Hash {
        &self.hash
    }
}

/// Untrusted serialized event used by [`Log::import_raw`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    pub seq: u64,
    pub kind: String,
    pub schema_version: u32,
    pub payload_json: Vec<u8>,
    pub prev_hash: Hash,
    pub hash: Hash,
}

/// Exact history position and head hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LogCursor {
    /// Count of events already consumed.
    pub position: u64,
    /// Hash at `position - 1`, or zero at position zero.
    pub head_hash: Hash,
}

impl Default for LogCursor {
    fn default() -> Self {
        Self {
            position: 0,
            head_hash: GENESIS_HASH,
        }
    }
}

/// Failure while appending or importing log data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    InvalidKind(String),
    InvalidSchemaVersion,
    InvalidSchemaId(String),
    InvalidPayload(CanonicalJsonError),
    SequenceOverflow,
    TamperedAt(u64),
}

impl fmt::Display for LogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKind(reason) => write!(formatter, "invalid event kind: {reason}"),
            Self::InvalidSchemaVersion => write!(
                formatter,
                "schema version must be between 1 and {MAX_SCHEMA_VERSION}"
            ),
            Self::InvalidSchemaId(reason) => write!(formatter, "invalid schema id: {reason}"),
            Self::InvalidPayload(error) => write!(formatter, "invalid event payload: {error}"),
            Self::SequenceOverflow => formatter.write_str("event sequence exceeds u64"),
            Self::TamperedAt(seq) => write!(formatter, "event chain is invalid at sequence {seq}"),
        }
    }
}

impl Error for LogError {}

impl From<CanonicalJsonError> for LogError {
    fn from(error: CanonicalJsonError) -> Self {
        Self::InvalidPayload(error)
    }
}

/// A checked-cursor lookup failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    /// Position lies after the current log end.
    OutOfBounds { position: u64, len: u64 },
    /// Position exists, but the supplied head belongs to another history.
    HeadMismatch { position: u64 },
}

impl fmt::Display for CursorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { position, len } => {
                write!(
                    formatter,
                    "cursor position {position} exceeds log length {len}"
                )
            }
            Self::HeadMismatch { position } => {
                write!(
                    formatter,
                    "cursor head does not match history at position {position}"
                )
            }
        }
    }
}

impl Error for CursorError {}

/// Append-only, hash-chained application event log.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Log {
    events: Vec<Event>,
}

impl Log {
    /// Empty log at the genesis cursor.
    #[must_use]
    pub const fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Number of events.
    #[must_use]
    pub fn len(&self) -> u64 {
        u64::try_from(self.events.len()).expect("usize always fits u64 on supported targets")
    }

    /// Whether the history is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Current head hash, or zero for an empty history.
    #[must_use]
    pub fn head(&self) -> Hash {
        self.events.last().map_or(GENESIS_HASH, |event| event.hash)
    }

    /// Current exact cursor.
    #[must_use]
    pub fn cursor(&self) -> LogCursor {
        LogCursor {
            position: self.len(),
            head_hash: self.head(),
        }
    }

    /// Append a typed schema payload after canonical serialization.
    pub fn append_typed<T>(&mut self, payload: &T) -> Result<&Event, LogError>
    where
        T: EventSchema + Serialize + ?Sized,
    {
        validate_schema::<T>()?;
        let payload = CanonicalJson::from_serialize(payload)?;
        self.append_canonical(T::KIND, T::VERSION, payload)
    }

    /// Import untrusted serialized history. Every field is validated and each
    /// link is verified before that event is retained.
    ///
    /// There is deliberately no global event-count cap so this can round-trip
    /// every [`Log`] produced by [`Log::append_typed`]. A caller accepting a
    /// potentially valid, unbounded iterator must enforce its own transport or
    /// storage quota; malformed history is rejected at its first broken link.
    pub fn import_raw<I>(events: I) -> Result<Self, LogError>
    where
        I: IntoIterator<Item = RawEvent>,
    {
        let mut admitted = Vec::new();
        let mut previous = GENESIS_HASH;
        for raw in events {
            validate_kind(&raw.kind).map_err(LogError::InvalidKind)?;
            if !(1..=MAX_SCHEMA_VERSION).contains(&raw.schema_version) {
                return Err(LogError::InvalidSchemaVersion);
            }
            let expected_seq =
                u64::try_from(admitted.len()).map_err(|_| LogError::SequenceOverflow)?;
            if raw.seq != expected_seq || raw.prev_hash != previous {
                return Err(LogError::TamperedAt(expected_seq));
            }
            let event = Event {
                seq: raw.seq,
                kind: raw.kind,
                schema_version: raw.schema_version,
                payload: CanonicalJson::parse(raw.payload_json)?,
                prev_hash: raw.prev_hash,
                hash: raw.hash,
            };
            if event.hash != event_hash(&event) {
                return Err(LogError::TamperedAt(expected_seq));
            }
            previous = event.hash;
            admitted.push(event);
        }
        Ok(Self { events: admitted })
    }

    /// All admitted events.
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Read one event by its exact sequence.
    #[must_use]
    pub fn get(&self, seq: u64) -> Option<&Event> {
        usize::try_from(seq)
            .ok()
            .and_then(|index| self.events.get(index))
    }

    /// Produce a cursor at an exact position. Unlike the legacy API, this never
    /// clamps a position beyond the end of the log.
    pub fn cursor_at(&self, position: u64) -> Result<LogCursor, CursorError> {
        if position > self.len() {
            return Err(CursorError::OutOfBounds {
                position,
                len: self.len(),
            });
        }
        let head_hash = if position == 0 {
            GENESIS_HASH
        } else {
            self.get(position - 1)
                .expect("checked position has a preceding event")
                .hash
        };
        Ok(LogCursor {
            position,
            head_hash,
        })
    }

    /// Return events strictly after a checked history cursor.
    pub fn tail(&self, cursor: &LogCursor) -> Result<&[Event], CursorError> {
        let expected = self.cursor_at(cursor.position)?;
        if expected.head_hash != cursor.head_hash {
            return Err(CursorError::HeadMismatch {
                position: cursor.position,
            });
        }
        let index = usize::try_from(cursor.position).map_err(|_| CursorError::OutOfBounds {
            position: cursor.position,
            len: self.len(),
        })?;
        Ok(&self.events[index..])
    }

    /// Verify positions, links, validated schema shape, canonical payloads, and
    /// every event hash from genesis through the current head.
    pub fn verify(&self) -> Result<(), LogError> {
        let mut previous = GENESIS_HASH;
        for (index, event) in self.events.iter().enumerate() {
            let expected_seq = u64::try_from(index).map_err(|_| LogError::SequenceOverflow)?;
            if event.seq != expected_seq
                || validate_kind(&event.kind).is_err()
                || !(1..=MAX_SCHEMA_VERSION).contains(&event.schema_version)
                || event.prev_hash != previous
                || event.hash != event_hash(event)
            {
                return Err(LogError::TamperedAt(expected_seq));
            }
            previous = event.hash;
        }
        Ok(())
    }

    fn append_canonical(
        &mut self,
        kind: &str,
        schema_version: u32,
        payload: CanonicalJson,
    ) -> Result<&Event, LogError> {
        validate_kind(kind).map_err(LogError::InvalidKind)?;
        if !(1..=MAX_SCHEMA_VERSION).contains(&schema_version) {
            return Err(LogError::InvalidSchemaVersion);
        }
        let seq = u64::try_from(self.events.len()).map_err(|_| LogError::SequenceOverflow)?;
        let mut event = Event {
            seq,
            kind: kind.to_owned(),
            schema_version,
            payload,
            prev_hash: self.head(),
            hash: GENESIS_HASH,
        };
        event.hash = event_hash(&event);
        self.events.push(event);
        Ok(self.events.last().expect("event was just pushed"))
    }
}

fn event_hash(event: &Event) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(EVENT_HASH_DOMAIN);
    hasher.update(event.seq.to_be_bytes());
    hasher.update(event.prev_hash);
    update_len_prefixed(&mut hasher, event.kind.as_bytes());
    hasher.update(event.schema_version.to_be_bytes());
    update_len_prefixed(&mut hasher, event.payload.as_bytes());
    hasher.finalize().into()
}

/// Render a digest as lowercase hexadecimal.
#[must_use]
pub fn hex(hash: &Hash) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(hash.len() * 2);
    for byte in hash {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[derive(Default)]
struct TransformObservations {
    outputs: BTreeMap<Hash, Hash>,
    insertion_order: VecDeque<Hash>,
}

impl TransformObservations {
    fn accepts(&mut self, input: &CanonicalJson, output: &CanonicalJson) -> bool {
        let input_digest: Hash = Sha256::digest(input.as_bytes()).into();
        let output_digest: Hash = Sha256::digest(output.as_bytes()).into();
        if let Some(expected) = self.outputs.get(&input_digest) {
            return *expected == output_digest;
        }

        if self.outputs.len() == MAX_TRANSFORM_OBSERVATIONS {
            let oldest = self
                .insertion_order
                .pop_front()
                .expect("a full observation cache has an oldest entry");
            self.outputs.remove(&oldest);
        }
        self.outputs.insert(input_digest, output_digest);
        self.insertion_order.push_back(input_digest);
        true
    }
}

fn observed_transform_is_stable(
    observations: &Mutex<TransformObservations>,
    input: &CanonicalJson,
    output: &CanonicalJson,
) -> bool {
    observations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .accepts(input, output)
}

/// Mutable construction phase for a typed event schema catalog.
pub struct EventCatalogBuilder<E> {
    kinds: BTreeMap<String, KindBuilder<E>>,
}

impl<E> Default for EventCatalogBuilder<E> {
    fn default() -> Self {
        Self {
            kinds: BTreeMap::new(),
        }
    }
}

impl<E: 'static> EventCatalogBuilder<E> {
    /// Create an empty catalog builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the reducer-facing current schema for one event kind.
    ///
    /// `mapping_id` is a stable identity for the typed `T -> E` mapping and is
    /// included in the sealed schema-set digest. The mapper is executed twice
    /// per decode and checked against a bounded history of prior observations.
    /// This detects observed divergence; application mapper code must still be
    /// pure because finite runtime sampling cannot prove arbitrary host code.
    pub fn register_current<T, F>(
        &mut self,
        mapping_id: impl Into<String>,
        into_event: F,
    ) -> Result<&mut Self, CatalogError>
    where
        T: EventSchema + DeserializeOwned + 'static,
        E: Serialize + PartialEq,
        F: Fn(T) -> E + Send + Sync + 'static,
    {
        validate_schema::<T>().map_err(CatalogError::InvalidSchema)?;
        let mapping_id = mapping_id.into();
        validate_contract_id(&mapping_id).map_err(CatalogError::InvalidMappingId)?;
        if !self.kinds.contains_key(T::KIND) && self.kinds.len() >= MAX_CATALOG_KINDS {
            return Err(CatalogError::TooManyKinds);
        }
        let kind = self.kinds.entry(T::KIND.to_owned()).or_default();
        kind.register_schema::<T>()?;
        if kind.current.is_some() {
            return Err(CatalogError::DuplicateCurrent(T::KIND.to_owned()));
        }
        let observations = Mutex::new(TransformObservations::default());
        let decoder = move |json: &CanonicalJson| {
            let run_once = || {
                let value = json.decode::<T>().map_err(CatalogError::Payload)?;
                Ok::<E, CatalogError>(into_event(value))
            };
            let first = run_once()?;
            let second = run_once()?;
            if first != second {
                return Err(CatalogError::NondeterministicMapping {
                    kind: T::KIND.to_owned(),
                    version: T::VERSION,
                });
            }
            let first_json =
                CanonicalJson::from_serialize(&first).map_err(CatalogError::Payload)?;
            let second_json =
                CanonicalJson::from_serialize(&second).map_err(CatalogError::Payload)?;
            if first_json != second_json
                || !observed_transform_is_stable(&observations, json, &first_json)
            {
                return Err(CatalogError::NondeterministicMapping {
                    kind: T::KIND.to_owned(),
                    version: T::VERSION,
                });
            }
            Ok(first)
        };
        kind.current = Some(CurrentSchema {
            version: T::VERSION,
            mapping_id,
            decode: Arc::new(decoder),
        });
        Ok(self)
    }

    /// Register one explicit adjacent same-kind upcast edge.
    ///
    /// The transform is executed twice per decode and compared with a bounded
    /// history of prior observations for the same canonical input. This is an
    /// observed-divergence tripwire, not a proof that arbitrary host code is
    /// pure; the application still owns that invariant.
    pub fn register_upcaster<From, To, F>(
        &mut self,
        step_id: impl Into<String>,
        upcast: F,
    ) -> Result<&mut Self, CatalogError>
    where
        From: EventSchema + DeserializeOwned + 'static,
        To: EventSchema + Serialize + 'static,
        F: Fn(From) -> Result<To, String> + Send + Sync + 'static,
    {
        validate_schema::<From>().map_err(CatalogError::InvalidSchema)?;
        validate_schema::<To>().map_err(CatalogError::InvalidSchema)?;
        if From::KIND != To::KIND {
            return Err(CatalogError::CrossKindUpcast {
                from: From::KIND.to_owned(),
                to: To::KIND.to_owned(),
            });
        }
        if From::VERSION.checked_add(1) != Some(To::VERSION) {
            return Err(CatalogError::NonAdjacentUpcast {
                kind: From::KIND.to_owned(),
                from: From::VERSION,
                to: To::VERSION,
            });
        }
        let step_id = step_id.into();
        validate_contract_id(&step_id).map_err(CatalogError::InvalidStepId)?;

        if !self.kinds.contains_key(From::KIND) && self.kinds.len() >= MAX_CATALOG_KINDS {
            return Err(CatalogError::TooManyKinds);
        }
        let kind = self.kinds.entry(From::KIND.to_owned()).or_default();
        kind.register_schema::<From>()?;
        kind.register_schema::<To>()?;
        if kind.upcasters.contains_key(&From::VERSION) {
            return Err(CatalogError::DuplicateUpcaster {
                kind: From::KIND.to_owned(),
                from: From::VERSION,
            });
        }
        if kind.upcasters.len() >= MAX_SCHEMAS_PER_KIND.saturating_sub(1) {
            return Err(CatalogError::TooManyUpcasters(From::KIND.to_owned()));
        }

        let run_once = move |json: &CanonicalJson| -> Result<CanonicalJson, CatalogError> {
            let value = json.decode::<From>().map_err(CatalogError::Payload)?;
            let output = upcast(value).map_err(|reason| CatalogError::UpcastFailed {
                kind: From::KIND.to_owned(),
                from: From::VERSION,
                reason,
            })?;
            CanonicalJson::from_serialize(&output).map_err(CatalogError::Payload)
        };
        let observations = Mutex::new(TransformObservations::default());
        let deterministic = move |json: &CanonicalJson| {
            let first = run_once(json)?;
            let second = run_once(json)?;
            if first != second || !observed_transform_is_stable(&observations, json, &first) {
                return Err(CatalogError::NondeterministicUpcast {
                    kind: From::KIND.to_owned(),
                    from: From::VERSION,
                });
            }
            Ok(first)
        };
        kind.upcasters.insert(
            From::VERSION,
            Upcaster {
                to: To::VERSION,
                step_id,
                apply: Arc::new(deterministic),
            },
        );
        Ok(self)
    }

    /// Validate and freeze the complete graph, then compute its order-independent
    /// deterministic schema-set digest.
    pub fn seal(self) -> Result<SealedEventCatalog<E>, CatalogError> {
        if self.kinds.is_empty() {
            return Err(CatalogError::EmptyCatalog);
        }
        let mut sealed = BTreeMap::new();
        for (name, kind) in self.kinds {
            let current = kind
                .current
                .ok_or_else(|| CatalogError::MissingCurrent(name.clone()))?;
            if !kind.schemas.contains_key(&current.version) {
                return Err(CatalogError::MissingSchema {
                    kind: name,
                    version: current.version,
                });
            }
            let mut previous: Option<u32> = None;
            for &version in kind.schemas.keys() {
                if version > current.version {
                    return Err(CatalogError::SchemaPastCurrent {
                        kind: name,
                        version,
                        current: current.version,
                    });
                }
                if let Some(previous) = previous {
                    let expected =
                        previous
                            .checked_add(1)
                            .ok_or_else(|| CatalogError::SchemaPastCurrent {
                                kind: name.clone(),
                                version,
                                current: current.version,
                            })?;
                    if version != expected {
                        return Err(CatalogError::SchemaGap {
                            kind: name,
                            version: expected,
                        });
                    }
                }
                previous = Some(version);
            }
            for &version in kind
                .schemas
                .keys()
                .take_while(|version| **version < current.version)
            {
                let edge =
                    kind.upcasters
                        .get(&version)
                        .ok_or_else(|| CatalogError::MissingUpcaster {
                            kind: name.clone(),
                            from: version,
                        })?;
                if edge.to != version + 1 {
                    return Err(CatalogError::NonAdjacentUpcast {
                        kind: name.clone(),
                        from: version,
                        to: edge.to,
                    });
                }
            }
            if let Some((&from, _)) = kind
                .upcasters
                .iter()
                .find(|(from, edge)| **from >= current.version || edge.to > current.version)
            {
                return Err(CatalogError::UpcastPastCurrent {
                    kind: name,
                    from,
                    current: current.version,
                });
            }
            sealed.insert(
                name,
                SealedKind {
                    schemas: kind.schemas,
                    current,
                    upcasters: kind.upcasters,
                },
            );
        }
        let digest = schema_set_digest(&sealed);
        Ok(SealedEventCatalog {
            kinds: sealed,
            digest,
            marker: PhantomData,
        })
    }
}

struct KindBuilder<E> {
    schemas: BTreeMap<u32, String>,
    current: Option<CurrentSchema<E>>,
    upcasters: BTreeMap<u32, Upcaster>,
}

impl<E> Default for KindBuilder<E> {
    fn default() -> Self {
        Self {
            schemas: BTreeMap::new(),
            current: None,
            upcasters: BTreeMap::new(),
        }
    }
}

impl<E> KindBuilder<E> {
    fn register_schema<T: EventSchema>(&mut self) -> Result<(), CatalogError> {
        match self.schemas.get(&T::VERSION) {
            Some(existing) if existing == T::SCHEMA_ID => Ok(()),
            Some(_) => Err(CatalogError::DuplicateSchema {
                kind: T::KIND.to_owned(),
                version: T::VERSION,
            }),
            None => {
                if self.schemas.len() >= MAX_SCHEMAS_PER_KIND {
                    return Err(CatalogError::TooManySchemas(T::KIND.to_owned()));
                }
                self.schemas.insert(T::VERSION, T::SCHEMA_ID.to_owned());
                Ok(())
            }
        }
    }
}

type DecodeCurrent<E> = dyn Fn(&CanonicalJson) -> Result<E, CatalogError> + Send + Sync;
type ApplyUpcast = dyn Fn(&CanonicalJson) -> Result<CanonicalJson, CatalogError> + Send + Sync;

struct CurrentSchema<E> {
    version: u32,
    mapping_id: String,
    decode: Arc<DecodeCurrent<E>>,
}

struct Upcaster {
    to: u32,
    step_id: String,
    apply: Arc<ApplyUpcast>,
}

struct SealedKind<E> {
    schemas: BTreeMap<u32, String>,
    current: CurrentSchema<E>,
    upcasters: BTreeMap<u32, Upcaster>,
}

/// Immutable, validated event admission and upcast graph.
pub struct SealedEventCatalog<E> {
    kinds: BTreeMap<String, SealedKind<E>>,
    digest: Hash,
    marker: PhantomData<fn() -> E>,
}

impl<E> SealedEventCatalog<E> {
    /// Whether this exact `(kind, schema_version)` is admitted.
    #[must_use]
    pub fn supports(&self, kind: &str, schema_version: u32) -> bool {
        self.kinds
            .get(kind)
            .is_some_and(|entry| entry.schemas.contains_key(&schema_version))
    }

    /// Deterministic digest of every schema identity, target version, current
    /// mapper identity, and upcaster edge/step identity in the sealed graph.
    #[must_use]
    pub const fn schema_set_digest(&self) -> Hash {
        self.digest
    }

    /// Admit an exact event schema, apply every explicit adjacent upcaster, and
    /// decode the current schema into reducer-facing type `E`.
    pub fn decode(&self, event: &Event) -> Result<E, CatalogError> {
        let kind = self
            .kinds
            .get(&event.kind)
            .ok_or_else(|| CatalogError::UnknownKind(event.kind.clone()))?;
        if !kind.schemas.contains_key(&event.schema_version) {
            return Err(CatalogError::UnknownVersion {
                kind: event.kind.clone(),
                version: event.schema_version,
            });
        }
        if event.schema_version > kind.current.version {
            return Err(CatalogError::UnknownVersion {
                kind: event.kind.clone(),
                version: event.schema_version,
            });
        }

        let mut version = event.schema_version;
        let mut payload = event.payload.clone();
        while version < kind.current.version {
            let edge =
                kind.upcasters
                    .get(&version)
                    .ok_or_else(|| CatalogError::MissingUpcaster {
                        kind: event.kind.clone(),
                        from: version,
                    })?;
            payload = (edge.apply)(&payload)?;
            version = edge.to;
        }
        (kind.current.decode)(&payload)
    }
}

/// Event catalog construction or admission failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    EmptyCatalog,
    TooManyKinds,
    TooManySchemas(String),
    TooManyUpcasters(String),
    InvalidSchema(LogError),
    InvalidMappingId(String),
    InvalidStepId(String),
    DuplicateSchema {
        kind: String,
        version: u32,
    },
    DuplicateCurrent(String),
    DuplicateUpcaster {
        kind: String,
        from: u32,
    },
    CrossKindUpcast {
        from: String,
        to: String,
    },
    NonAdjacentUpcast {
        kind: String,
        from: u32,
        to: u32,
    },
    MissingCurrent(String),
    MissingSchema {
        kind: String,
        version: u32,
    },
    SchemaGap {
        kind: String,
        version: u32,
    },
    SchemaPastCurrent {
        kind: String,
        version: u32,
        current: u32,
    },
    MissingUpcaster {
        kind: String,
        from: u32,
    },
    UpcastPastCurrent {
        kind: String,
        from: u32,
        current: u32,
    },
    UnknownKind(String),
    UnknownVersion {
        kind: String,
        version: u32,
    },
    Payload(CanonicalJsonError),
    UpcastFailed {
        kind: String,
        from: u32,
        reason: String,
    },
    NondeterministicUpcast {
        kind: String,
        from: u32,
    },
    NondeterministicMapping {
        kind: String,
        version: u32,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCatalog => formatter.write_str("event catalog must not be empty"),
            Self::TooManyKinds => write!(
                formatter,
                "event catalog exceeds the {MAX_CATALOG_KINDS}-kind limit"
            ),
            Self::TooManySchemas(kind) => write!(
                formatter,
                "event kind `{kind}` exceeds the {MAX_SCHEMAS_PER_KIND}-schema limit"
            ),
            Self::TooManyUpcasters(kind) => {
                write!(formatter, "event kind `{kind}` exceeds the upcaster limit")
            }
            Self::InvalidSchema(error) => write!(formatter, "invalid event schema: {error}"),
            Self::InvalidMappingId(reason) => {
                write!(formatter, "invalid current-schema mapping id: {reason}")
            }
            Self::InvalidStepId(reason) => write!(formatter, "invalid upcaster step id: {reason}"),
            Self::DuplicateSchema { kind, version } => {
                write!(formatter, "duplicate schema `{kind}` version {version}")
            }
            Self::DuplicateCurrent(kind) => write!(formatter, "duplicate current schema `{kind}`"),
            Self::DuplicateUpcaster { kind, from } => {
                write!(formatter, "duplicate upcaster `{kind}` from version {from}")
            }
            Self::CrossKindUpcast { from, to } => {
                write!(formatter, "upcaster crosses event kinds `{from}` to `{to}`")
            }
            Self::NonAdjacentUpcast { kind, from, to } => {
                write!(
                    formatter,
                    "upcaster `{kind}` must be adjacent, got {from} to {to}"
                )
            }
            Self::MissingCurrent(kind) => {
                write!(formatter, "event kind `{kind}` has no current schema")
            }
            Self::MissingSchema { kind, version } => {
                write!(
                    formatter,
                    "event kind `{kind}` is missing schema version {version}"
                )
            }
            Self::SchemaGap { kind, version } => {
                write!(
                    formatter,
                    "event kind `{kind}` has a gap at version {version}"
                )
            }
            Self::SchemaPastCurrent {
                kind,
                version,
                current,
            } => write!(
                formatter,
                "event kind `{kind}` schema {version} is newer than current {current}"
            ),
            Self::MissingUpcaster { kind, from } => {
                write!(
                    formatter,
                    "event kind `{kind}` is missing upcaster from version {from}"
                )
            }
            Self::UpcastPastCurrent {
                kind,
                from,
                current,
            } => write!(
                formatter,
                "event kind `{kind}` has upcaster from {from} beyond current version {current}"
            ),
            Self::UnknownKind(kind) => write!(formatter, "unknown event kind `{kind}`"),
            Self::UnknownVersion { kind, version } => {
                write!(formatter, "unknown event schema `{kind}` version {version}")
            }
            Self::Payload(error) => write!(formatter, "event payload is invalid: {error}"),
            Self::UpcastFailed { kind, from, reason } => {
                write!(
                    formatter,
                    "upcaster `{kind}` from version {from} failed: {reason}"
                )
            }
            Self::NondeterministicUpcast { kind, from } => write!(
                formatter,
                "upcaster `{kind}` from version {from} produced different canonical outputs"
            ),
            Self::NondeterministicMapping { kind, version } => write!(
                formatter,
                "current-schema mapper `{kind}` version {version} produced different outputs"
            ),
        }
    }
}

impl Error for CatalogError {}

fn validate_schema<T: EventSchema + ?Sized>() -> Result<(), LogError> {
    validate_kind(T::KIND).map_err(LogError::InvalidKind)?;
    if !(1..=MAX_SCHEMA_VERSION).contains(&T::VERSION) {
        return Err(LogError::InvalidSchemaVersion);
    }
    validate_contract_id(T::SCHEMA_ID).map_err(LogError::InvalidSchemaId)
}

fn validate_kind(kind: &str) -> Result<(), String> {
    if !(5..=MAX_KIND_BYTES).contains(&kind.len()) {
        return Err(format!(
            "length must be between 5 and {MAX_KIND_BYTES} bytes"
        ));
    }
    let suffix = kind
        .strip_prefix("app_")
        .ok_or_else(|| "must begin with `app_`".to_owned())?;
    if suffix.is_empty()
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err("suffix must contain only lowercase ASCII, digits, or `_`".to_owned());
    }
    Ok(())
}

fn validate_contract_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_CONTRACT_ID_BYTES {
        return Err(format!(
            "length must be between 1 and {MAX_CONTRACT_ID_BYTES} bytes"
        ));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':' | b'+')
    }) {
        return Err("contains a non-portable character".to_owned());
    }
    Ok(())
}

fn schema_set_digest<E>(kinds: &BTreeMap<String, SealedKind<E>>) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(SCHEMA_SET_HASH_DOMAIN);
    hasher.update((kinds.len() as u64).to_be_bytes());
    for (name, kind) in kinds {
        update_len_prefixed(&mut hasher, name.as_bytes());
        hasher.update(kind.current.version.to_be_bytes());
        update_len_prefixed(&mut hasher, kind.current.mapping_id.as_bytes());
        hasher.update((kind.schemas.len() as u64).to_be_bytes());
        for (version, schema_id) in &kind.schemas {
            hasher.update(version.to_be_bytes());
            update_len_prefixed(&mut hasher, schema_id.as_bytes());
        }
        hasher.update((kind.upcasters.len() as u64).to_be_bytes());
        for (from, edge) in &kind.upcasters {
            hasher.update(from.to_be_bytes());
            hasher.update(edge.to.to_be_bytes());
            update_len_prefixed(&mut hasher, edge.step_id.as_bytes());
        }
    }
    hasher.finalize().into()
}

fn update_len_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TaskV1 {
        title: String,
    }

    impl EventSchema for TaskV1 {
        const KIND: &'static str = "app_task_added";
        const VERSION: u32 = 1;
        const SCHEMA_ID: &'static str = "pliego.example/task-added/1";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TaskV2 {
        title: String,
        priority: u8,
    }

    impl EventSchema for TaskV2 {
        const KIND: &'static str = "app_task_added";
        const VERSION: u32 = 2;
        const SCHEMA_ID: &'static str = "pliego.example/task-added/2";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TaskV3 {
        title: String,
        priority: u8,
        owner: String,
    }

    impl EventSchema for TaskV3 {
        const KIND: &'static str = "app_task_added";
        const VERSION: u32 = 3;
        const SCHEMA_ID: &'static str = "pliego.example/task-added/3";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TaskV4 {
        title: String,
    }

    impl EventSchema for TaskV4 {
        const KIND: &'static str = "app_task_added";
        const VERSION: u32 = 4;
        const SCHEMA_ID: &'static str = "pliego.example/task-added/4";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct AlternateTaskV2 {
        title: String,
        priority: u8,
    }

    impl EventSchema for AlternateTaskV2 {
        const KIND: &'static str = TaskV2::KIND;
        const VERSION: u32 = TaskV2::VERSION;
        const SCHEMA_ID: &'static str = "pliego.example/task-added/2-alternate";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct NoteV1 {
        body: String,
    }

    impl EventSchema for NoteV1 {
        const KIND: &'static str = "app_note_added";
        const VERSION: u32 = 1;
        const SCHEMA_ID: &'static str = "pliego.example/note-added/1";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct BigAmount {
        value: u128,
    }

    impl EventSchema for BigAmount {
        const KIND: &'static str = "app_big_amount";
        const VERSION: u32 = 1;
        const SCHEMA_ID: &'static str = "pliego.example/big-amount/1";
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    enum AppEvent {
        Task(TaskV2),
        Note(NoteV1),
    }

    fn task_catalog() -> SealedEventCatalog<AppEvent> {
        let mut builder = EventCatalogBuilder::new();
        builder
            .register_upcaster::<TaskV1, TaskV2, _>("task-title-to-priority/1", |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: 0,
                })
            })
            .unwrap()
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        builder.seal().unwrap()
    }

    #[test]
    fn canonical_json_sorts_objects_and_removes_whitespace() {
        let first = CanonicalJson::parse(br#" { "z": 1, "a": {"y":2,"x":3} } "#).unwrap();
        let second = CanonicalJson::parse(br#"{"a":{"x":3,"y":2},"z":1}"#).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.as_str(), r#"{"a":{"x":3,"y":2},"z":1}"#);
        assert_ne!(
            first,
            CanonicalJson::parse(br#"{"a":{"x":3,"y":2},"z":"1"}"#).unwrap()
        );
        assert_ne!(
            CanonicalJson::parse("[1,2]").unwrap(),
            CanonicalJson::parse("[2,1]").unwrap()
        );
    }

    #[test]
    fn canonical_json_preserves_significant_arbitrary_precision_numbers() {
        let short = CanonicalJson::parse("0.1").unwrap();
        let precise = CanonicalJson::parse("0.100000000000000000000000000000000001").unwrap();
        assert_ne!(short, precise);
        assert_eq!(precise.as_str(), "0.100000000000000000000000000000000001");

        let first = CanonicalJson::parse("18446744073709551616").unwrap();
        let second = CanonicalJson::parse("18446744073709551617").unwrap();
        assert_ne!(first, second);
        assert_eq!(second.as_str(), "18446744073709551617");
    }

    #[test]
    fn canonical_json_normalizes_equivalent_decimal_lexemes_without_float_rounding() {
        let one = CanonicalJson::parse("1").unwrap();
        for equivalent in ["1.0", "1e0", "1E+000", "10e-1"] {
            assert_eq!(CanonicalJson::parse(equivalent).unwrap(), one);
        }
        assert_eq!(CanonicalJson::from_serialize(&1.0f64).unwrap(), one);
        assert_eq!(one.as_str(), "1");

        let zero = CanonicalJson::parse("0").unwrap();
        for equivalent in ["-0", "0.0", "-0.000e+999"] {
            assert_eq!(CanonicalJson::parse(equivalent).unwrap(), zero);
        }
        assert_eq!(CanonicalJson::from_serialize(&-0.0f64).unwrap(), zero);
        assert_eq!(zero.as_str(), "0");

        assert_eq!(CanonicalJson::parse("12345e-2").unwrap().as_str(), "123.45");
        assert_eq!(
            CanonicalJson::parse("1000000000000000000000")
                .unwrap()
                .as_str(),
            "1e+21"
        );
        assert_eq!(
            CanonicalJson::parse("1e+000000000000000000021")
                .unwrap()
                .as_str(),
            "1e+21"
        );
        assert_eq!(
            CanonicalJson::parse("0.000001").unwrap().as_str(),
            "0.000001"
        );
        assert_eq!(CanonicalJson::parse("0.0000001").unwrap().as_str(), "1e-7");

        for coefficient in 1..=97i32 {
            for exponent in -24..=24i32 {
                let base = CanonicalJson::parse(format!("{coefficient}e{exponent}")).unwrap();
                let shifted =
                    CanonicalJson::parse(format!("{coefficient}0e{}", exponent - 1)).unwrap();
                let decimal = CanonicalJson::parse(format!("{coefficient}.0e{exponent}")).unwrap();
                assert_eq!(base, shifted);
                assert_eq!(base, decimal);
            }
        }
    }

    #[test]
    fn decimal_exponent_arithmetic_handles_large_carry_and_borrow() {
        assert_eq!(
            add_signed_decimal(false, "999", 1),
            (false, "1000".to_owned())
        );
        assert_eq!(
            add_signed_decimal(true, "1000", 1),
            (true, "999".to_owned())
        );
        assert_eq!(
            add_signed_decimal(true, "999", -1),
            (true, "1000".to_owned())
        );
        assert_eq!(add_signed_decimal(false, "1", -2), (true, "1".to_owned()));
        assert_eq!(add_signed_decimal(true, "1", 2), (false, "1".to_owned()));

        let nines = "9".repeat(100);
        let power = "1".to_owned() + &"0".repeat(100);
        assert_eq!(add_decimal_magnitude(&nines, 1), power);
        assert_eq!(subtract_small_from_decimal_magnitude(&power, 1), nines);

        let huge = "9".repeat(100);
        let one = CanonicalJson::parse(format!("1e+{huge}")).unwrap();
        let shifted_exponent = subtract_small_from_decimal_magnitude(&huge, 1);
        let ten = CanonicalJson::parse(format!("10e+{shifted_exponent}")).unwrap();
        assert_eq!(one, ten);
    }

    #[test]
    fn canonical_json_rejects_duplicate_keys_at_any_depth() {
        assert!(matches!(
            CanonicalJson::parse(br#"{"x":1,"x":2}"#),
            Err(CanonicalJsonError::DuplicateKey(key)) if key == "x"
        ));
        assert!(matches!(
            CanonicalJson::parse(br#"{"nested":{"x":1,"x":2}}"#),
            Err(CanonicalJsonError::DuplicateKey(key)) if key == "x"
        ));
    }

    #[test]
    fn canonical_json_rejects_serde_number_sentinel_object_keys() {
        let number = CanonicalJson::parse("1").unwrap();
        assert_eq!(number.as_str(), "1");

        for raw in [
            r#"{"$serde_json::private::Number":"1"}"#,
            r#"{"\u0024serde_json::private::Number":"1"}"#,
            r#"{"\u0024\u0073\u0065\u0072\u0064\u0065\u005f\u006a\u0073\u006f\u006e\u003a\u003a\u0070\u0072\u0069\u0076\u0061\u0074\u0065\u003a\u003a\u004e\u0075\u006d\u0062\u0065\u0072":"1"}"#,
            r#"{"nested":{"$serde_json::private::Number":"1"}}"#,
        ] {
            assert!(matches!(
                CanonicalJson::parse(raw),
                Err(CanonicalJsonError::Invalid(reason)) if reason.contains("is reserved")
            ));
        }

        assert_eq!(
            CanonicalJson::parse(r#"{"value":"$serde_json::private::Number"}"#)
                .unwrap()
                .as_str(),
            r#"{"value":"$serde_json::private::Number"}"#
        );

        for truncated in [
            r#"{"$serde_json::private::Number":"1"#,
            r#"{"$serde_json::private::Number"#,
            r#"{"\u0024serde_json::private::Number":"1"#,
            r#"{"unterminated\"#,
        ] {
            assert!(CanonicalJson::parse(truncated).is_err());
        }
    }

    #[test]
    fn canonical_json_enforces_byte_depth_and_node_bounds() {
        let exact_bytes = format!("\"{}\"", "a".repeat(MAX_JSON_BYTES - 2));
        assert!(CanonicalJson::parse(exact_bytes).is_ok());
        assert!(matches!(
            CanonicalJson::parse(vec![b' '; MAX_JSON_BYTES + 1]),
            Err(CanonicalJsonError::TooLarge { .. })
        ));
        let exact_depth = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH - 1),
            "]".repeat(MAX_JSON_DEPTH - 1)
        );
        assert!(CanonicalJson::parse(exact_depth).is_ok());
        let too_deep = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH),
            "]".repeat(MAX_JSON_DEPTH)
        );
        assert!(matches!(
            CanonicalJson::parse(too_deep),
            Err(CanonicalJsonError::TooDeep { .. }) | Err(CanonicalJsonError::Invalid(_))
        ));
        let exact_nodes = format!("[{}0]", "0,".repeat(MAX_JSON_NODES - 2));
        assert!(CanonicalJson::parse(exact_nodes).is_ok());
        let too_many = format!("[{}]", "0,".repeat(MAX_JSON_NODES) + "0");
        assert!(matches!(
            CanonicalJson::parse(too_many),
            Err(CanonicalJsonError::TooManyNodes { .. })
        ));
        let oversized_typed = "x".repeat(MAX_JSON_BYTES);
        assert!(matches!(
            CanonicalJson::from_serialize(&oversized_typed),
            Err(CanonicalJsonError::TooLarge { .. })
        ));
    }

    #[test]
    fn typed_append_hashes_domain_and_big_endian_fields() {
        let mut log = Log::new();
        let stored = log
            .append_typed(&TaskV1 {
                title: "write tests".to_owned(),
            })
            .unwrap()
            .clone();
        assert_eq!(stored.seq, 0);
        assert_eq!(stored.kind, TaskV1::KIND);
        assert_eq!(stored.schema_version, 1);
        assert_eq!(stored.payload.as_str(), r#"{"title":"write tests"}"#);
        assert_eq!(stored.hash, event_hash(&stored));
        assert!(log.verify().is_ok());
    }

    #[test]
    fn typed_u128_payload_round_trips_through_its_exact_catalog_schema() {
        let source = BigAmount {
            value: u128::from(u64::MAX) + 2,
        };
        let mut log = Log::new();
        let stored = log.append_typed(&source).unwrap();
        assert_eq!(
            stored.payload().as_str(),
            r#"{"value":18446744073709551617}"#
        );

        let mut builder = EventCatalogBuilder::new();
        builder
            .register_current::<BigAmount, _>("big-amount/identity/1", |value| value)
            .unwrap();
        let catalog = builder.seal().unwrap();
        assert_eq!(catalog.decode(stored), Ok(source));
    }

    #[test]
    fn cursor_and_tail_are_exact_and_never_clamp() {
        let mut log = Log::new();
        log.append_typed(&TaskV1 { title: "a".into() }).unwrap();
        log.append_typed(&TaskV1 { title: "b".into() }).unwrap();
        let at_one = log.cursor_at(1).unwrap();
        assert_eq!(log.tail(&at_one).unwrap().len(), 1);
        assert_eq!(
            log.cursor_at(3),
            Err(CursorError::OutOfBounds {
                position: 3,
                len: 2
            })
        );
        let fork = LogCursor {
            position: 1,
            head_hash: [9; 32],
        };
        assert_eq!(
            log.tail(&fork),
            Err(CursorError::HeadMismatch { position: 1 })
        );
    }

    #[test]
    fn import_raw_canonicalizes_and_verifies_every_field() {
        let mut source = Log::new();
        source.append_typed(&TaskV1 { title: "a".into() }).unwrap();
        let event = source.events()[0].clone();
        let raw = RawEvent {
            seq: event.seq,
            kind: event.kind,
            schema_version: event.schema_version,
            payload_json: br#" { "title" : "a" } "#.to_vec(),
            prev_hash: event.prev_hash,
            hash: event.hash,
        };
        let imported = Log::import_raw([raw.clone()]).unwrap();
        assert_eq!(imported, source);
        let mut tampered = raw;
        tampered.schema_version = 2;
        assert_eq!(Log::import_raw([tampered]), Err(LogError::TamperedAt(0)));
    }

    #[test]
    fn import_raw_rejects_the_first_broken_link_without_consuming_the_tail() {
        let mut consumed = 0usize;
        let invalid = (0..10_000).map(|_| {
            consumed += 1;
            RawEvent {
                seq: 99,
                kind: TaskV1::KIND.to_owned(),
                schema_version: TaskV1::VERSION,
                payload_json: br#"{"title":"never parsed"}"#.to_vec(),
                prev_hash: GENESIS_HASH,
                hash: GENESIS_HASH,
            }
        });
        assert_eq!(Log::import_raw(invalid), Err(LogError::TamperedAt(0)));
        assert_eq!(consumed, 1);
    }

    #[test]
    fn verification_rejects_sequence_link_payload_and_hash_tampering() {
        let build = || {
            let mut log = Log::new();
            log.append_typed(&TaskV1 { title: "a".into() }).unwrap();
            log.append_typed(&TaskV1 { title: "b".into() }).unwrap();
            log
        };
        let mut seq = build();
        seq.events[1].seq = 7;
        assert!(matches!(seq.verify(), Err(LogError::TamperedAt(1))));
        let mut link = build();
        link.events[1].prev_hash = [4; 32];
        assert!(matches!(link.verify(), Err(LogError::TamperedAt(1))));
        let mut payload = build();
        payload.events[0].payload = CanonicalJson::parse(r#"{"title":"x"}"#).unwrap();
        assert!(matches!(payload.verify(), Err(LogError::TamperedAt(0))));
        let mut hash = build();
        hash.events[0].hash[0] ^= 1;
        assert!(matches!(hash.verify(), Err(LogError::TamperedAt(0))));
    }

    #[test]
    fn catalog_upcasts_to_typed_current_payload() {
        let mut log = Log::new();
        let event = log
            .append_typed(&TaskV1 {
                title: "old".into(),
            })
            .unwrap();
        assert_eq!(
            task_catalog().decode(event).unwrap(),
            AppEvent::Task(TaskV2 {
                title: "old".into(),
                priority: 0,
            })
        );
    }

    #[test]
    fn catalog_rejects_unknown_kind_and_version() {
        let catalog = task_catalog();
        let mut log = Log::new();
        let unknown_kind = log.append_typed(&NoteV1 { body: "x".into() }).unwrap();
        assert!(matches!(
            catalog.decode(unknown_kind),
            Err(CatalogError::UnknownKind(_))
        ));
        let mut event = unknown_kind.clone();
        event.kind = TaskV1::KIND.to_owned();
        event.schema_version = 9;
        assert!(matches!(
            catalog.decode(&event),
            Err(CatalogError::UnknownVersion { version: 9, .. })
        ));
    }

    #[test]
    fn catalog_rejects_cross_kind_skipped_duplicate_and_missing_edges() {
        let mut cross: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        assert!(matches!(
            cross.register_upcaster::<TaskV1, NoteV1, _>("bad", |_| unreachable!()),
            Err(CatalogError::CrossKindUpcast { .. })
        ));

        assert!(matches!(
            cross.register_upcaster::<TaskV1, TaskV3, _>("skip", |_| unreachable!()),
            Err(CatalogError::NonAdjacentUpcast { .. })
        ));

        let mut duplicate: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        duplicate
            .register_upcaster::<TaskV1, TaskV2, _>("one", |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: 0,
                })
            })
            .unwrap();
        assert!(matches!(
            duplicate.register_upcaster::<TaskV1, TaskV2, _>("two", |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: 1,
                })
            }),
            Err(CatalogError::DuplicateUpcaster { .. })
        ));

        let mut current_only: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        current_only
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        assert!(current_only.seal().is_ok());

        let mut no_current: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        no_current
            .register_upcaster::<TaskV1, TaskV2, _>("one", |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: 0,
                })
            })
            .unwrap();
        assert!(matches!(
            no_current.seal(),
            Err(CatalogError::MissingCurrent(_))
        ));

        let mut missing_edge: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        missing_edge
            .register_upcaster::<TaskV1, TaskV2, _>("one", |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: 0,
                })
            })
            .unwrap()
            .register_current::<TaskV3, _>("app-event/task-v3/1", |value| {
                AppEvent::Task(TaskV2 {
                    title: value.title,
                    priority: value.priority,
                })
            })
            .unwrap();
        assert!(matches!(
            missing_edge.seal(),
            Err(CatalogError::MissingUpcaster { from: 2, .. })
        ));

        let mut gap: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        gap.register_upcaster::<TaskV1, TaskV2, _>("one", |old| {
            Ok(TaskV2 {
                title: old.title,
                priority: 0,
            })
        })
        .unwrap()
        .register_current::<TaskV4, _>("app-event/task-v4/1", |value| {
            AppEvent::Task(TaskV2 {
                title: value.title,
                priority: 0,
            })
        })
        .unwrap();
        assert!(matches!(
            gap.seal(),
            Err(CatalogError::SchemaGap { version: 3, .. })
        ));

        let mut conflicting: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        conflicting
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        assert!(matches!(
            conflicting.register_current::<AlternateTaskV2, _>(
                "app-event/alternate-task-v2/1",
                |value| {
                    AppEvent::Task(TaskV2 {
                        title: value.title,
                        priority: value.priority,
                    })
                },
            ),
            Err(CatalogError::DuplicateSchema { version: 2, .. })
        ));
    }

    #[test]
    fn catalog_digest_is_registration_order_independent() {
        let build = |reverse: bool| {
            let mut builder = EventCatalogBuilder::new();
            if reverse {
                builder
                    .register_current::<NoteV1, _>("app-event/note-v1/1", AppEvent::Note)
                    .unwrap();
            }
            builder
                .register_upcaster::<TaskV1, TaskV2, _>("task-title-to-priority/1", |old| {
                    Ok(TaskV2 {
                        title: old.title,
                        priority: 0,
                    })
                })
                .unwrap()
                .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
                .unwrap();
            if !reverse {
                builder
                    .register_current::<NoteV1, _>("app-event/note-v1/1", AppEvent::Note)
                    .unwrap();
            }
            builder.seal().unwrap().schema_set_digest()
        };
        assert_eq!(build(false), build(true));
    }

    #[test]
    fn catalog_digest_binds_schema_step_and_mapping_ids() {
        fn digest(step: &str) -> Hash {
            let mut builder = EventCatalogBuilder::new();
            builder
                .register_upcaster::<TaskV1, TaskV2, _>(step, |old| {
                    Ok(TaskV2 {
                        title: old.title,
                        priority: 0,
                    })
                })
                .unwrap()
                .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
                .unwrap();
            builder.seal().unwrap().schema_set_digest()
        }
        assert_ne!(digest("step/1"), digest("step/2"));

        fn current_digest(mapping_id: &str) -> Hash {
            let mut builder = EventCatalogBuilder::new();
            builder
                .register_current::<TaskV2, _>(mapping_id, AppEvent::Task)
                .unwrap();
            builder.seal().unwrap().schema_set_digest()
        }
        assert_ne!(current_digest("mapping/1"), current_digest("mapping/2"));

        let mut invalid_mapping = EventCatalogBuilder::new();
        assert!(matches!(
            invalid_mapping.register_current::<TaskV2, _>("not portable!", AppEvent::Task),
            Err(CatalogError::InvalidMappingId(_))
        ));

        let mut builder = EventCatalogBuilder::new();
        builder
            .register_current::<AlternateTaskV2, _>("app-event/alternate-task-v2/1", |value| {
                AppEvent::Task(TaskV2 {
                    title: value.title,
                    priority: value.priority,
                })
            })
            .unwrap();
        assert_ne!(
            builder.seal().unwrap().schema_set_digest(),
            task_catalog().schema_set_digest()
        );

        let mut standard = EventCatalogBuilder::new();
        standard
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        let mut alternate = EventCatalogBuilder::new();
        alternate
            .register_current::<AlternateTaskV2, _>("app-event/alternate-task-v2/1", |value| {
                AppEvent::Task(TaskV2 {
                    title: value.title,
                    priority: value.priority,
                })
            })
            .unwrap();
        assert_ne!(
            standard.seal().unwrap().schema_set_digest(),
            alternate.seal().unwrap().schema_set_digest()
        );
    }

    #[test]
    fn upcaster_failure_and_nondeterminism_fail_closed() {
        let mut failing = EventCatalogBuilder::new();
        failing
            .register_upcaster::<TaskV1, TaskV2, _>("failure/1", |_| Err("nope".into()))
            .unwrap()
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        let failing = failing.seal().unwrap();

        let mut log = Log::new();
        let event = log
            .append_typed(&TaskV1 {
                title: "old".into(),
            })
            .unwrap();
        assert!(matches!(
            failing.decode(event),
            Err(CatalogError::UpcastFailed { .. })
        ));

        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let mut changing = EventCatalogBuilder::new();
        changing
            .register_upcaster::<TaskV1, TaskV2, _>("changing/1", move |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: observed.fetch_add(1, Ordering::SeqCst) as u8,
                })
            })
            .unwrap()
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        assert!(matches!(
            changing.seal().unwrap().decode(event),
            Err(CatalogError::NondeterministicUpcast { .. })
        ));

        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let mut paired = EventCatalogBuilder::new();
        paired
            .register_upcaster::<TaskV1, TaskV2, _>("paired/1", move |old| {
                Ok(TaskV2 {
                    title: old.title,
                    priority: (observed.fetch_add(1, Ordering::SeqCst) / 2) as u8,
                })
            })
            .unwrap()
            .register_current::<TaskV2, _>("app-event/task-v2/1", AppEvent::Task)
            .unwrap();
        let paired = paired.seal().unwrap();
        assert!(paired.decode(event).is_ok());
        assert!(matches!(
            paired.decode(event),
            Err(CatalogError::NondeterministicUpcast { .. })
        ));
    }

    #[test]
    fn current_mapper_identity_and_observed_output_fail_closed() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let mut builder = EventCatalogBuilder::new();
        builder
            .register_current::<TaskV1, _>("app-event/task-v1-dynamic/1", move |old| {
                AppEvent::Task(TaskV2 {
                    title: old.title,
                    priority: (observed.fetch_add(1, Ordering::SeqCst) / 2) as u8,
                })
            })
            .unwrap();
        let catalog = builder.seal().unwrap();

        let mut log = Log::new();
        let event = log
            .append_typed(&TaskV1 {
                title: "old".into(),
            })
            .unwrap();
        assert!(catalog.decode(event).is_ok());
        assert!(matches!(
            catalog.decode(event),
            Err(CatalogError::NondeterministicMapping { .. })
        ));
    }

    #[test]
    fn schema_constants_fail_closed() {
        #[derive(Serialize, Deserialize)]
        struct Bad;
        impl EventSchema for Bad {
            const KIND: &'static str = "task";
            const VERSION: u32 = 0;
            const SCHEMA_ID: &'static str = "";
        }
        let mut log = Log::new();
        assert!(matches!(
            log.append_typed(&Bad),
            Err(LogError::InvalidKind(_))
        ));
        let mut catalog: EventCatalogBuilder<AppEvent> = EventCatalogBuilder::new();
        assert!(matches!(
            catalog.register_current::<Bad, _>("bad/1", |_| unreachable!()),
            Err(CatalogError::InvalidSchema(LogError::InvalidKind(_)))
        ));

        #[derive(Serialize, Deserialize)]
        struct ExtremeVersion;
        impl EventSchema for ExtremeVersion {
            const KIND: &'static str = "app_extreme";
            const VERSION: u32 = u32::MAX;
            const SCHEMA_ID: &'static str = "pliego.example/extreme/max";
        }
        assert!(matches!(
            log.append_typed(&ExtremeVersion),
            Err(LogError::InvalidSchemaVersion)
        ));
        assert!(matches!(
            catalog.register_current::<ExtremeVersion, _>("extreme/1", |_| unreachable!()),
            Err(CatalogError::InvalidSchema(LogError::InvalidSchemaVersion))
        ));
    }

    #[test]
    fn golden_hash_and_catalog_vectors_are_stable() {
        let mut log = Log::new();
        log.append_typed(&TaskV1 {
            title: "gold".into(),
        })
        .unwrap();
        assert_eq!(
            hex(&log.head()),
            "215aa537508e444844a58eb9fbf684cce6010143c85ccf1ac42a55c717d1f784"
        );
        assert_eq!(
            hex(&task_catalog().schema_set_digest()),
            "88dd035aa6c68b2af282d43d2500b0d1d40d6fcdcc7bf3d986dedc9f462aca18"
        );
    }

    #[test]
    fn sealed_catalog_is_send_sync_and_has_no_mutation_api() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SealedEventCatalog<AppEvent>>();
        assert!(task_catalog().supports(TaskV1::KIND, TaskV1::VERSION));
        assert!(task_catalog().supports(TaskV2::KIND, TaskV2::VERSION));
        assert!(!task_catalog().supports(TaskV2::KIND, 0));
    }
}
