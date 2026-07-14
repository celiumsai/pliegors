// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! A versioned client-side sync contract between PliegoRS and Hyphae.
//!
//! This crate defines and validates envelopes, idempotent append batches,
//! durable receipts, pull pages, and deterministic replay. It deliberately does
//! **not** provide credentials, tenant resolution, a production gateway, or a
//! Hyphae service implementation. A valid value proves only that it satisfies
//! this wire contract; receipt authenticity still requires a trusted verifier
//! and a production transport.
//!
//! [`JournalTransport`] and [`push_pending`] remain as the experimental M5
//! compatibility seam. New integrations should use [`BatchTransport`],
//! [`append_with_retry`], [`PullRequest`], and [`apply_pull_page`].

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use pliego_log::{Event, Log, hex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Exact wire protocol accepted by this client contract.
pub const PROTOCOL_V1: &str = "pliego-hyphae/1";
/// Namespace accepted by Hyphae for application-owned journal events.
pub const APP_KIND_PREFIX: &str = "app_";
/// Maximum number of events in one append batch.
pub const MAX_BATCH_EVENTS: usize = 128;
/// Maximum serialized payload size for one event.
pub const MAX_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;
/// Maximum aggregate payload size for one batch.
pub const MAX_BATCH_PAYLOAD_BYTES: usize = 512 * 1024;
/// Maximum causal parents carried by one event.
pub const MAX_CAUSAL_PARENTS: usize = 16;
/// Maximum events requested or returned by a pull page.
pub const MAX_PULL_EVENTS: u16 = 256;
/// Defensive ceiling for the legacy in-memory acknowledgement vector.
pub const MAX_LEGACY_ACKS: usize = 1_000_000;
/// Canonical head hash for a stream that has accepted no events.
pub const EMPTY_STREAM_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// A validation failure localized to one protocol field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    field: &'static str,
    reason: &'static str,
}

impl ValidationError {
    const fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }

    /// Name of the rejected field.
    #[must_use]
    pub const fn field(&self) -> &'static str {
        self.field
    }

    /// Stable, non-sensitive explanation suitable for diagnostics.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {}", self.field, self.reason)
    }
}

impl Error for ValidationError {}

fn ensure_protocol(protocol: &str) -> Result<(), ValidationError> {
    if protocol == PROTOCOL_V1 {
        Ok(())
    } else {
        Err(ValidationError::new("protocol", "unsupported version"))
    }
}

fn is_hex_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Validate a canonical lowercase SHA-256 hex string.
pub fn validate_hash(value: &str) -> Result<(), ValidationError> {
    if is_hex_hash(value) {
        Ok(())
    } else {
        Err(ValidationError::new(
            "hash",
            "expected 64 lowercase hexadecimal characters",
        ))
    }
}

fn validate_uuid_v7(field: &'static str, value: &str) -> Result<(), ValidationError> {
    let bytes = value.as_bytes();
    let hyphens = [8, 13, 18, 23];
    let shape = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if hyphens.contains(&index) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        });
    if !shape || bytes[14] != b'7' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {
        return Err(ValidationError::new(
            field,
            "expected canonical lowercase UUIDv7",
        ));
    }
    Ok(())
}

/// Validate a client event identifier.
pub fn validate_client_event_id(value: &str) -> Result<(), ValidationError> {
    validate_uuid_v7("client_event_id", value)
}

/// Validate an idempotency batch identifier.
pub fn validate_batch_id(value: &str) -> Result<(), ValidationError> {
    validate_uuid_v7("batch_id", value)
}

/// Validate a bounded application stream identifier.
pub fn validate_stream_id(value: &str) -> Result<(), ValidationError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
        && !value.contains("..")
        && !value.starts_with('/')
        && !value.ends_with('/');
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "stream_id",
            "expected 1..128 safe ASCII characters without traversal segments",
        ))
    }
}

/// Validate an application-owned event kind.
pub fn validate_kind(value: &str) -> Result<(), ValidationError> {
    let suffix = value.strip_prefix(APP_KIND_PREFIX).unwrap_or_default();
    let valid = !suffix.is_empty()
        && value.len() <= 96
        && !suffix.starts_with('_')
        && !suffix.ends_with('_')
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "kind",
            "expected app_ followed by lowercase ASCII letters, digits, or underscores",
        ))
    }
}

fn validate_timestamp(value: &str) -> Result<(), ValidationError> {
    let bytes = value.as_bytes();
    let shape = (20..=64).contains(&bytes.len())
        && value.is_ascii()
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && matches!(bytes.get(10), Some(b'T' | b't'))
        && bytes.get(13) == Some(&b':')
        && bytes.get(16) == Some(&b':');
    if !shape {
        return Err(ValidationError::new(
            "timestamp",
            "expected a valid bounded RFC3339 timestamp",
        ));
    }

    let parse_digits = |start: usize, length: usize| -> Option<u32> {
        let mut value = 0_u32;
        for byte in bytes.get(start..start.checked_add(length)?)? {
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
        }
        Some(value)
    };
    let Some(year) = parse_digits(0, 4) else {
        return Err(ValidationError::new("timestamp", "invalid RFC3339 year"));
    };
    let Some(month) = parse_digits(5, 2) else {
        return Err(ValidationError::new("timestamp", "invalid RFC3339 month"));
    };
    let Some(day) = parse_digits(8, 2) else {
        return Err(ValidationError::new("timestamp", "invalid RFC3339 day"));
    };
    let Some(hour) = parse_digits(11, 2) else {
        return Err(ValidationError::new("timestamp", "invalid RFC3339 hour"));
    };
    let Some(minute) = parse_digits(14, 2) else {
        return Err(ValidationError::new("timestamp", "invalid RFC3339 minute"));
    };
    let Some(second) = parse_digits(17, 2) else {
        return Err(ValidationError::new("timestamp", "invalid RFC3339 second"));
    };
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => 0,
    };
    let plausible_leap_second = second == 60
        && hour == 23
        && minute == 59
        && ((month == 6 && day == 30) || (month == 12 && day == 31));
    if day == 0
        || day > max_day
        || hour > 23
        || minute > 59
        || (second > 59 && !plausible_leap_second)
    {
        return Err(ValidationError::new(
            "timestamp",
            "RFC3339 date or time component is out of range",
        ));
    }

    let mut zone = 19;
    if bytes.get(zone) == Some(&b'.') {
        zone += 1;
        let fraction_start = zone;
        while bytes.get(zone).is_some_and(u8::is_ascii_digit) {
            zone += 1;
        }
        if zone == fraction_start {
            return Err(ValidationError::new(
                "timestamp",
                "RFC3339 fractional seconds cannot be empty",
            ));
        }
    }
    let valid_zone = match bytes.get(zone) {
        Some(b'Z' | b'z') => zone + 1 == bytes.len(),
        Some(b'+' | b'-') => {
            zone + 6 == bytes.len()
                && bytes.get(zone + 3) == Some(&b':')
                && parse_digits(zone + 1, 2).is_some_and(|value| value <= 23)
                && parse_digits(zone + 4, 2).is_some_and(|value| value <= 59)
        }
        _ => false,
    };
    if valid_zone {
        Ok(())
    } else {
        Err(ValidationError::new(
            "timestamp",
            "expected a valid bounded RFC3339 timestamp",
        ))
    }
}

fn validate_signature(value: &str) -> Result<(), ValidationError> {
    let core = value.trim_end_matches('=');
    let padding = value.len() - core.len();
    let valid = (16..=512).contains(&value.len())
        && padding <= 2
        && core.len() % 4 != 1
        && core
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "signature",
            "expected bounded base64url data",
        ))
    }
}

fn validate_key_id(value: &str) -> Result<(), ValidationError> {
    let valid = (1..=96).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "key_id",
            "expected a safe ASCII token",
        ))
    }
}

/// Convert a local event kind into the durable Hyphae application namespace.
/// Already-namespaced kinds are preserved so adapters cannot double-prefix them.
#[must_use]
pub fn durable_kind(local_kind: &str) -> String {
    if local_kind.starts_with(APP_KIND_PREFIX) {
        local_kind.to_owned()
    } else {
        format!("{APP_KIND_PREFIX}{local_kind}")
    }
}

/// A position and head hash in one isolated stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamCursor {
    /// Number of accepted events visible at this position.
    pub position: u64,
    /// Hash at the stream head, or 64 zeroes for an empty stream.
    pub head_hash: String,
}

impl StreamCursor {
    /// Cursor for a stream that has accepted no events.
    #[must_use]
    pub fn genesis() -> Self {
        Self {
            position: 0,
            head_hash: EMPTY_STREAM_HASH.to_owned(),
        }
    }

    /// Validate the cursor shape.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_hash(&self.head_hash)?;
        if (self.position == 0) != (self.head_hash == EMPTY_STREAM_HASH) {
            return Err(ValidationError::new(
                "cursor",
                "position zero and the empty-stream hash must occur together",
            ));
        }
        Ok(())
    }
}

/// One immutable client event prepared for durable append.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    /// Wire protocol version.
    pub protocol: String,
    /// Stable UUIDv7 used for deduplication.
    pub client_event_id: String,
    /// Application stream; tenant and actor are intentionally absent.
    pub stream_id: String,
    /// Payload schema version consumed by the reducer.
    pub schema_version: u32,
    /// Application-owned `app_*` event kind.
    pub kind: String,
    /// A serialized JSON value. It is opaque to this crate.
    pub payload: String,
    /// Position on the local PliegoRS chain.
    pub local_seq: u64,
    /// Previous local SHA-256 hash.
    pub local_prev_hash: String,
    /// Current local SHA-256 hash.
    pub local_hash: String,
    /// Optional causal UUIDv7 parents.
    pub causal_parents: Vec<String>,
    /// Client-observed RFC3339 timestamp; never authoritative server time.
    pub created_at: String,
}

impl EventEnvelope {
    /// Create a validated envelope from a PliegoRS local event.
    pub fn from_local_event(
        event: &Event,
        client_event_id: impl Into<String>,
        stream_id: impl Into<String>,
        schema_version: u32,
        created_at: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let envelope = Self {
            protocol: PROTOCOL_V1.to_owned(),
            client_event_id: client_event_id.into(),
            stream_id: stream_id.into(),
            schema_version,
            kind: durable_kind(&event.kind),
            payload: serde_json::to_string(&event.payload)
                .map_err(|_| ValidationError::new("payload", "could not encode JSON"))?,
            local_seq: event.seq,
            local_prev_hash: hex(&event.prev_hash),
            local_hash: hex(&event.hash),
            causal_parents: Vec::new(),
            created_at: created_at.into(),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    /// Validate bounds, identifiers, namespace, hashes, and JSON syntax.
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure_protocol(&self.protocol)?;
        validate_client_event_id(&self.client_event_id)?;
        validate_stream_id(&self.stream_id)?;
        if self.schema_version == 0 {
            return Err(ValidationError::new(
                "schema_version",
                "must be greater than zero",
            ));
        }
        validate_kind(&self.kind)?;
        if self.payload.len() > MAX_EVENT_PAYLOAD_BYTES {
            return Err(ValidationError::new("payload", "event limit exceeded"));
        }
        serde_json::from_str::<serde_json::Value>(&self.payload)
            .map_err(|_| ValidationError::new("payload", "expected valid JSON"))?;
        validate_hash(&self.local_prev_hash)?;
        validate_hash(&self.local_hash)?;
        if self.causal_parents.len() > MAX_CAUSAL_PARENTS {
            return Err(ValidationError::new(
                "causal_parents",
                "parent limit exceeded",
            ));
        }
        let mut parents = BTreeSet::new();
        for parent in &self.causal_parents {
            validate_client_event_id(parent)?;
            if parent == &self.client_event_id || !parents.insert(parent) {
                return Err(ValidationError::new(
                    "causal_parents",
                    "self-reference or duplicate parent",
                ));
            }
        }
        validate_timestamp(&self.created_at)
    }

    /// SHA-256 over the canonical validated envelope, used to bind a receipt
    /// to the exact kind, payload, local chain, and causal metadata accepted.
    pub fn wire_hash(&self) -> Result<String, ValidationError> {
        self.validate()?;
        let mut hasher = Sha256::new();
        for field in [
            self.protocol.as_str(),
            self.client_event_id.as_str(),
            self.stream_id.as_str(),
        ] {
            hash_field(&mut hasher, field)?;
        }
        hasher.update(self.schema_version.to_be_bytes());
        for field in [self.kind.as_str(), self.payload.as_str()] {
            hash_field(&mut hasher, field)?;
        }
        hasher.update(self.local_seq.to_be_bytes());
        for field in [self.local_prev_hash.as_str(), self.local_hash.as_str()] {
            hash_field(&mut hasher, field)?;
        }
        let parent_count = u32::try_from(self.causal_parents.len())
            .map_err(|_| ValidationError::new("causal_parents", "count overflow"))?;
        hasher.update(parent_count.to_be_bytes());
        for parent in &self.causal_parents {
            hash_field(&mut hasher, parent)?;
        }
        hash_field(&mut hasher, &self.created_at)?;
        let digest: [u8; 32] = hasher.finalize().into();
        Ok(hex(&digest))
    }
}

fn hash_field(hasher: &mut Sha256, value: &str) -> Result<(), ValidationError> {
    let length = u32::try_from(value.len())
        .map_err(|_| ValidationError::new("envelope", "field length overflow"))?;
    hasher.update(length.to_be_bytes());
    hasher.update(value.as_bytes());
    Ok(())
}

/// Idempotent unit of append and retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendBatch {
    /// Wire protocol version.
    pub protocol: String,
    /// Stable UUIDv7 reused byte-for-byte for every retry.
    pub batch_id: String,
    /// Stream shared by every envelope in this batch.
    pub stream_id: String,
    /// Last observed durable cursor; `None` is the genesis precondition.
    /// Blind appends are intentionally not representable in protocol v1.
    pub expected_cursor: Option<StreamCursor>,
    /// Ordered, contiguous local events.
    pub events: Vec<EventEnvelope>,
}

impl AppendBatch {
    /// Validate batch bounds, identities, ordering, and local chain links.
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure_protocol(&self.protocol)?;
        validate_batch_id(&self.batch_id)?;
        validate_stream_id(&self.stream_id)?;
        if let Some(cursor) = &self.expected_cursor {
            cursor.validate()?;
        }
        if self.events.is_empty() || self.events.len() > MAX_BATCH_EVENTS {
            return Err(ValidationError::new(
                "events",
                "batch must contain 1..128 events",
            ));
        }
        let mut total_payload = 0usize;
        let mut ids = BTreeSet::new();
        let mut previous: Option<&EventEnvelope> = None;
        for event in &self.events {
            event.validate()?;
            if event.stream_id != self.stream_id {
                return Err(ValidationError::new(
                    "stream_id",
                    "event and batch streams differ",
                ));
            }
            if !ids.insert(&event.client_event_id) {
                return Err(ValidationError::new(
                    "client_event_id",
                    "duplicate inside batch",
                ));
            }
            total_payload = total_payload
                .checked_add(event.payload.len())
                .ok_or(ValidationError::new("payload", "batch size overflow"))?;
            if total_payload > MAX_BATCH_PAYLOAD_BYTES {
                return Err(ValidationError::new(
                    "payload",
                    "aggregate batch limit exceeded",
                ));
            }
            if let Some(prior) = previous {
                let expected_seq = prior.local_seq.checked_add(1).ok_or(ValidationError::new(
                    "local_seq",
                    "sequence number overflow",
                ))?;
                if event.local_seq != expected_seq || event.local_prev_hash != prior.local_hash {
                    return Err(ValidationError::new(
                        "events",
                        "local sequence or hash link is not contiguous",
                    ));
                }
            }
            previous = Some(event);
        }
        Ok(())
    }
}

/// A server receipt for one accepted client event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Client identity echoed by the server.
    pub client_event_id: String,
    /// Resolved stream identity.
    pub stream_id: String,
    /// Canonical hash of the exact accepted [`EventEnvelope`].
    pub envelope_hash: String,
    /// Sequence on the durable journal.
    pub server_seq: u64,
    /// Hash of the accepted durable entry.
    pub server_hash: String,
    /// Previous durable hash.
    pub server_prev_hash: String,
    /// Durable journal head after the append transaction.
    pub journal_head: String,
    /// Authoritative server RFC3339 timestamp.
    pub committed_at: String,
    /// Identifier of the signing key.
    pub key_id: String,
    /// Detached base64url signature. Shape validation is not verification.
    pub signature: String,
}

impl Receipt {
    /// Validate the receipt's untrusted wire shape.
    pub fn validate_shape(&self) -> Result<(), ValidationError> {
        validate_client_event_id(&self.client_event_id)?;
        validate_stream_id(&self.stream_id)?;
        validate_hash(&self.envelope_hash)?;
        validate_hash(&self.server_hash)?;
        validate_hash(&self.server_prev_hash)?;
        validate_hash(&self.journal_head)?;
        validate_timestamp(&self.committed_at)?;
        validate_key_id(&self.key_id)?;
        validate_signature(&self.signature)
    }

    /// Canonical length-prefixed bytes covered by a receipt signature.
    ///
    /// The signature itself is excluded. Every string is encoded as a
    /// big-endian `u32` byte length followed by UTF-8 bytes; `server_seq` is a
    /// big-endian `u64`. Call [`Self::validate_shape`] before trusting this
    /// payload or allocating verifier state.
    pub fn signing_payload(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate_shape()?;
        let leading_fields = [
            PROTOCOL_V1,
            self.client_event_id.as_str(),
            self.stream_id.as_str(),
            self.envelope_hash.as_str(),
        ];
        let trailing_fields = [
            self.server_hash.as_str(),
            self.server_prev_hash.as_str(),
            self.journal_head.as_str(),
            self.committed_at.as_str(),
            self.key_id.as_str(),
        ];
        let capacity = leading_fields
            .iter()
            .chain(&trailing_fields)
            .map(|field| field.len() + 4)
            .sum::<usize>()
            + 8;
        let mut payload = Vec::with_capacity(capacity);
        for field in leading_fields {
            let length = u32::try_from(field.len())
                .map_err(|_| ValidationError::new("receipt", "field length overflow"))?;
            payload.extend_from_slice(&length.to_be_bytes());
            payload.extend_from_slice(field.as_bytes());
        }
        payload.extend_from_slice(&self.server_seq.to_be_bytes());
        for field in trailing_fields {
            let length = u32::try_from(field.len())
                .map_err(|_| ValidationError::new("receipt", "field length overflow"))?;
            payload.extend_from_slice(&length.to_be_bytes());
            payload.extend_from_slice(field.as_bytes());
        }
        Ok(payload)
    }

    /// Lossy compatibility view used by the legacy one-event seam.
    #[must_use]
    pub fn legacy_ack(&self) -> Ack {
        Ack {
            seq: self.server_seq,
            hash: self.server_hash.clone(),
        }
    }
}

/// Cryptographic boundary for verifying receipts against trusted Hyphae keys.
/// Implementations should pin or securely rotate keys outside this crate.
pub trait ReceiptVerifier {
    /// Verify a detached base64url signature over the canonical payload.
    fn verify(&self, key_id: &str, signing_payload: &[u8], signature: &str)
    -> Result<bool, String>;
}

/// A receipt failed shape, key lookup, or signature verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptVerificationError {
    /// Untrusted receipt shape was invalid.
    Validation(ValidationError),
    /// The verifier could not resolve or use the trusted key.
    Verifier(String),
    /// Cryptographic verification returned false.
    InvalidSignature,
}

impl fmt::Display for ReceiptVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(f),
            Self::Verifier(message) => write!(f, "receipt verifier failed: {message}"),
            Self::InvalidSignature => f.write_str("invalid receipt signature"),
        }
    }
}

impl Error for ReceiptVerificationError {}

/// Verify one typed receipt. Shape validation alone never implies durability.
pub fn verify_receipt<V: ReceiptVerifier>(
    receipt: &Receipt,
    verifier: &V,
) -> Result<(), ReceiptVerificationError> {
    let payload = receipt
        .signing_payload()
        .map_err(ReceiptVerificationError::Validation)?;
    match verifier
        .verify(&receipt.key_id, &payload, &receipt.signature)
        .map_err(ReceiptVerificationError::Verifier)?
    {
        true => Ok(()),
        false => Err(ReceiptVerificationError::InvalidSignature),
    }
}

/// Response for an accepted or deduplicated append batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendResponse {
    /// Wire protocol version.
    pub protocol: String,
    /// Batch id echoed exactly.
    pub batch_id: String,
    /// Stream id echoed exactly.
    pub stream_id: String,
    /// Original receipts, in event order.
    pub receipts: Vec<Receipt>,
    /// Cursor after the complete atomic batch.
    pub next_cursor: StreamCursor,
}

impl AppendResponse {
    /// Validate that this untrusted response corresponds exactly to `batch`.
    pub fn validate_against(&self, batch: &AppendBatch) -> Result<(), ValidationError> {
        ensure_protocol(&self.protocol)?;
        if self.batch_id != batch.batch_id {
            return Err(ValidationError::new("batch_id", "response mismatch"));
        }
        if self.stream_id != batch.stream_id {
            return Err(ValidationError::new("stream_id", "response mismatch"));
        }
        if self.receipts.len() != batch.events.len() {
            return Err(ValidationError::new(
                "receipts",
                "response cardinality mismatch",
            ));
        }
        self.next_cursor.validate()?;
        let mut previous: Option<&Receipt> = None;
        for (event, receipt) in batch.events.iter().zip(&self.receipts) {
            receipt.validate_shape()?;
            if receipt.client_event_id != event.client_event_id
                || receipt.stream_id != batch.stream_id
                || receipt.envelope_hash != event.wire_hash()?
            {
                return Err(ValidationError::new(
                    "receipts",
                    "event identity or stream mismatch",
                ));
            }
            if let Some(prior) = previous {
                if receipt.server_seq <= prior.server_seq
                    || receipt.server_prev_hash != prior.server_hash
                {
                    return Err(ValidationError::new(
                        "receipts",
                        "durable order or hash link mismatch",
                    ));
                }
            } else {
                let expected_head = batch
                    .expected_cursor
                    .as_ref()
                    .map_or(EMPTY_STREAM_HASH, |cursor| cursor.head_hash.as_str());
                if receipt.server_prev_hash != expected_head {
                    return Err(ValidationError::new(
                        "receipts",
                        "first receipt does not extend expected cursor",
                    ));
                }
            }
            previous = Some(receipt);
        }
        let last = self
            .receipts
            .last()
            .ok_or(ValidationError::new("receipts", "empty response"))?;
        if self.next_cursor.head_hash != last.journal_head {
            return Err(ValidationError::new(
                "next_cursor",
                "head does not match final receipt",
            ));
        }
        if last.server_hash != last.journal_head {
            return Err(ValidationError::new(
                "receipts",
                "final durable entry is not the journal head",
            ));
        }
        if self
            .receipts
            .iter()
            .any(|receipt| receipt.journal_head != self.next_cursor.head_hash)
        {
            return Err(ValidationError::new(
                "receipts",
                "atomic batch receipts disagree on journal head",
            ));
        }
        let start_position = batch
            .expected_cursor
            .as_ref()
            .map_or(0, |cursor| cursor.position);
        let appended = u64::try_from(batch.events.len())
            .map_err(|_| ValidationError::new("events", "count overflow"))?;
        let next_position = start_position
            .checked_add(appended)
            .ok_or(ValidationError::new("next_cursor", "position overflow"))?;
        if self.next_cursor.position != next_position {
            return Err(ValidationError::new(
                "next_cursor",
                "position does not match atomic append",
            ));
        }
        Ok(())
    }

    /// Verify every receipt with a trusted external key implementation.
    pub fn verify_receipts<V: ReceiptVerifier>(
        &self,
        verifier: &V,
    ) -> Result<(), ReceiptVerificationError> {
        for receipt in &self.receipts {
            verify_receipt(receipt, verifier)?;
        }
        Ok(())
    }
}

/// Transport failures are classified so conflicts and rejections are never
/// retried as transient network failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Timeout, disconnect, 429, or retryable server failure.
    Retryable(String),
    /// The stream no longer has the client's expected cursor.
    CursorConflict {
        /// Cursor supplied by the client.
        expected: Option<StreamCursor>,
        /// Current cursor disclosed for this authorized stream.
        actual: StreamCursor,
    },
    /// Permanent authenticated rejection with a non-sensitive message.
    Rejected(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Retryable(message) => write!(f, "retryable transport error: {message}"),
            Self::CursorConflict { .. } => f.write_str("stream cursor conflict"),
            Self::Rejected(message) => write!(f, "append rejected: {message}"),
        }
    }
}

impl Error for TransportError {}

/// Versioned append transport. Implementations own authentication and I/O.
pub trait BatchTransport {
    /// Append an idempotent batch. A retry must send the identical value.
    fn append_batch(&mut self, batch: &AppendBatch) -> Result<AppendResponse, TransportError>;
}

/// Versioned pull transport. Append-only workers do not need to expose it.
pub trait PullTransport {
    /// Pull an ordered recovery page.
    fn pull_page(&mut self, request: &PullRequest) -> Result<PullPage, TransportError>;
}

/// Marker for a transport supporting the complete sync protocol.
pub trait SyncTransport: BatchTransport + PullTransport {}

impl<T: BatchTransport + PullTransport> SyncTransport for T {}

/// Failure returned by the bounded retry driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// Local request or untrusted response failed contract validation.
    Validation(ValidationError),
    /// Transport failed or rejected the operation.
    Transport(TransportError),
    /// Every allowed transient attempt failed.
    AttemptsExhausted,
    /// The application reducer rejected an otherwise valid recovered event.
    Reducer(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
            Self::AttemptsExhausted => f.write_str("append retry attempts exhausted"),
            Self::Reducer(message) => write!(f, "replay reducer failed: {message}"),
        }
    }
}

impl Error for SyncError {}

/// Retry the same immutable batch after transient failures, including a lost
/// acknowledgement. Scheduling and backoff remain the caller's responsibility.
pub fn append_with_retry<T: BatchTransport>(
    transport: &mut T,
    batch: &AppendBatch,
    max_attempts: u8,
) -> Result<AppendResponse, SyncError> {
    batch.validate().map_err(SyncError::Validation)?;
    if max_attempts == 0 {
        return Err(SyncError::AttemptsExhausted);
    }
    for _ in 0..max_attempts {
        match transport.append_batch(batch) {
            Ok(response) => {
                response
                    .validate_against(batch)
                    .map_err(SyncError::Validation)?;
                return Ok(response);
            }
            Err(TransportError::Retryable(_)) => {}
            Err(error) => return Err(SyncError::Transport(error)),
        }
    }
    Err(SyncError::AttemptsExhausted)
}

/// Request for one bounded pull page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequest {
    /// Wire protocol version.
    pub protocol: String,
    /// Authorized application stream.
    pub stream_id: String,
    /// Cursor after which events are requested.
    pub after: Option<StreamCursor>,
    /// Bounded page size.
    pub limit: u16,
}

impl PullRequest {
    /// Validate request bounds and identities.
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure_protocol(&self.protocol)?;
        validate_stream_id(&self.stream_id)?;
        if let Some(cursor) = &self.after {
            cursor.validate()?;
        }
        if self.limit == 0 || self.limit > MAX_PULL_EVENTS {
            return Err(ValidationError::new("limit", "must be in 1..=256"));
        }
        Ok(())
    }
}

/// One envelope paired with its durable receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedEvent {
    /// Original accepted client envelope.
    pub envelope: EventEnvelope,
    /// Durable server receipt.
    pub receipt: Receipt,
}

/// Ordered response page for pull, recovery, and replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullPage {
    /// Wire protocol version.
    pub protocol: String,
    /// Stream shared by all returned records.
    pub stream_id: String,
    /// Ordered accepted events after the requested cursor.
    pub events: Vec<AcceptedEvent>,
    /// Cursor immediately after this page.
    pub next_cursor: StreamCursor,
    /// `true` only when the authorized stream has no further records.
    pub complete: bool,
}

impl PullPage {
    /// Validate an untrusted page against its request.
    pub fn validate_against(&self, request: &PullRequest) -> Result<(), ValidationError> {
        request.validate()?;
        ensure_protocol(&self.protocol)?;
        if self.stream_id != request.stream_id {
            return Err(ValidationError::new("stream_id", "pull response mismatch"));
        }
        if self.events.len() > usize::from(request.limit) {
            return Err(ValidationError::new("events", "pull limit exceeded"));
        }
        self.next_cursor.validate()?;
        let mut ids = BTreeSet::new();
        let mut previous_hash = Some(
            request
                .after
                .as_ref()
                .map_or(EMPTY_STREAM_HASH, |cursor| cursor.head_hash.as_str()),
        );
        let mut previous_seq = None;
        for accepted in &self.events {
            accepted.envelope.validate()?;
            accepted.receipt.validate_shape()?;
            if accepted.envelope.stream_id != self.stream_id
                || accepted.receipt.stream_id != self.stream_id
                || accepted.envelope.client_event_id != accepted.receipt.client_event_id
                || accepted.receipt.envelope_hash != accepted.envelope.wire_hash()?
            {
                return Err(ValidationError::new(
                    "events",
                    "pull identity or stream mismatch",
                ));
            }
            if !ids.insert(&accepted.envelope.client_event_id) {
                return Err(ValidationError::new(
                    "client_event_id",
                    "duplicate inside pull page",
                ));
            }
            if previous_hash.is_some_and(|hash| hash != accepted.receipt.server_prev_hash) {
                return Err(ValidationError::new(
                    "receipts",
                    "pull hash chain is discontinuous",
                ));
            }
            if previous_seq.is_some_and(|seq| accepted.receipt.server_seq <= seq) {
                return Err(ValidationError::new(
                    "receipts",
                    "pull sequence is not increasing",
                ));
            }
            previous_hash = Some(&accepted.receipt.server_hash);
            previous_seq = Some(accepted.receipt.server_seq);
        }
        if let Some(last) = self.events.last() {
            if self.next_cursor.head_hash != last.receipt.server_hash {
                return Err(ValidationError::new(
                    "next_cursor",
                    "head does not match final durable entry",
                ));
            }
        }
        let start = request.after.as_ref().map_or(0, |cursor| cursor.position);
        let count = u64::try_from(self.events.len())
            .map_err(|_| ValidationError::new("events", "count overflow"))?;
        let expected_position = start
            .checked_add(count)
            .ok_or(ValidationError::new("next_cursor", "position overflow"))?;
        if self.next_cursor.position != expected_position {
            return Err(ValidationError::new(
                "next_cursor",
                "position does not match returned page",
            ));
        }
        if !self.complete && self.events.is_empty() {
            return Err(ValidationError::new(
                "complete",
                "an incomplete page must advance",
            ));
        }
        Ok(())
    }
}

/// Persistent replay bookkeeping used to deduplicate overlapping pull pages.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReplayState {
    cursor: Option<StreamCursor>,
    recent: BTreeMap<String, String>,
}

impl ReplayState {
    /// Last validated durable cursor.
    #[must_use]
    pub fn cursor(&self) -> Option<&StreamCursor> {
        self.cursor.as_ref()
    }

    /// Number of entries retained in the bounded overlap-deduplication window.
    #[must_use]
    pub fn dedupe_window_len(&self) -> usize {
        self.recent.len()
    }
}

/// Application reducer boundary for recovered accepted events.
pub trait ReplaySink {
    /// Atomically apply one bounded page of previously unseen accepted events.
    /// Returning an error must leave application state unchanged.
    fn apply_batch(&mut self, events: &[AcceptedEvent]) -> Result<(), String>;
}

/// Validate and apply a pull page. Repeated event IDs with the same durable hash
/// are ignored; an ID reused with another hash is rejected as equivocation.
pub fn apply_pull_page<S: ReplaySink>(
    state: &mut ReplayState,
    request: &PullRequest,
    page: &PullPage,
    sink: &mut S,
) -> Result<u64, SyncError> {
    page.validate_against(request)
        .map_err(SyncError::Validation)?;
    let current_position = state.cursor.as_ref().map_or(0, |cursor| cursor.position);
    let start_position = request.after.as_ref().map_or(0, |cursor| cursor.position);
    let mut fresh = Vec::new();
    for (index, accepted) in page.events.iter().enumerate() {
        let offset = u64::try_from(index)
            .map_err(|_| SyncError::Validation(ValidationError::new("events", "index overflow")))?;
        let absolute_position = start_position
            .checked_add(offset)
            .and_then(|position| position.checked_add(1))
            .ok_or_else(|| {
                SyncError::Validation(ValidationError::new("events", "position overflow"))
            })?;
        if absolute_position <= current_position {
            match state.recent.get(&accepted.envelope.client_event_id) {
                Some(hash) if hash == &accepted.receipt.server_hash => continue,
                Some(_) => {
                    return Err(SyncError::Validation(ValidationError::new(
                        "client_event_id",
                        "durable hash changed during replay",
                    )));
                }
                None => {
                    return Err(SyncError::Validation(ValidationError::new(
                        "events",
                        "stale replay falls outside the bounded dedupe window",
                    )));
                }
            }
        }
        if state
            .recent
            .get(&accepted.envelope.client_event_id)
            .is_some_and(|hash| hash != &accepted.receipt.server_hash)
        {
            return Err(SyncError::Validation(ValidationError::new(
                "client_event_id",
                "durable hash changed during replay",
            )));
        }
        if state
            .recent
            .contains_key(&accepted.envelope.client_event_id)
        {
            return Err(SyncError::Validation(ValidationError::new(
                "client_event_id",
                "event identity moved to another stream position",
            )));
        }
        fresh.push(accepted.clone());
    }
    sink.apply_batch(&fresh).map_err(SyncError::Reducer)?;
    let applied = u64::try_from(fresh.len())
        .map_err(|_| SyncError::Validation(ValidationError::new("events", "count overflow")))?;
    match &state.cursor {
        Some(current) if current.position > page.next_cursor.position => {}
        Some(current)
            if current.position == page.next_cursor.position
                && current.head_hash != page.next_cursor.head_hash =>
        {
            return Err(SyncError::Validation(ValidationError::new(
                "next_cursor",
                "same position changed durable head",
            )));
        }
        _ => {
            state.cursor = Some(page.next_cursor.clone());
            if !page.events.is_empty() {
                state.recent = page
                    .events
                    .iter()
                    .map(|accepted| {
                        (
                            accepted.envelope.client_event_id.clone(),
                            accepted.receipt.server_hash.clone(),
                        )
                    })
                    .collect();
            }
        }
    }
    Ok(applied)
}

// ───────────────────────── legacy compatibility seam ─────────────────────────

/// A legacy server acknowledgment: where one event landed on a durable chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    /// Sequence on the Hyphae journal.
    pub seq: u64,
    /// Lowercase SHA-256 hex hash of the durable entry.
    pub hash: String,
}

impl Ack {
    /// Validate the acknowledgement's untrusted shape.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_hash(&self.hash)
    }
}

/// Tracks which local events have legacy durable acknowledgments.
#[derive(Debug, Default)]
pub struct SyncState {
    acks: Vec<Option<Ack>>, // indexed by local seq
    next_to_push: u64,
}

impl SyncState {
    /// Empty synchronization state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// First local sequence not yet pushed.
    #[must_use]
    pub fn next_to_push(&self) -> u64 {
        self.next_to_push
    }

    /// The durable ack for a local event, if it has one.
    #[must_use]
    pub fn ack_of(&self, local_seq: u64) -> Option<&Ack> {
        usize::try_from(local_seq)
            .ok()
            .and_then(|index| self.acks.get(index))
            .and_then(Option::as_ref)
    }

    /// How many contiguous local events are durably acknowledged.
    #[must_use]
    pub fn confirmed(&self) -> u64 {
        self.next_to_push
    }

    /// Validate and record one ordered acknowledgement.
    pub fn try_confirm(&mut self, local_seq: u64, ack: Ack) -> Result<(), ValidationError> {
        ack.validate()?;
        if local_seq < self.next_to_push {
            return match self.ack_of(local_seq) {
                Some(existing) if existing == &ack => Ok(()),
                _ => Err(ValidationError::new(
                    "ack",
                    "conflicting duplicate acknowledgement",
                )),
            };
        }
        if local_seq != self.next_to_push {
            return Err(ValidationError::new("local_seq", "acknowledgement gap"));
        }
        let index = usize::try_from(local_seq)
            .map_err(|_| ValidationError::new("local_seq", "platform index overflow"))?;
        if index >= MAX_LEGACY_ACKS {
            return Err(ValidationError::new(
                "local_seq",
                "legacy acknowledgement limit exceeded",
            ));
        }
        if self.acks.len() <= index {
            self.acks.resize(index + 1, None);
        }
        self.acks[index] = Some(ack);
        self.next_to_push += 1;
        Ok(())
    }

    /// Compatibility wrapper. Invalid or out-of-order acks are ignored; new
    /// integrations should use [`Self::try_confirm`] and handle the error.
    pub fn confirm(&mut self, local_seq: u64, ack: Ack) {
        let _ = self.try_confirm(local_seq, ack);
    }
}

/// Legacy one-event journal transport retained for the existing spike.
pub trait JournalTransport {
    /// Append one event; return where it landed on the durable chain.
    fn append(&mut self, kind: &str, payload: &str) -> Result<Ack, String>;
}

/// Push everything pending through a legacy one-event transport.
///
/// This function validates the local chain, event kind, payload bound, and
/// acknowledgement hash. It cannot make a lost acknowledgement idempotent;
/// use [`append_with_retry`] with [`BatchTransport`] for that guarantee.
pub fn push_pending<T: JournalTransport>(
    log: &Log,
    sync: &mut SyncState,
    transport: &mut T,
) -> Result<u64, String> {
    log.verify()
        .map_err(|tampered| format!("local log tampered at {}", tampered.0))?;
    let mut pushed = 0;
    let events: Vec<(u64, String, String)> = log
        .tail(sync.next_to_push())
        .iter()
        .map(|event| (event.seq, event.kind.clone(), event.payload.clone()))
        .collect();
    for (seq, kind, payload) in events {
        let kind = durable_kind(&kind);
        validate_kind(&kind).map_err(|error| error.to_string())?;
        if payload.len() > MAX_EVENT_PAYLOAD_BYTES {
            return Err("legacy payload exceeds event limit".to_owned());
        }
        let ack = transport.append(&kind, &payload)?;
        sync.try_confirm(seq, ack)
            .map_err(|error| error.to_string())?;
        pushed += 1;
    }
    Ok(pushed)
}

// ───────────────────────── browser legacy transport ─────────────────────────

#[cfg(target_arch = "wasm32")]
pub mod fetch {
    //! Legacy `fetch` transport retained for the current browser spike. It is
    //! not the authenticated production batch transport.

    use super::{Ack, durable_kind, validate_kind};
    use js_sys::Uint8Array;
    use serde::Deserialize;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{ReadableStreamDefaultReader, ReadableStreamReadResult};

    const MAX_ACK_RESPONSE_BYTES: usize = 64 * 1024;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AckWire {
        seq: u64,
        hash: String,
    }

    fn js_error(error: JsValue) -> String {
        format!("{error:?}")
    }

    async fn bounded_response_text(response: &web_sys::Response) -> Result<String, String> {
        if let Some(content_length) = response.headers().get("content-length").map_err(js_error)? {
            let content_length = content_length
                .parse::<usize>()
                .map_err(|_| "invalid content-length response header".to_owned())?;
            if content_length > MAX_ACK_RESPONSE_BYTES {
                return Err("ack response exceeds limit".to_owned());
            }
        }

        let stream = response
            .body()
            .ok_or_else(|| "ack response has no readable body".to_owned())?;
        let reader = ReadableStreamDefaultReader::new(&stream).map_err(js_error)?;
        let mut bytes = Vec::new();
        loop {
            let result: ReadableStreamReadResult = JsFuture::from(reader.read())
                .await
                .map_err(js_error)?
                .dyn_into()
                .map_err(js_error)?;
            if result.get_done().unwrap_or(false) {
                break;
            }
            let chunk = Uint8Array::new(&result.get_value());
            let chunk_length = usize::try_from(chunk.length())
                .map_err(|_| "ack response chunk is too large".to_owned())?;
            let next_length = bytes
                .len()
                .checked_add(chunk_length)
                .ok_or_else(|| "ack response size overflow".to_owned())?;
            if next_length > MAX_ACK_RESPONSE_BYTES {
                let _ = JsFuture::from(reader.cancel()).await;
                return Err("ack response exceeds limit".to_owned());
            }
            let start = bytes.len();
            bytes.resize(next_length, 0);
            chunk.copy_to(&mut bytes[start..]);
        }
        String::from_utf8(bytes).map_err(|_| "ack response is not UTF-8".to_owned())
    }

    /// POST one local event to `{base}/v1/journal/append`.
    pub async fn append_remote(base: &str, local_kind: &str, payload: &str) -> Result<Ack, String> {
        let kind = durable_kind(local_kind);
        validate_kind(&kind).map_err(|error| error.to_string())?;
        if payload.len() > super::MAX_EVENT_PAYLOAD_BYTES {
            return Err("payload exceeds event limit".to_owned());
        }
        let body = serde_json::json!({ "kind": kind, "payload": payload }).to_string();
        let options = web_sys::RequestInit::new();
        options.set_method("POST");
        options.set_body(&JsValue::from_str(&body));
        let request =
            web_sys::Request::new_with_str_and_init(&format!("{base}/v1/journal/append"), &options)
                .map_err(js_error)?;
        request
            .headers()
            .set("content-type", "application/json")
            .map_err(js_error)?;
        request
            .headers()
            .set("accept", "application/json")
            .map_err(js_error)?;
        let window = web_sys::window().ok_or("no window")?;
        let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(js_error)?
            .dyn_into()
            .map_err(js_error)?;
        let text = bounded_response_text(&response).await?;
        if !response.ok() {
            let preview: String = text.chars().take(200).collect();
            return Err(format!("HTTP {}: {preview}", response.status()));
        }
        let wire: AckWire =
            serde_json::from_str(&text).map_err(|error| format!("bad ack json: {error}"))?;
        let ack = Ack {
            seq: wire.seq,
            hash: wire.hash,
        };
        ack.validate().map_err(|error| error.to_string())?;
        Ok(ack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_log::Hash;

    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const ID_1: &str = "01890f3e-9b4a-7cc0-8a1a-0123456789ab";
    const ID_2: &str = "01890f3e-9b4a-7cc1-8a1a-0123456789ab";
    const BATCH_ID: &str = "01890f3e-9b4a-7cc2-8a1a-0123456789ab";

    #[derive(Default)]
    struct FakeHyphae {
        journal: Log,
        appends: u64,
    }

    impl JournalTransport for FakeHyphae {
        fn append(&mut self, kind: &str, payload: &str) -> Result<Ack, String> {
            self.appends += 1;
            let event = self.journal.append(kind, payload);
            Ok(Ack {
                seq: event.seq,
                hash: hex(&event.hash),
            })
        }
    }

    struct Flaky {
        inner: FakeHyphae,
        allow: u64,
    }

    impl JournalTransport for Flaky {
        fn append(&mut self, kind: &str, payload: &str) -> Result<Ack, String> {
            if self.inner.appends >= self.allow {
                return Err("network down".into());
            }
            self.inner.append(kind, payload)
        }
    }

    fn client_log(n: u64) -> Log {
        let mut log = Log::new();
        for index in 0..n {
            log.append("task_added", format!("t{index}"));
        }
        log
    }

    fn envelopes() -> Vec<EventEnvelope> {
        let mut log = Log::new();
        log.append("task_added", "first");
        log.append("task_added", "second");
        [ID_1, ID_2]
            .into_iter()
            .zip(log.events())
            .map(|(id, event)| {
                EventEnvelope::from_local_event(
                    event,
                    id,
                    "project:alpha",
                    1,
                    "2026-07-12T20:00:00Z",
                )
                .unwrap()
            })
            .collect()
    }

    fn batch() -> AppendBatch {
        AppendBatch {
            protocol: PROTOCOL_V1.to_owned(),
            batch_id: BATCH_ID.to_owned(),
            stream_id: "project:alpha".to_owned(),
            expected_cursor: Some(StreamCursor {
                position: 0,
                head_hash: ZERO_HASH.to_owned(),
            }),
            events: envelopes(),
        }
    }

    fn server_hash(previous: &str, sequence: u64) -> String {
        let mut bytes: Hash = [0; 32];
        for (index, byte) in previous.bytes().take(32).enumerate() {
            bytes[index] = byte ^ (sequence as u8).wrapping_add(1);
        }
        hex(&bytes)
    }

    fn response_for(batch: &AppendBatch) -> AppendResponse {
        let mut previous = batch
            .expected_cursor
            .as_ref()
            .map_or_else(|| ZERO_HASH.to_owned(), |cursor| cursor.head_hash.clone());
        let mut receipts = Vec::new();
        for (index, event) in batch.events.iter().enumerate() {
            let hash = server_hash(&previous, index as u64);
            receipts.push(Receipt {
                client_event_id: event.client_event_id.clone(),
                stream_id: batch.stream_id.clone(),
                envelope_hash: event.wire_hash().unwrap(),
                server_seq: index as u64,
                server_hash: hash.clone(),
                server_prev_hash: previous,
                journal_head: hash.clone(),
                committed_at: "2026-07-12T20:00:01Z".to_owned(),
                key_id: "hyphae-test-1".to_owned(),
                signature: "dGVzdC1zaWduYXR1cmU".to_owned(),
            });
            previous = hash;
        }
        for receipt in &mut receipts {
            receipt.journal_head.clone_from(&previous);
        }
        AppendResponse {
            protocol: PROTOCOL_V1.to_owned(),
            batch_id: batch.batch_id.clone(),
            stream_id: batch.stream_id.clone(),
            receipts,
            next_cursor: StreamCursor {
                position: batch.events.len() as u64,
                head_hash: previous,
            },
        }
    }

    #[test]
    fn rejects_traversal_reserved_kinds_and_malformed_hashes() {
        assert!(validate_stream_id("../tenant/other").is_err());
        assert!(validate_stream_id("tenant\nother").is_err());
        assert!(validate_kind("kv_put").is_err());
        assert!(validate_kind("app_tombstone-").is_err());
        assert!(validate_hash(&"A".repeat(64)).is_err());
        assert!(validate_hash(&"a".repeat(63)).is_err());
    }

    #[test]
    fn timestamps_are_validated_as_rfc3339_not_by_shape() {
        for valid in [
            "2026-07-12T20:00:00Z",
            "2016-12-31t23:59:60z",
            "2026-07-12T20:00:00.123456+05:30",
        ] {
            assert!(validate_timestamp(valid).is_ok(), "rejected {valid}");
        }
        for invalid in [
            "2026-02-29T20:00:00Z",
            "2026-13-12T20:00:00Z",
            "2026-07-12T24:00:00Z",
            "2026-07-12T20:60:00Z",
            "2026-07-12T20:00:61Z",
            "2024-02-29T23:59:60Z",
            "2026-07-12T20:00:00.+01:00",
            "2026-07-12T20:00:00+24:00",
            "2026-07-12T20:00:00Ztrailing",
        ] {
            assert!(validate_timestamp(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn cursors_bind_genesis_position_and_hash() {
        assert!(StreamCursor::genesis().validate().is_ok());
        assert!(
            StreamCursor {
                position: 0,
                head_hash: "a".repeat(64),
            }
            .validate()
            .is_err()
        );
        assert!(
            StreamCursor {
                position: 1,
                head_hash: EMPTY_STREAM_HASH.to_owned(),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_protocol_fields_and_oversized_payloads() {
        let mut event = envelopes().remove(0);
        event.protocol = "pliego-hyphae/2".to_owned();
        assert_eq!(event.validate().unwrap_err().field(), "protocol");

        let json = format!(
            "{{\"protocol\":\"{PROTOCOL_V1}\",\"stream_id\":\"a\",\"after\":null,\"limit\":1,\"tenant_id\":\"forged\"}}"
        );
        assert!(serde_json::from_str::<PullRequest>(&json).is_err());

        event = envelopes().remove(0);
        event.payload = format!("\"{}\"", "x".repeat(MAX_EVENT_PAYLOAD_BYTES));
        assert_eq!(event.validate().unwrap_err().field(), "payload");
    }

    #[test]
    fn batch_rejects_duplicates_reordering_and_broken_local_links() {
        let mut duplicate = batch();
        duplicate.events[1].client_event_id = ID_1.to_owned();
        assert!(duplicate.validate().is_err());

        let mut reordered = batch();
        reordered.events.swap(0, 1);
        assert!(reordered.validate().is_err());

        let mut broken = batch();
        broken.events[1].local_prev_hash = ZERO_HASH.to_owned();
        assert!(broken.validate().is_err());
    }

    #[test]
    fn response_rejects_id_hash_and_cursor_substitution() {
        let batch = batch();
        let mut response = response_for(&batch);
        response.receipts[0].client_event_id = ID_2.to_owned();
        assert!(response.validate_against(&batch).is_err());

        response = response_for(&batch);
        response.receipts[1].server_prev_hash = ZERO_HASH.to_owned();
        assert!(response.validate_against(&batch).is_err());

        response = response_for(&batch);
        response.next_cursor.position = 99;
        assert!(response.validate_against(&batch).is_err());

        let response = response_for(&batch);
        let mut substituted = batch.clone();
        substituted.events[0].payload = "\"changed after idempotent commit\"".to_owned();
        assert!(response.validate_against(&substituted).is_err());
    }

    #[test]
    fn absent_append_cursor_is_a_derived_genesis_precondition() {
        let mut initial = batch();
        initial.expected_cursor = None;
        let response = response_for(&initial);
        response.validate_against(&initial).unwrap();

        let mut substituted = response.clone();
        substituted.next_cursor.position += 1;
        assert!(substituted.validate_against(&initial).is_err());

        substituted = response;
        substituted.receipts[0].server_prev_hash = "a".repeat(64);
        assert!(substituted.validate_against(&initial).is_err());
    }

    #[test]
    fn receipt_verification_covers_every_durable_claim() {
        struct ExactVerifier(Vec<u8>);
        impl ReceiptVerifier for ExactVerifier {
            fn verify(
                &self,
                key_id: &str,
                payload: &[u8],
                signature: &str,
            ) -> Result<bool, String> {
                Ok(key_id == "hyphae-test-1"
                    && signature == "dGVzdC1zaWduYXR1cmU"
                    && payload == self.0)
            }
        }

        let mut receipt = response_for(&batch()).receipts.remove(0);
        let verifier = ExactVerifier(receipt.signing_payload().unwrap());
        verify_receipt(&receipt, &verifier).unwrap();
        receipt.server_hash = "c".repeat(64);
        assert_eq!(
            verify_receipt(&receipt, &verifier),
            Err(ReceiptVerificationError::InvalidSignature)
        );
    }

    #[derive(Default)]
    struct IdempotentServer {
        committed: BTreeMap<String, AppendResponse>,
        durable_appends: usize,
        lose_first_ack: bool,
    }

    impl BatchTransport for IdempotentServer {
        fn append_batch(&mut self, batch: &AppendBatch) -> Result<AppendResponse, TransportError> {
            if let Some(response) = self.committed.get(&batch.batch_id) {
                return Ok(response.clone());
            }
            let response = response_for(batch);
            self.durable_appends += batch.events.len();
            self.committed
                .insert(batch.batch_id.clone(), response.clone());
            if self.lose_first_ack {
                self.lose_first_ack = false;
                return Err(TransportError::Retryable("ack lost".to_owned()));
            }
            Ok(response)
        }
    }

    #[test]
    fn lost_ack_retry_reuses_batch_and_deduplicates_server_append() {
        let batch = batch();
        let mut server = IdempotentServer {
            lose_first_ack: true,
            ..IdempotentServer::default()
        };
        let response = append_with_retry(&mut server, &batch, 2).unwrap();
        assert_eq!(response.receipts.len(), 2);
        assert_eq!(
            server.durable_appends, 2,
            "retry created no duplicate entries"
        );
        assert_eq!(server.committed.len(), 1, "one idempotency record");
    }

    #[test]
    fn retry_never_retries_cursor_conflicts() {
        struct Conflict(u8);
        impl BatchTransport for Conflict {
            fn append_batch(
                &mut self,
                batch: &AppendBatch,
            ) -> Result<AppendResponse, TransportError> {
                self.0 += 1;
                Err(TransportError::CursorConflict {
                    expected: batch.expected_cursor.clone(),
                    actual: StreamCursor {
                        position: 9,
                        head_hash: "a".repeat(64),
                    },
                })
            }
        }
        let mut server = Conflict(0);
        assert!(matches!(
            append_with_retry(&mut server, &batch(), 5),
            Err(SyncError::Transport(TransportError::CursorConflict { .. }))
        ));
        assert_eq!(server.0, 1);
    }

    #[derive(Default)]
    struct CountingSink(Vec<String>);
    impl ReplaySink for CountingSink {
        fn apply_batch(&mut self, events: &[AcceptedEvent]) -> Result<(), String> {
            self.0.extend(
                events
                    .iter()
                    .map(|accepted| accepted.envelope.client_event_id.clone()),
            );
            Ok(())
        }
    }

    #[test]
    fn pull_page_validates_chain_and_replay_deduplicates_overlap() {
        let batch = batch();
        let response = response_for(&batch);
        let page = PullPage {
            protocol: PROTOCOL_V1.to_owned(),
            stream_id: batch.stream_id.clone(),
            events: batch
                .events
                .iter()
                .cloned()
                .zip(response.receipts.iter().cloned())
                .map(|(envelope, receipt)| AcceptedEvent { envelope, receipt })
                .collect(),
            next_cursor: response.next_cursor.clone(),
            complete: true,
        };
        let request = PullRequest {
            protocol: PROTOCOL_V1.to_owned(),
            stream_id: batch.stream_id.clone(),
            after: batch.expected_cursor.clone(),
            limit: 10,
        };
        let mut state = ReplayState::default();
        let mut sink = CountingSink::default();
        assert_eq!(
            apply_pull_page(&mut state, &request, &page, &mut sink).unwrap(),
            2
        );
        assert_eq!(state.dedupe_window_len(), 2);

        // The same validated recovery page is harmless when a caller resumes
        // from the same persisted request after losing its local completion ACK.
        assert_eq!(
            apply_pull_page(&mut state, &request, &page, &mut sink).unwrap(),
            0
        );
        assert_eq!(sink.0.len(), 2);
    }

    #[test]
    fn pull_rejects_discontinuous_and_non_advancing_pages() {
        let batch = batch();
        let response = response_for(&batch);
        let request = PullRequest {
            protocol: PROTOCOL_V1.to_owned(),
            stream_id: batch.stream_id.clone(),
            after: batch.expected_cursor.clone(),
            limit: 10,
        };
        let mut page = PullPage {
            protocol: PROTOCOL_V1.to_owned(),
            stream_id: batch.stream_id.clone(),
            events: vec![AcceptedEvent {
                envelope: batch.events[0].clone(),
                receipt: response.receipts[0].clone(),
            }],
            next_cursor: StreamCursor {
                position: 1,
                head_hash: response.receipts[0].journal_head.clone(),
            },
            complete: false,
        };
        page.events[0].receipt.server_prev_hash = "b".repeat(64);
        assert!(page.validate_against(&request).is_err());

        page.events.clear();
        page.next_cursor = request.after.clone().unwrap();
        assert!(page.validate_against(&request).is_err());
    }

    #[test]
    fn legacy_gate_push_and_retry_remain_compatible() {
        let log = client_log(6);
        let mut sync = SyncState::new();
        let mut flaky = Flaky {
            inner: FakeHyphae::default(),
            allow: 2,
        };
        assert!(push_pending(&log, &mut sync, &mut flaky).is_err());
        assert_eq!(sync.next_to_push(), 2);
        flaky.allow = u64::MAX;
        assert_eq!(push_pending(&log, &mut sync, &mut flaky).unwrap(), 4);
        assert_eq!(sync.confirmed(), 6);
        assert_eq!(flaky.inner.appends, 6);
    }

    #[test]
    fn legacy_ack_guard_rejects_gaps_conflicts_and_memory_abuse() {
        let valid = Ack {
            seq: 0,
            hash: "a".repeat(64),
        };
        let mut state = SyncState::new();
        assert!(state.try_confirm(1, valid.clone()).is_err());
        state.try_confirm(0, valid.clone()).unwrap();
        assert!(state.try_confirm(0, valid).is_ok());
        assert!(
            state
                .try_confirm(
                    MAX_LEGACY_ACKS as u64,
                    Ack {
                        seq: 1,
                        hash: "b".repeat(64),
                    }
                )
                .is_err()
        );
    }
}
