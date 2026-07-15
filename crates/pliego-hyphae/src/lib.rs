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
//! The modern API is fail-closed: untrusted wire values must cross validation,
//! authority verification, and event-version policy before replay. The old M5
//! seam is available only with the non-default `experimental-legacy` feature.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

#[cfg(feature = "experimental-legacy")]
use pliego_log::Log;
use pliego_log::{Event, hex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Exact modern wire protocol accepted by this client contract.
pub const PROTOCOL_V2: &str = "pliego-hyphae/2";
const RECEIPT_DOMAIN: &str = "pliego-hyphae/2/receipt";
const PAGE_DOMAIN: &str = "pliego-hyphae/2/page";
const APPEND_DOMAIN: &str = "pliego-hyphae/2/append";
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
#[cfg(feature = "experimental-legacy")]
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
    if protocol == PROTOCOL_V2 {
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
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "stream_id",
            "expected 1..128 safe ASCII characters without traversal segments",
        ))
    }
}

fn validate_authority_id(value: &str) -> Result<(), ValidationError> {
    let valid = (1..=96).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "authority_id",
            "expected a safe ASCII authority token",
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
    let padding_is_canonical = match padding {
        0 => core.len() % 4 != 1,
        1 => core.len() % 4 == 3 && value.len() % 4 == 0,
        2 => core.len() % 4 == 2 && value.len() % 4 == 0,
        _ => false,
    };
    let valid = (16..=512).contains(&value.len())
        && padding_is_canonical
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
            protocol: PROTOCOL_V2.to_owned(),
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

/// A validated, opaque authority identity returned by the trust boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AuthorityId(String);

impl AuthorityId {
    /// Construct an authority identity after validating its bounded wire form.
    pub fn try_new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        validate_authority_id(&value)?;
        Ok(Self(value))
    }

    /// Borrow the canonical authority token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AuthorityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(value).map_err(serde::de::Error::custom)
    }
}

fn hash_bytes(hasher: &mut Sha256, value: &[u8]) -> Result<(), ValidationError> {
    let length = u32::try_from(value.len())
        .map_err(|_| ValidationError::new("signature_payload", "field length overflow"))?;
    hasher.update(length.to_be_bytes());
    hasher.update(value);
    Ok(())
}

fn append_field(target: &mut Vec<u8>, value: &str) -> Result<(), ValidationError> {
    let length = u32::try_from(value.len())
        .map_err(|_| ValidationError::new("signature_payload", "field length overflow"))?;
    target.extend_from_slice(&length.to_be_bytes());
    target.extend_from_slice(value.as_bytes());
    Ok(())
}

fn append_cursor(target: &mut Vec<u8>, cursor: &StreamCursor) -> Result<(), ValidationError> {
    cursor.validate()?;
    target.extend_from_slice(&cursor.position.to_be_bytes());
    append_field(target, &cursor.head_hash)
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
    /// Exact durable cursor the batch must extend, including genesis.
    pub expected_cursor: StreamCursor,
    /// Ordered, contiguous local events.
    pub events: Vec<EventEnvelope>,
}

impl AppendBatch {
    /// Validate batch bounds, identities, ordering, and local chain links.
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure_protocol(&self.protocol)?;
        validate_batch_id(&self.batch_id)?;
        validate_stream_id(&self.stream_id)?;
        self.expected_cursor.validate()?;
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
    /// One-based sequence on the durable journal.
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
        if self.server_seq == 0 {
            return Err(ValidationError::new("server_seq", "must be one-based"));
        }
        validate_hash(&self.server_hash)?;
        validate_hash(&self.server_prev_hash)?;
        validate_hash(&self.journal_head)?;
        if self.server_hash == EMPTY_STREAM_HASH || self.server_hash == self.server_prev_hash {
            return Err(ValidationError::new(
                "server_hash",
                "durable entry hash must advance from a non-entry sentinel",
            ));
        }
        if self.journal_head == EMPTY_STREAM_HASH || self.journal_head == self.server_prev_hash {
            return Err(ValidationError::new(
                "journal_head",
                "durable journal head must include the accepted entry",
            ));
        }
        validate_timestamp(&self.committed_at)?;
        validate_key_id(&self.key_id)?;
        validate_signature(&self.signature)
    }

    /// Canonical bytes signed for one durable receipt.
    pub fn signing_payload(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate_shape()?;
        let mut payload = Vec::new();
        for field in [
            RECEIPT_DOMAIN,
            PROTOCOL_V2,
            self.client_event_id.as_str(),
            self.stream_id.as_str(),
            self.envelope_hash.as_str(),
        ] {
            append_field(&mut payload, field)?;
        }
        payload.extend_from_slice(&self.server_seq.to_be_bytes());
        for field in [
            self.server_hash.as_str(),
            self.server_prev_hash.as_str(),
            self.journal_head.as_str(),
            self.committed_at.as_str(),
            self.key_id.as_str(),
        ] {
            append_field(&mut payload, field)?;
        }
        Ok(payload)
    }

    /// Lossy compatibility view used only by the experimental legacy seam.
    #[cfg(feature = "experimental-legacy")]
    #[must_use]
    pub fn legacy_ack(&self) -> Ack {
        Ack {
            seq: self.server_seq,
            hash: self.server_hash.clone(),
        }
    }
}

/// Kind of signed object presented to the authority verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignaturePurpose {
    /// One durable event receipt.
    Receipt,
    /// One complete atomic append response.
    AppendAttestation,
    /// One bounded pull page, including an empty page.
    PageAttestation,
}

/// Typed metadata used for key scope, rotation, and revocation decisions.
#[derive(Debug, Clone, Copy)]
pub struct VerificationContext<'a> {
    purpose: SignaturePurpose,
    claimed_authority: Option<&'a AuthorityId>,
    stream_id: &'a str,
    signed_at: &'a str,
    key_id: &'a str,
}

impl<'a> VerificationContext<'a> {
    /// Signed object class.
    #[must_use]
    pub const fn purpose(&self) -> SignaturePurpose {
        self.purpose
    }

    /// Authority claimed by the enclosing attestation, when present.
    #[must_use]
    pub const fn claimed_authority(&self) -> Option<&'a AuthorityId> {
        self.claimed_authority
    }

    /// Authorized stream scope.
    #[must_use]
    pub const fn stream_id(&self) -> &'a str {
        self.stream_id
    }

    /// Timestamp covered by the signature.
    #[must_use]
    pub const fn signed_at(&self) -> &'a str {
        self.signed_at
    }

    /// Key identifier covered by the signature.
    #[must_use]
    pub const fn key_id(&self) -> &'a str {
        self.key_id
    }
}

/// Fail-closed authority verification failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationError {
    /// The key identifier is not trusted.
    UnknownKey,
    /// The key was revoked at the signed timestamp.
    RevokedKey,
    /// The key or authority is not authorized for this stream.
    UnauthorizedStream,
    /// The detached signature does not cover the supplied canonical bytes.
    InvalidSignature,
    /// The verifier could not make a trustworthy decision.
    Unavailable(String),
    /// Different signatures resolved to different authorities.
    AuthorityMismatch,
}

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey => f.write_str("unknown signing key"),
            Self::RevokedKey => f.write_str("signing key was revoked"),
            Self::UnauthorizedStream => f.write_str("authority is not authorized for stream"),
            Self::InvalidSignature => f.write_str("invalid detached signature"),
            Self::Unavailable(message) => write!(f, "authority verifier unavailable: {message}"),
            Self::AuthorityMismatch => f.write_str("signatures resolve to different authorities"),
        }
    }
}

impl Error for VerificationError {}

/// Cryptographic boundary for trusted Hyphae keys and authority policy.
pub trait ReceiptVerifier {
    /// Verify canonical bytes and return the stable authority controlling the key.
    fn verify(
        &self,
        context: VerificationContext<'_>,
        signing_payload: &[u8],
        signature: &str,
    ) -> Result<AuthorityId, VerificationError>;
}

fn verify_as<V: ReceiptVerifier>(
    verifier: &V,
    context: VerificationContext<'_>,
    payload: &[u8],
    signature: &str,
) -> Result<AuthorityId, VerificationError> {
    let claimed = context.claimed_authority.cloned();
    let authority = verifier.verify(context, payload, signature)?;
    if claimed
        .as_ref()
        .is_some_and(|expected| expected != &authority)
    {
        return Err(VerificationError::AuthorityMismatch);
    }
    Ok(authority)
}

fn accepted_events_hash(events: &[AcceptedEvent]) -> Result<String, ValidationError> {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "pliego-hyphae/2/accepted-events")?;
    let count = u64::try_from(events.len())
        .map_err(|_| ValidationError::new("events", "count overflow"))?;
    hasher.update(count.to_be_bytes());
    for accepted in events {
        hash_field(&mut hasher, &accepted.envelope.wire_hash()?)?;
        hash_bytes(&mut hasher, &accepted.receipt.signing_payload()?)?;
        hash_field(&mut hasher, &accepted.receipt.signature)?;
    }
    let digest: [u8; 32] = hasher.finalize().into();
    Ok(hex(&digest))
}

fn append_events_hash(
    events: &[EventEnvelope],
    receipts: &[Receipt],
) -> Result<String, ValidationError> {
    if events.len() != receipts.len() {
        return Err(ValidationError::new("receipts", "cardinality mismatch"));
    }
    let paired: Vec<AcceptedEvent> = events
        .iter()
        .cloned()
        .zip(receipts.iter().cloned())
        .map(|(envelope, receipt)| AcceptedEvent { envelope, receipt })
        .collect();
    accepted_events_hash(&paired)
}

/// Signature over the complete atomic append result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendAttestation {
    /// Stable authority owning the signing key.
    pub authority_id: AuthorityId,
    /// Authoritative timestamp for the atomic append claim.
    pub committed_at: String,
    /// Signing key identifier.
    pub key_id: String,
    /// Detached base64url signature.
    pub signature: String,
}

impl AppendAttestation {
    fn validate_shape(&self) -> Result<(), ValidationError> {
        validate_authority_id(self.authority_id.as_str())?;
        validate_timestamp(&self.committed_at)?;
        validate_key_id(&self.key_id)?;
        validate_signature(&self.signature)
    }

    /// Canonical bytes binding batch identity, precondition, atomic set, and cursor.
    pub fn signing_payload(
        &self,
        batch: &AppendBatch,
        response: &AppendResponse,
    ) -> Result<Vec<u8>, ValidationError> {
        self.validate_shape()?;
        batch.validate()?;
        let mut payload = Vec::new();
        for field in [
            APPEND_DOMAIN,
            PROTOCOL_V2,
            self.authority_id.as_str(),
            batch.batch_id.as_str(),
            batch.stream_id.as_str(),
        ] {
            append_field(&mut payload, field)?;
        }
        append_cursor(&mut payload, &batch.expected_cursor)?;
        append_cursor(&mut payload, &response.next_cursor)?;
        let count = u64::try_from(batch.events.len())
            .map_err(|_| ValidationError::new("events", "count overflow"))?;
        payload.extend_from_slice(&count.to_be_bytes());
        append_field(
            &mut payload,
            &append_events_hash(&batch.events, &response.receipts)?,
        )?;
        append_field(&mut payload, &self.committed_at)?;
        append_field(&mut payload, &self.key_id)?;
        Ok(payload)
    }
}

/// Raw response for an accepted or deduplicated append batch.
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
    /// Signature over the complete append transaction.
    pub attestation: AppendAttestation,
}

impl AppendResponse {
    fn validate_against(&self, batch: &AppendBatch) -> Result<(), ValidationError> {
        batch.validate()?;
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
        self.attestation.validate_shape()?;
        for (index, (event, receipt)) in batch.events.iter().zip(&self.receipts).enumerate() {
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
            let offset = u64::try_from(index)
                .map_err(|_| ValidationError::new("receipts", "index overflow"))?;
            let expected_seq = batch
                .expected_cursor
                .position
                .checked_add(offset)
                .and_then(|position| position.checked_add(1))
                .ok_or(ValidationError::new("server_seq", "sequence overflow"))?;
            if receipt.server_seq != expected_seq {
                return Err(ValidationError::new(
                    "server_seq",
                    "receipt sequence is not contiguous",
                ));
            }
            let expected_prev = if index == 0 {
                &batch.expected_cursor.head_hash
            } else {
                &self.receipts[index - 1].server_hash
            };
            if &receipt.server_prev_hash != expected_prev {
                return Err(ValidationError::new(
                    "receipts",
                    "durable hash link mismatch",
                ));
            }
        }
        let last = self
            .receipts
            .last()
            .ok_or(ValidationError::new("receipts", "empty response"))?;
        let appended = u64::try_from(batch.events.len())
            .map_err(|_| ValidationError::new("events", "count overflow"))?;
        let next_position = batch
            .expected_cursor
            .position
            .checked_add(appended)
            .ok_or(ValidationError::new("next_cursor", "position overflow"))?;
        if self.next_cursor.position != next_position
            || self.next_cursor.head_hash != last.server_hash
            || last.journal_head != self.next_cursor.head_hash
            || self
                .receipts
                .iter()
                .any(|receipt| receipt.journal_head != self.next_cursor.head_hash)
        {
            return Err(ValidationError::new(
                "next_cursor",
                "atomic append cursor or journal head mismatch",
            ));
        }
        Ok(())
    }
}

/// Structurally valid append response; signatures remain untrusted.
#[derive(Debug, Clone)]
pub struct ValidatedAppendResponse {
    batch: AppendBatch,
    response: AppendResponse,
}

impl ValidatedAppendResponse {
    /// Inspect the validated raw response without claiming authenticity.
    #[must_use]
    pub fn response(&self) -> &AppendResponse {
        &self.response
    }

    /// Verify the append attestation and every receipt under one authority.
    pub fn verify<V: ReceiptVerifier>(
        self,
        verifier: &V,
    ) -> Result<VerifiedAppendResponse, SyncError> {
        let claimed = &self.response.attestation.authority_id;
        let payload = self
            .response
            .attestation
            .signing_payload(&self.batch, &self.response)
            .map_err(SyncError::Validation)?;
        let authority = verify_as(
            verifier,
            VerificationContext {
                purpose: SignaturePurpose::AppendAttestation,
                claimed_authority: Some(claimed),
                stream_id: &self.batch.stream_id,
                signed_at: &self.response.attestation.committed_at,
                key_id: &self.response.attestation.key_id,
            },
            &payload,
            &self.response.attestation.signature,
        )
        .map_err(SyncError::Verification)?;
        for receipt in &self.response.receipts {
            let payload = receipt.signing_payload().map_err(SyncError::Validation)?;
            let receipt_authority = verify_as(
                verifier,
                VerificationContext {
                    purpose: SignaturePurpose::Receipt,
                    claimed_authority: Some(&authority),
                    stream_id: &receipt.stream_id,
                    signed_at: &receipt.committed_at,
                    key_id: &receipt.key_id,
                },
                &payload,
                &receipt.signature,
            )
            .map_err(SyncError::Verification)?;
            if receipt_authority != authority {
                return Err(SyncError::Verification(
                    VerificationError::AuthorityMismatch,
                ));
            }
        }
        Ok(VerifiedAppendResponse {
            response: self.response,
            authority,
        })
    }
}

/// Cryptographically verified atomic append response.
#[derive(Debug, Clone)]
pub struct VerifiedAppendResponse {
    response: AppendResponse,
    authority: AuthorityId,
}

impl VerifiedAppendResponse {
    /// Authority that signed the complete result and every receipt.
    #[must_use]
    pub fn authority(&self) -> &AuthorityId {
        &self.authority
    }

    /// Authenticated raw response.
    #[must_use]
    pub fn response(&self) -> &AppendResponse {
        &self.response
    }

    /// Authenticated next cursor.
    #[must_use]
    pub fn next_cursor(&self) -> &StreamCursor {
        &self.response.next_cursor
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
        expected: StreamCursor,
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
    /// A signature, key, scope, or authority decision failed closed.
    Verification(VerificationError),
    /// The application does not support this event kind and schema version.
    EventVersion(EventVersionError),
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
            Self::Verification(error) => error.fmt(f),
            Self::EventVersion(error) => error.fmt(f),
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
) -> Result<ValidatedAppendResponse, SyncError> {
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
                return Ok(ValidatedAppendResponse {
                    batch: batch.clone(),
                    response,
                });
            }
            Err(TransportError::Retryable(_)) => {}
            Err(error) => return Err(SyncError::Transport(error)),
        }
    }
    Err(SyncError::AttemptsExhausted)
}

/// Only whole-stream replay is supported by protocol v2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullSelection {
    /// Return the complete ordered stream within the fixed snapshot.
    WholeStream,
}

/// Snapshot policy for one pull cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "cursor", rename_all = "snake_case")]
pub enum SnapshotSelection {
    /// Ask the authority to fix the latest stream head for this cycle.
    Latest,
    /// Continue against an exact snapshot returned by an earlier page.
    Exact(StreamCursor),
}

/// Request for one bounded, snapshot-consistent pull page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullRequest {
    /// Wire protocol version.
    pub protocol: String,
    /// UUIDv7 binding all signed values to this request.
    pub request_id: String,
    /// Authorized application stream.
    pub stream_id: String,
    /// Exact cursor after which events are requested, including genesis.
    pub after: StreamCursor,
    /// Latest snapshot discovery or an exact fixed snapshot.
    pub snapshot: SnapshotSelection,
    /// Explicit replay selection. V2 supports only the whole stream.
    pub selection: PullSelection,
    /// Bounded page size.
    pub limit: u16,
}

impl PullRequest {
    /// Validate request bounds and identities.
    pub fn validate(&self) -> Result<(), ValidationError> {
        ensure_protocol(&self.protocol)?;
        validate_uuid_v7("request_id", &self.request_id)?;
        validate_stream_id(&self.stream_id)?;
        self.after.validate()?;
        if let SnapshotSelection::Exact(snapshot) = &self.snapshot {
            snapshot.validate()?;
            if snapshot.position < self.after.position
                || (snapshot.position == self.after.position
                    && snapshot.head_hash != self.after.head_hash)
            {
                return Err(ValidationError::new(
                    "snapshot",
                    "exact snapshot precedes or forks from request cursor",
                ));
            }
        }
        if self.limit == 0 || self.limit > MAX_PULL_EVENTS {
            return Err(ValidationError::new("limit", "must be in 1..=256"));
        }
        Ok(())
    }
}

/// One untrusted wire envelope paired with its durable receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedEvent {
    /// Original accepted client envelope.
    pub envelope: EventEnvelope,
    /// Durable server receipt.
    pub receipt: Receipt,
}

/// Signature over a complete pull page, including empty pages and completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageAttestation {
    /// Stable authority owning the signing key.
    pub authority_id: AuthorityId,
    /// Authoritative RFC3339 issuance time.
    pub issued_at: String,
    /// Signing key identifier.
    pub key_id: String,
    /// Detached base64url signature.
    pub signature: String,
}

impl PageAttestation {
    fn validate_shape(&self) -> Result<(), ValidationError> {
        validate_authority_id(self.authority_id.as_str())?;
        validate_timestamp(&self.issued_at)?;
        validate_key_id(&self.key_id)?;
        validate_signature(&self.signature)
    }

    /// Canonical bytes binding the request, fixed snapshot, result, and completion.
    pub fn signing_payload(
        &self,
        request: &PullRequest,
        page: &PullPage,
    ) -> Result<Vec<u8>, ValidationError> {
        self.validate_shape()?;
        request.validate()?;
        let mut payload = Vec::new();
        for field in [
            PAGE_DOMAIN,
            PROTOCOL_V2,
            self.authority_id.as_str(),
            request.request_id.as_str(),
            request.stream_id.as_str(),
        ] {
            append_field(&mut payload, field)?;
        }
        append_cursor(&mut payload, &request.after)?;
        match &request.snapshot {
            SnapshotSelection::Latest => append_field(&mut payload, "latest")?,
            SnapshotSelection::Exact(cursor) => {
                append_field(&mut payload, "exact")?;
                append_cursor(&mut payload, cursor)?;
            }
        }
        match request.selection {
            PullSelection::WholeStream => append_field(&mut payload, "whole_stream")?,
        }
        payload.extend_from_slice(&request.limit.to_be_bytes());
        append_cursor(&mut payload, &page.next_cursor)?;
        append_cursor(&mut payload, &page.snapshot_cursor)?;
        payload.push(u8::from(page.complete));
        let count = u64::try_from(page.events.len())
            .map_err(|_| ValidationError::new("events", "count overflow"))?;
        payload.extend_from_slice(&count.to_be_bytes());
        append_field(&mut payload, &accepted_events_hash(&page.events)?)?;
        append_field(&mut payload, &self.issued_at)?;
        append_field(&mut payload, &self.key_id)?;
        Ok(payload)
    }
}

/// Ordered raw response page for pull, recovery, and replay.
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
    /// Fixed terminal snapshot for the entire pull cycle.
    pub snapshot_cursor: StreamCursor,
    /// Must equal `next_cursor == snapshot_cursor`.
    pub complete: bool,
    /// Mandatory signature over the entire page, even when `events` is empty.
    pub attestation: PageAttestation,
}

impl PullPage {
    fn validate_against(&self, request: &PullRequest) -> Result<(), ValidationError> {
        request.validate()?;
        ensure_protocol(&self.protocol)?;
        if self.stream_id != request.stream_id {
            return Err(ValidationError::new("stream_id", "pull response mismatch"));
        }
        if self.events.len() > usize::from(request.limit) {
            return Err(ValidationError::new("events", "pull limit exceeded"));
        }
        self.next_cursor.validate()?;
        self.snapshot_cursor.validate()?;
        self.attestation.validate_shape()?;
        if self.snapshot_cursor.position < request.after.position {
            return Err(ValidationError::new(
                "snapshot_cursor",
                "snapshot regressed",
            ));
        }
        if let SnapshotSelection::Exact(expected) = &request.snapshot {
            if &self.snapshot_cursor != expected {
                return Err(ValidationError::new(
                    "snapshot_cursor",
                    "response changed fixed snapshot",
                ));
            }
        }
        if self.next_cursor.position > self.snapshot_cursor.position {
            return Err(ValidationError::new(
                "next_cursor",
                "page advanced beyond fixed snapshot",
            ));
        }
        if self.next_cursor.position == self.snapshot_cursor.position
            && self.next_cursor.head_hash != self.snapshot_cursor.head_hash
        {
            return Err(ValidationError::new(
                "snapshot_cursor",
                "same snapshot position has a different durable head",
            ));
        }
        let mut ids = BTreeSet::new();
        for (index, accepted) in self.events.iter().enumerate() {
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
            let offset = u64::try_from(index)
                .map_err(|_| ValidationError::new("events", "index overflow"))?;
            let expected_seq = request
                .after
                .position
                .checked_add(offset)
                .and_then(|position| position.checked_add(1))
                .ok_or(ValidationError::new("server_seq", "sequence overflow"))?;
            if accepted.receipt.server_seq != expected_seq {
                return Err(ValidationError::new(
                    "server_seq",
                    "pull sequence is not contiguous",
                ));
            }
            let expected_prev = if index == 0 {
                &request.after.head_hash
            } else {
                &self.events[index - 1].receipt.server_hash
            };
            if &accepted.receipt.server_prev_hash != expected_prev {
                return Err(ValidationError::new(
                    "receipts",
                    "pull hash chain is discontinuous",
                ));
            }
        }
        let count = u64::try_from(self.events.len())
            .map_err(|_| ValidationError::new("events", "count overflow"))?;
        let expected_position = request
            .after
            .position
            .checked_add(count)
            .ok_or(ValidationError::new("next_cursor", "position overflow"))?;
        if self.next_cursor.position != expected_position {
            return Err(ValidationError::new(
                "next_cursor",
                "position does not match returned page",
            ));
        }
        match self.events.last() {
            Some(last) if self.next_cursor.head_hash != last.receipt.server_hash => {
                return Err(ValidationError::new(
                    "next_cursor",
                    "head does not match final durable entry",
                ));
            }
            None if self.next_cursor != request.after => {
                return Err(ValidationError::new(
                    "next_cursor",
                    "empty page changed request cursor",
                ));
            }
            _ => {}
        }
        let derived_complete = self.next_cursor == self.snapshot_cursor;
        if self.complete != derived_complete {
            return Err(ValidationError::new(
                "complete",
                "must exactly reflect next cursor reaching fixed snapshot",
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

/// Unsupported event kind/version returned by application policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventVersionError {
    kind: String,
    schema_version: u32,
    reason: String,
}

impl EventVersionError {
    /// Build an application policy rejection.
    #[must_use]
    pub fn new(kind: impl Into<String>, schema_version: u32, reason: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            schema_version,
            reason: reason.into(),
        }
    }

    /// Rejected event kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Rejected schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

impl fmt::Display for EventVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unsupported event {}@{}: {}",
            self.kind, self.schema_version, self.reason
        )
    }
}

impl Error for EventVersionError {}

/// Application-owned fail-closed event schema policy.
pub trait EventVersionPolicy {
    /// Accept a known reducer input or reject unknown kind/version pairs.
    fn validate(&self, kind: &str, schema_version: u32) -> Result<(), EventVersionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplayAnchor {
    head_hash: String,
    event_identity: Option<(String, String)>,
}

const MAX_REPLAY_ANCHORS: usize = MAX_PULL_EVENTS as usize * 4 + 1;

/// Persistent replay bookkeeping bound to exactly one stream and authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayState {
    stream_id: String,
    authority: Option<AuthorityId>,
    cursor: StreamCursor,
    snapshot: Option<StreamCursor>,
    anchors: BTreeMap<u64, ReplayAnchor>,
}

impl ReplayState {
    /// Create state bound to one validated stream at explicit genesis.
    pub fn new(stream_id: impl Into<String>) -> Result<Self, ValidationError> {
        let stream_id = stream_id.into();
        validate_stream_id(&stream_id)?;
        let cursor = StreamCursor::genesis();
        let anchors = BTreeMap::from([(
            0,
            ReplayAnchor {
                head_hash: cursor.head_hash.clone(),
                event_identity: None,
            },
        )]);
        Ok(Self {
            stream_id,
            authority: None,
            cursor,
            snapshot: None,
            anchors,
        })
    }

    /// Bound stream identifier.
    #[must_use]
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }

    /// Authority established by the first applied page.
    #[must_use]
    pub const fn authority(&self) -> Option<&AuthorityId> {
        self.authority.as_ref()
    }

    /// Last authenticated durable cursor.
    #[must_use]
    pub const fn cursor(&self) -> &StreamCursor {
        &self.cursor
    }

    /// Fixed snapshot currently bound to the replay cycle.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&StreamCursor> {
        self.snapshot.as_ref()
    }

    /// Number of retained absolute-position anchors.
    #[must_use]
    pub fn dedupe_window_len(&self) -> usize {
        self.anchors
            .values()
            .filter(|anchor| anchor.event_identity.is_some())
            .count()
    }

    fn preflight(&self, request: &PullRequest, page: &PullPage) -> Result<(), ValidationError> {
        if request.stream_id != self.stream_id || page.stream_id != self.stream_id {
            return Err(ValidationError::new("stream_id", "replay state mismatch"));
        }
        if request.after.position > self.cursor.position {
            return Err(ValidationError::new(
                "after",
                "request skips local replay state",
            ));
        }
        let Some(anchor) = self.anchors.get(&request.after.position) else {
            return Err(ValidationError::new(
                "after",
                "cursor falls outside bounded replay window",
            ));
        };
        if anchor.head_hash != request.after.head_hash {
            return Err(ValidationError::new(
                "after",
                "cursor forks from replay state",
            ));
        }
        if page.snapshot_cursor.position < self.cursor.position {
            return Err(ValidationError::new(
                "snapshot_cursor",
                "snapshot regressed behind replay state",
            ));
        }
        if page.snapshot_cursor.position == self.cursor.position
            && page.snapshot_cursor.head_hash != self.cursor.head_hash
        {
            return Err(ValidationError::new(
                "snapshot_cursor",
                "snapshot forks at the current replay position",
            ));
        }
        if let Some(bound) = &self.snapshot {
            let cycle_active = self.cursor != *bound;
            match request.snapshot {
                SnapshotSelection::Exact(_) if page.snapshot_cursor != *bound => {
                    return Err(ValidationError::new(
                        "snapshot_cursor",
                        "exact snapshot differs from replay state",
                    ));
                }
                SnapshotSelection::Latest if cycle_active && page.snapshot_cursor != *bound => {
                    return Err(ValidationError::new(
                        "snapshot_cursor",
                        "active pull cycle changed snapshot",
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn prune_anchors(&mut self) {
        while self.anchors.len() > MAX_REPLAY_ANCHORS {
            let Some(oldest) = self.anchors.keys().next().copied() else {
                break;
            };
            if oldest == self.cursor.position {
                break;
            }
            self.anchors.remove(&oldest);
        }
    }
}

/// Raw request/page pair. Construction proves no trust property.
#[derive(Debug, Clone)]
pub struct UntrustedPullPage {
    request: PullRequest,
    page: PullPage,
}

impl UntrustedPullPage {
    /// Pair raw wire values before fail-closed validation.
    #[must_use]
    pub fn new(request: PullRequest, page: PullPage) -> Self {
        Self { request, page }
    }

    /// Validate shape, sequence, snapshot, stream, and current replay anchors.
    pub fn validate(self, state: &ReplayState) -> Result<ValidatedPullPage, SyncError> {
        self.page
            .validate_against(&self.request)
            .map_err(SyncError::Validation)?;
        state
            .preflight(&self.request, &self.page)
            .map_err(SyncError::Validation)?;
        Ok(ValidatedPullPage {
            request: self.request,
            page: self.page,
        })
    }
}

/// Structurally validated pull page. It is still cryptographically untrusted.
///
/// ```compile_fail
/// use pliego_hyphae::{ReplaySink, ReplayState, ValidatedPullPage};
/// fn bypass(mut page: ValidatedPullPage, state: &mut ReplayState, sink: &mut impl ReplaySink) {
///     let _ = page.apply(state, sink);
/// }
/// ```
#[derive(Debug)]
pub struct ValidatedPullPage {
    request: PullRequest,
    page: PullPage,
}

impl ValidatedPullPage {
    /// Verify page attestation, every receipt, one authority, and event policy.
    pub fn verify<V: ReceiptVerifier, P: EventVersionPolicy>(
        self,
        verifier: &V,
        policy: &P,
    ) -> Result<VerifiedPullPage, SyncError> {
        let claimed = &self.page.attestation.authority_id;
        let payload = self
            .page
            .attestation
            .signing_payload(&self.request, &self.page)
            .map_err(SyncError::Validation)?;
        let authority = verify_as(
            verifier,
            VerificationContext {
                purpose: SignaturePurpose::PageAttestation,
                claimed_authority: Some(claimed),
                stream_id: &self.request.stream_id,
                signed_at: &self.page.attestation.issued_at,
                key_id: &self.page.attestation.key_id,
            },
            &payload,
            &self.page.attestation.signature,
        )
        .map_err(SyncError::Verification)?;
        let mut verified_events = Vec::with_capacity(self.page.events.len());
        for accepted in &self.page.events {
            let receipt_payload = accepted
                .receipt
                .signing_payload()
                .map_err(SyncError::Validation)?;
            let receipt_authority = verify_as(
                verifier,
                VerificationContext {
                    purpose: SignaturePurpose::Receipt,
                    claimed_authority: Some(&authority),
                    stream_id: &accepted.receipt.stream_id,
                    signed_at: &accepted.receipt.committed_at,
                    key_id: &accepted.receipt.key_id,
                },
                &receipt_payload,
                &accepted.receipt.signature,
            )
            .map_err(SyncError::Verification)?;
            if receipt_authority != authority {
                return Err(SyncError::Verification(
                    VerificationError::AuthorityMismatch,
                ));
            }
            policy
                .validate(&accepted.envelope.kind, accepted.envelope.schema_version)
                .map_err(SyncError::EventVersion)?;
            verified_events.push(VerifiedAcceptedEvent {
                envelope: accepted.envelope.clone(),
                receipt: accepted.receipt.clone(),
                authority: authority.clone(),
            });
        }
        Ok(VerifiedPullPage {
            request: self.request,
            page: self.page,
            authority,
            events: verified_events,
        })
    }
}

/// One authenticated event that passed application version policy.
#[derive(Debug, Clone)]
pub struct VerifiedAcceptedEvent {
    envelope: EventEnvelope,
    receipt: Receipt,
    authority: AuthorityId,
}

impl VerifiedAcceptedEvent {
    /// Authenticated application event.
    #[must_use]
    pub const fn envelope(&self) -> &EventEnvelope {
        &self.envelope
    }

    /// Authenticated durable receipt.
    #[must_use]
    pub const fn receipt(&self) -> &Receipt {
        &self.receipt
    }

    /// Authority that authenticated this event and its enclosing page.
    #[must_use]
    pub const fn authority(&self) -> &AuthorityId {
        &self.authority
    }
}

/// Application reducer boundary. Raw [`AcceptedEvent`] values cannot enter it.
///
/// ```compile_fail
/// use pliego_hyphae::{AcceptedEvent, ReplaySink};
/// fn raw_event_cannot_reach_sink(raw: AcceptedEvent, sink: &mut impl ReplaySink) {
///     let _ = sink.apply_batch(&[raw]);
/// }
/// ```
pub trait ReplaySink {
    /// Atomically apply previously unseen, authenticated events.
    /// Returning an error must leave the application reducer unchanged.
    fn apply_batch(&mut self, events: &[VerifiedAcceptedEvent]) -> Result<(), String>;
}

/// Fully authenticated pull page. Private fields prevent construction by callers.
///
/// ```compile_fail
/// use pliego_hyphae::{PullPage, ReplaySink, ReplayState};
/// fn raw_cannot_apply(raw: PullPage, state: &mut ReplayState, sink: &mut impl ReplaySink) {
///     let _ = raw.apply(state, sink);
/// }
/// ```
///
/// ```compile_fail
/// use pliego_hyphae::VerifiedPullPage;
/// fn cannot_forge() {
///     let _ = VerifiedPullPage { request: todo!(), page: todo!(), authority: todo!(), events: vec![] };
/// }
/// ```
#[derive(Debug)]
pub struct VerifiedPullPage {
    request: PullRequest,
    page: PullPage,
    authority: AuthorityId,
    events: Vec<VerifiedAcceptedEvent>,
}

impl VerifiedPullPage {
    /// Apply once. All state, fork, overlap, and authority checks precede the sink.
    pub fn apply<S: ReplaySink>(
        self,
        state: &mut ReplayState,
        sink: &mut S,
    ) -> Result<AppliedPullPage, SyncError> {
        state
            .preflight(&self.request, &self.page)
            .map_err(SyncError::Validation)?;
        if state
            .authority
            .as_ref()
            .is_some_and(|bound| bound != &self.authority)
        {
            return Err(SyncError::Verification(
                VerificationError::AuthorityMismatch,
            ));
        }

        let mut candidate = state.clone();
        candidate.authority = Some(self.authority.clone());
        candidate.snapshot = Some(self.page.snapshot_cursor.clone());
        let current_position = state.cursor.position;
        let mut fresh = Vec::new();
        for (index, event) in self.events.iter().enumerate() {
            let offset = u64::try_from(index).map_err(|_| {
                SyncError::Validation(ValidationError::new("events", "index overflow"))
            })?;
            let position = self
                .request
                .after
                .position
                .checked_add(offset)
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    SyncError::Validation(ValidationError::new("events", "position overflow"))
                })?;
            if position <= current_position {
                let Some(anchor) = state.anchors.get(&position) else {
                    return Err(SyncError::Validation(ValidationError::new(
                        "events",
                        "overlap falls outside bounded replay window",
                    )));
                };
                let expected = Some((
                    event.envelope.client_event_id.clone(),
                    event.receipt.server_hash.clone(),
                ));
                if anchor.head_hash != event.receipt.server_hash
                    || anchor.event_identity != expected
                {
                    return Err(SyncError::Validation(ValidationError::new(
                        "events",
                        "overlap changed event identity or durable hash",
                    )));
                }
                continue;
            }
            let fresh_offset = u64::try_from(fresh.len()).map_err(|_| {
                SyncError::Validation(ValidationError::new("events", "count overflow"))
            })?;
            let expected_position = current_position
                .checked_add(fresh_offset)
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    SyncError::Validation(ValidationError::new("events", "position overflow"))
                })?;
            if position != expected_position {
                return Err(SyncError::Validation(ValidationError::new(
                    "events",
                    "fresh replay contains a position gap",
                )));
            }
            if candidate.anchors.values().any(|anchor| {
                anchor
                    .event_identity
                    .as_ref()
                    .is_some_and(|(id, _)| id == &event.envelope.client_event_id)
            }) {
                return Err(SyncError::Validation(ValidationError::new(
                    "client_event_id",
                    "event identity moved to another stream position",
                )));
            }
            candidate.anchors.insert(
                position,
                ReplayAnchor {
                    head_hash: event.receipt.server_hash.clone(),
                    event_identity: Some((
                        event.envelope.client_event_id.clone(),
                        event.receipt.server_hash.clone(),
                    )),
                },
            );
            fresh.push(event.clone());
        }
        if self.page.next_cursor.position > current_position {
            candidate.cursor = self.page.next_cursor.clone();
        } else if self.page.next_cursor.position == current_position
            && self.page.next_cursor.head_hash != state.cursor.head_hash
        {
            return Err(SyncError::Validation(ValidationError::new(
                "next_cursor",
                "same position changed durable head",
            )));
        }
        candidate.prune_anchors();

        let applied_count = u64::try_from(fresh.len())
            .map_err(|_| SyncError::Validation(ValidationError::new("events", "count overflow")))?;
        let applied_cursor = candidate.cursor.clone();
        let snapshot_cursor = self.page.snapshot_cursor.clone();
        let complete = applied_cursor == snapshot_cursor;
        if !fresh.is_empty() {
            sink.apply_batch(&fresh).map_err(SyncError::Reducer)?;
        }
        *state = candidate;
        Ok(AppliedPullPage {
            applied_count,
            cursor: applied_cursor,
            complete,
            snapshot_cursor,
            authority: self.authority,
        })
    }
}

/// Consumed result of one authenticated replay application.
///
/// ```compile_fail
/// use pliego_hyphae::{AppliedPullPage, ReplaySink, ReplayState};
/// fn cannot_apply_twice(page: AppliedPullPage, state: &mut ReplayState, sink: &mut impl ReplaySink) {
///     let _ = page.apply(state, sink);
/// }
/// ```
///
/// ```compile_fail
/// use pliego_hyphae::{ReplaySink, ReplayState, VerifiedPullPage};
/// fn verified_is_consumed(page: VerifiedPullPage, state: &mut ReplayState, sink: &mut impl ReplaySink) {
///     let _first = page.apply(state, sink);
///     let _second = page.apply(state, sink);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AppliedPullPage {
    applied_count: u64,
    cursor: StreamCursor,
    snapshot_cursor: StreamCursor,
    complete: bool,
    authority: AuthorityId,
}

impl AppliedPullPage {
    /// Number of fresh events sent to the reducer.
    #[must_use]
    pub const fn applied_count(&self) -> u64 {
        self.applied_count
    }

    /// Cursor after this application.
    #[must_use]
    pub const fn cursor(&self) -> &StreamCursor {
        &self.cursor
    }

    /// Fixed snapshot for continuation requests.
    #[must_use]
    pub const fn snapshot_cursor(&self) -> &StreamCursor {
        &self.snapshot_cursor
    }

    /// Whether this application reached the fixed snapshot.
    #[must_use]
    pub const fn complete(&self) -> bool {
        self.complete
    }

    /// Authority established for this stream.
    #[must_use]
    pub const fn authority(&self) -> &AuthorityId {
        &self.authority
    }
}

// ───────────────────────── legacy compatibility seam ─────────────────────────

/// A legacy server acknowledgment: where one event landed on a durable chain.
#[cfg(feature = "experimental-legacy")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    /// Sequence on the Hyphae journal.
    pub seq: u64,
    /// Lowercase SHA-256 hex hash of the durable entry.
    pub hash: String,
}

#[cfg(feature = "experimental-legacy")]
impl Ack {
    /// Validate the acknowledgement's untrusted shape.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_hash(&self.hash)
    }
}

/// Tracks which local events have legacy durable acknowledgments.
#[cfg(feature = "experimental-legacy")]
#[derive(Debug, Default)]
pub struct SyncState {
    acks: Vec<Option<Ack>>, // indexed by local seq
    next_to_push: u64,
}

#[cfg(feature = "experimental-legacy")]
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
#[cfg(feature = "experimental-legacy")]
pub trait JournalTransport {
    /// Append one event; return where it landed on the durable chain.
    fn append(&mut self, kind: &str, payload: &str) -> Result<Ack, String>;
}

/// Push everything pending through a legacy one-event transport.
///
/// This function validates the local chain, event kind, payload bound, and
/// acknowledgement hash. It cannot make a lost acknowledgement idempotent;
/// use [`append_with_retry`] with [`BatchTransport`] for that guarantee.
#[cfg(feature = "experimental-legacy")]
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

#[cfg(all(target_arch = "wasm32", feature = "experimental-legacy"))]
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
mod r2_tests;
