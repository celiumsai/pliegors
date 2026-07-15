// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Bounded projection snapshot envelopes with self-verifying integrity.

use std::error::Error;
use std::fmt;

use pliego_log::{Hash, LogCursor};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::codec::{MAX_CANONICAL_STATE_BYTES, encode_canonical_json};

const SNAPSHOT_MAGIC: &[u8; 8] = b"PLGSNAP\0";
const SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"pliego-fold/snapshot/1\0";
/// Current projection-snapshot wire format.
pub const SNAPSHOT_FORMAT_V1: u16 = 1;
/// Maximum reducer or codec identifier length.
pub const MAX_CONTRACT_ID_BYTES: usize = 192;
/// Maximum complete encoded envelope size.
pub const MAX_PROJECTION_SNAPSHOT_BYTES: usize =
    MAX_CANONICAL_STATE_BYTES + (2 * MAX_CONTRACT_ID_BYTES) + 256;

/// Stable identity of the pure reducer that materializes a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducerIdentity {
    id: String,
    revision: u64,
    config_hash: Hash,
}

impl ReducerIdentity {
    /// Build a validated reducer identity. Configuration changes require a new
    /// `config_hash`; code or semantic changes require a new `revision`.
    pub fn new(
        id: impl Into<String>,
        revision: u64,
        config_hash: Hash,
    ) -> Result<Self, SnapshotError> {
        let id = id.into();
        validate_contract_id("reducer", &id)?;
        Ok(Self {
            id,
            revision,
            config_hash,
        })
    }

    /// Canonicalize a serializable reducer configuration and bind its SHA-256
    /// digest directly, avoiding an ambiguous caller-defined byte encoding.
    pub fn from_serializable_config<T: Serialize>(
        id: impl Into<String>,
        revision: u64,
        config: &T,
    ) -> Result<Self, SnapshotError> {
        let bytes = encode_canonical_json(config)
            .map_err(|error| SnapshotError::ReducerConfig(error.to_string()))?;
        Self::new(id, revision, digest(&bytes))
    }

    /// Stable reducer identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Reducer implementation/semantic revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Digest of reducer configuration that affects materialized state.
    #[must_use]
    pub const fn config_hash(&self) -> &Hash {
        &self.config_hash
    }
}

/// Structural or integrity failure while reading a snapshot envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    TooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidMagic,
    UnsupportedFormat(u16),
    Truncated(&'static str),
    TrailingBytes(usize),
    InvalidUtf8(&'static str),
    InvalidContractId {
        field: &'static str,
        reason: &'static str,
    },
    ReducerConfig(String),
    StateTooLarge {
        actual: usize,
        maximum: usize,
    },
    StateDigestMismatch,
    SnapshotDigestMismatch,
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { actual, maximum } => {
                write!(f, "snapshot is {actual} bytes; limit is {maximum}")
            }
            Self::InvalidMagic => f.write_str("invalid projection snapshot magic"),
            Self::UnsupportedFormat(format) => {
                write!(f, "unsupported projection snapshot format {format}")
            }
            Self::Truncated(field) => write!(f, "projection snapshot is truncated at {field}"),
            Self::TrailingBytes(count) => {
                write!(f, "projection snapshot has {count} trailing bytes")
            }
            Self::InvalidUtf8(field) => write!(f, "snapshot {field} is not valid UTF-8"),
            Self::InvalidContractId { field, reason } => {
                write!(f, "invalid snapshot {field} identifier: {reason}")
            }
            Self::ReducerConfig(message) => {
                write!(f, "reducer configuration encoding failed: {message}")
            }
            Self::StateTooLarge { actual, maximum } => {
                write!(f, "snapshot state is {actual} bytes; limit is {maximum}")
            }
            Self::StateDigestMismatch => f.write_str("snapshot state digest mismatch"),
            Self::SnapshotDigestMismatch => f.write_str("snapshot envelope digest mismatch"),
        }
    }
}

impl Error for SnapshotError {}

/// A projection checkpoint bound to history, schemas, reducer, codec, and state.
///
/// Fields are private so untrusted callers cannot assemble a value that skipped
/// wire bounds or digest verification. Use [`ProjectionSnapshot::decode`] for
/// bytes received from storage or the network. These unkeyed digests detect
/// corruption and contract mismatch; they do not prove publisher authority or
/// authenticity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionSnapshot {
    format: u16,
    history: LogCursor,
    schema_set_digest: Hash,
    reducer: ReducerIdentity,
    codec_id: String,
    state_bytes: Vec<u8>,
    state_digest: Hash,
    snapshot_digest: Hash,
}

impl ProjectionSnapshot {
    pub(crate) fn create(
        history: LogCursor,
        schema_set_digest: Hash,
        reducer: ReducerIdentity,
        codec_id: impl Into<String>,
        state_bytes: Vec<u8>,
    ) -> Result<Self, SnapshotError> {
        let codec_id = codec_id.into();
        validate_contract_id("codec", &codec_id)?;
        if state_bytes.len() > MAX_CANONICAL_STATE_BYTES {
            return Err(SnapshotError::StateTooLarge {
                actual: state_bytes.len(),
                maximum: MAX_CANONICAL_STATE_BYTES,
            });
        }
        let state_digest = digest(&state_bytes);
        let mut snapshot = Self {
            format: SNAPSHOT_FORMAT_V1,
            history,
            schema_set_digest,
            reducer,
            codec_id,
            state_bytes,
            state_digest,
            snapshot_digest: [0; 32],
        };
        snapshot.snapshot_digest = snapshot.compute_snapshot_digest();
        Ok(snapshot)
    }

    /// Decode an untrusted envelope with bounds checked before every allocation.
    pub fn decode(bytes: &[u8]) -> Result<Self, SnapshotError> {
        if bytes.len() > MAX_PROJECTION_SNAPSHOT_BYTES {
            return Err(SnapshotError::TooLarge {
                actual: bytes.len(),
                maximum: MAX_PROJECTION_SNAPSHOT_BYTES,
            });
        }
        let mut decoder = Decoder::new(bytes);
        if decoder.take(SNAPSHOT_MAGIC.len(), "magic")? != SNAPSHOT_MAGIC {
            return Err(SnapshotError::InvalidMagic);
        }
        let format = decoder.u16("format")?;
        if format != SNAPSHOT_FORMAT_V1 {
            return Err(SnapshotError::UnsupportedFormat(format));
        }
        let history = LogCursor {
            position: decoder.u64("history position")?,
            head_hash: decoder.hash("history head")?,
        };
        let schema_set_digest = decoder.hash("schema-set digest")?;
        let reducer_id = decoder.string("reducer id", "reducer")?;
        let reducer_revision = decoder.u64("reducer revision")?;
        let reducer_config_hash = decoder.hash("reducer config hash")?;
        let codec_id = decoder.string("codec id", "codec")?;
        let state_len = decoder.u32("state length")? as usize;
        if state_len > MAX_CANONICAL_STATE_BYTES {
            return Err(SnapshotError::StateTooLarge {
                actual: state_len,
                maximum: MAX_CANONICAL_STATE_BYTES,
            });
        }
        let state_bytes = decoder.take(state_len, "state bytes")?.to_vec();
        let state_digest = decoder.hash("state digest")?;
        let snapshot_digest = decoder.hash("snapshot digest")?;
        if decoder.remaining() != 0 {
            return Err(SnapshotError::TrailingBytes(decoder.remaining()));
        }
        let reducer = ReducerIdentity::new(reducer_id, reducer_revision, reducer_config_hash)?;
        validate_contract_id("codec", &codec_id)?;
        let snapshot = Self {
            format,
            history,
            schema_set_digest,
            reducer,
            codec_id,
            state_bytes,
            state_digest,
            snapshot_digest,
        };
        snapshot.verify_integrity()?;
        Ok(snapshot)
    }

    /// Encode the canonical envelope. Values can only originate from a verified
    /// decode or from a live projection, so this operation is infallible.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            8 + 2
                + 8
                + 32
                + 32
                + 2
                + self.reducer.id.len()
                + 8
                + 32
                + 2
                + self.codec_id.len()
                + 4
                + self.state_bytes.len()
                + 64,
        );
        bytes.extend_from_slice(SNAPSHOT_MAGIC);
        bytes.extend_from_slice(&self.format.to_be_bytes());
        bytes.extend_from_slice(&self.history.position.to_be_bytes());
        bytes.extend_from_slice(&self.history.head_hash);
        bytes.extend_from_slice(&self.schema_set_digest);
        push_string(&mut bytes, &self.reducer.id);
        bytes.extend_from_slice(&self.reducer.revision.to_be_bytes());
        bytes.extend_from_slice(&self.reducer.config_hash);
        push_string(&mut bytes, &self.codec_id);
        bytes.extend_from_slice(&(self.state_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.state_bytes);
        bytes.extend_from_slice(&self.state_digest);
        bytes.extend_from_slice(&self.snapshot_digest);
        bytes
    }

    /// Snapshot wire-format revision.
    #[must_use]
    pub const fn format(&self) -> u16 {
        self.format
    }

    /// Exact history position and head folded into this state.
    #[must_use]
    pub const fn history(&self) -> &LogCursor {
        &self.history
    }

    /// Digest of the exact accepted event schema and upcaster graph.
    #[must_use]
    pub const fn schema_set_digest(&self) -> &Hash {
        &self.schema_set_digest
    }

    /// Reducer identity and semantic revision.
    #[must_use]
    pub const fn reducer(&self) -> &ReducerIdentity {
        &self.reducer
    }

    /// Stable state codec identifier.
    #[must_use]
    pub fn codec_id(&self) -> &str {
        &self.codec_id
    }

    /// Number of canonical state bytes in the snapshot.
    #[must_use]
    pub fn state_len(&self) -> usize {
        self.state_bytes.len()
    }

    /// Digest of canonical state bytes.
    #[must_use]
    pub const fn state_digest(&self) -> &Hash {
        &self.state_digest
    }

    /// Digest binding the complete snapshot envelope.
    #[must_use]
    pub const fn snapshot_digest(&self) -> &Hash {
        &self.snapshot_digest
    }

    pub(crate) fn state_bytes(&self) -> &[u8] {
        &self.state_bytes
    }

    fn verify_integrity(&self) -> Result<(), SnapshotError> {
        if digest(&self.state_bytes) != self.state_digest {
            return Err(SnapshotError::StateDigestMismatch);
        }
        if self.compute_snapshot_digest() != self.snapshot_digest {
            return Err(SnapshotError::SnapshotDigestMismatch);
        }
        Ok(())
    }

    fn compute_snapshot_digest(&self) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(SNAPSHOT_DIGEST_DOMAIN);
        hasher.update(self.format.to_be_bytes());
        hasher.update(self.history.position.to_be_bytes());
        hasher.update(self.history.head_hash);
        hasher.update(self.schema_set_digest);
        update_len_prefixed(&mut hasher, self.reducer.id.as_bytes());
        hasher.update(self.reducer.revision.to_be_bytes());
        hasher.update(self.reducer.config_hash);
        update_len_prefixed(&mut hasher, self.codec_id.as_bytes());
        update_len_prefixed(&mut hasher, &self.state_bytes);
        hasher.update(self.state_digest);
        hasher.finalize().into()
    }
}

pub(crate) fn validate_contract_id(field: &'static str, value: &str) -> Result<(), SnapshotError> {
    if value.is_empty() {
        return Err(SnapshotError::InvalidContractId {
            field,
            reason: "must not be empty",
        });
    }
    if value.len() > MAX_CONTRACT_ID_BYTES {
        return Err(SnapshotError::InvalidContractId {
            field,
            reason: "too long",
        });
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
    }) {
        return Err(SnapshotError::InvalidContractId {
            field,
            reason: "contains a non-portable character",
        });
    }
    Ok(())
}

fn push_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn update_len_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn digest(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], SnapshotError> {
        let end = self
            .offset
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(SnapshotError::Truncated(field))?;
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, SnapshotError> {
        let bytes: [u8; 2] = self.take(2, field)?.try_into().expect("length checked");
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, SnapshotError> {
        let bytes: [u8; 4] = self.take(4, field)?.try_into().expect("length checked");
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self, field: &'static str) -> Result<u64, SnapshotError> {
        let bytes: [u8; 8] = self.take(8, field)?.try_into().expect("length checked");
        Ok(u64::from_be_bytes(bytes))
    }

    fn hash(&mut self, field: &'static str) -> Result<Hash, SnapshotError> {
        Ok(self.take(32, field)?.try_into().expect("length checked"))
    }

    fn string(
        &mut self,
        length_field: &'static str,
        value_field: &'static str,
    ) -> Result<String, SnapshotError> {
        let len = self.u16(length_field)? as usize;
        if len > MAX_CONTRACT_ID_BYTES {
            return Err(SnapshotError::InvalidContractId {
                field: value_field,
                reason: "too long",
            });
        }
        let bytes = self.take(len, value_field)?;
        let value =
            std::str::from_utf8(bytes).map_err(|_| SnapshotError::InvalidUtf8(value_field))?;
        Ok(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProjectionSnapshot {
        ProjectionSnapshot::create(
            LogCursor {
                position: 3,
                head_hash: [4; 32],
            },
            [5; 32],
            ReducerIdentity::new("tasks", 7, [6; 32]).unwrap(),
            "test/json/1",
            br#"{"count":3}"#.to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn envelope_round_trips_exactly() {
        let snapshot = sample();
        let bytes = snapshot.encode();
        let decoded = ProjectionSnapshot::decode(&bytes).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.encode(), bytes);
    }

    #[test]
    fn envelope_and_state_digests_match_golden_vectors() {
        let snapshot = sample();
        assert_eq!(
            pliego_log::hex(snapshot.state_digest()),
            "0c07187ea6d064441225b3cba26a7b1e8bc702fcf332b457dae8e26892ba68a6"
        );
        assert_eq!(
            pliego_log::hex(snapshot.snapshot_digest()),
            "ed9d191e4f8b6451a54402931d52190ea5992ea08cf6c3fa2266dd03ab73d77f"
        );
    }

    #[test]
    fn envelope_rejects_corruption_truncation_and_trailing_data() {
        let bytes = sample().encode();
        for cut in 0..bytes.len() {
            assert!(
                ProjectionSnapshot::decode(&bytes[..cut]).is_err(),
                "cut={cut}"
            );
        }
        let mut corrupted = bytes.clone();
        let state_offset = corrupted
            .windows(br#"{"count":3}"#.len())
            .position(|window| window == br#"{"count":3}"#)
            .unwrap();
        corrupted[state_offset] ^= 1;
        assert_eq!(
            ProjectionSnapshot::decode(&corrupted),
            Err(SnapshotError::StateDigestMismatch)
        );
        let mut trailing = bytes;
        trailing.push(0);
        assert_eq!(
            ProjectionSnapshot::decode(&trailing),
            Err(SnapshotError::TrailingBytes(1))
        );
    }
}
