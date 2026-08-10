// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use pliego_log::{Hash, hex};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use sha2::{Digest, Sha256};

use crate::ProjectionSnapshot;

/// Domain separation for the first projection state-root contract.
pub const STATE_ROOT_DOMAIN_V1: &[u8] = b"pliego-fold/state-root/1\0";
/// Current state-root contract version.
pub const STATE_ROOT_FORMAT_V1: u16 = 1;

/// Deterministic identity of one verified projection state and its contracts.
///
/// A state root is integrity evidence, not authority, signature, or provenance.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StateRoot {
    format: u16,
    digest: Hash,
}

impl StateRoot {
    /// Derive a versioned root from an integrity-checked projection snapshot.
    #[must_use]
    pub fn from_snapshot(snapshot: &ProjectionSnapshot) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(STATE_ROOT_DOMAIN_V1);
        hasher.update(snapshot.format().to_be_bytes());
        hasher.update(snapshot.history().position.to_be_bytes());
        hasher.update(snapshot.history().head_hash);
        hasher.update(snapshot.schema_set_digest());
        push_identity(
            &mut hasher,
            snapshot.reducer().id(),
            snapshot.reducer().revision(),
            snapshot.reducer().config_hash(),
        );
        push_identity(
            &mut hasher,
            snapshot.codec().id(),
            snapshot.codec().revision(),
            snapshot.codec().config_hash(),
        );
        hasher.update(snapshot.state_digest());
        hasher.update(snapshot.snapshot_digest());
        Self {
            format: STATE_ROOT_FORMAT_V1,
            digest: hasher.finalize().into(),
        }
    }

    /// State-root contract revision.
    #[must_use]
    pub const fn format(&self) -> u16 {
        self.format
    }

    /// Exact SHA-256 bytes under the versioned state-root domain.
    #[must_use]
    pub const fn digest(&self) -> &Hash {
        &self.digest
    }

    /// Algorithm-tagged portable spelling.
    #[must_use]
    pub fn tagged(&self) -> String {
        format!("sha256:{}", hex(&self.digest))
    }
}

impl fmt::Display for StateRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.tagged())
    }
}

impl Serialize for StateRoot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("StateRoot", 2)?;
        state.serialize_field("format", &self.format)?;
        state.serialize_field("value", &self.tagged())?;
        state.end()
    }
}

fn push_identity(hasher: &mut Sha256, id: &str, revision: u64, config_hash: &Hash) {
    let length = u16::try_from(id.len()).expect("validated snapshot contract ID fits in u16");
    hasher.update(length.to_be_bytes());
    hasher.update(id.as_bytes());
    hasher.update(revision.to_be_bytes());
    hasher.update(config_hash);
}
