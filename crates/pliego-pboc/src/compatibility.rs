// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

use crate::{PbocError, PbocManifest, validate_manifest};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatibilityDirection {
    Rolling,
    Rollback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityReceipt {
    pub contract: String,
    pub direction: CompatibilityDirection,
    pub application_id: String,
    pub from_release_id: String,
    pub to_release_id: String,
    pub epoch: u32,
    pub state_schema: String,
    pub from_sequence: u64,
    pub to_sequence: u64,
}

#[derive(Debug)]
pub enum CompatibilityError {
    InvalidManifest(PbocError),
    ApplicationMismatch,
    EpochMismatch,
    StateSchemaMismatch,
    SequenceMismatch,
    ReleaseChainMismatch,
    RollbackUnsafe,
}

impl CompatibilityError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidManifest(_) => "PLG-PBOC-100",
            Self::ApplicationMismatch => "PLG-PBOC-101",
            Self::EpochMismatch => "PLG-PBOC-102",
            Self::StateSchemaMismatch => "PLG-PBOC-103",
            Self::SequenceMismatch => "PLG-PBOC-104",
            Self::ReleaseChainMismatch => "PLG-PBOC-105",
            Self::RollbackUnsafe => "PLG-PBOC-106",
        }
    }
}

impl Display for CompatibilityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ", self.code())?;
        match self {
            Self::InvalidManifest(error) => Display::fmt(error, formatter),
            Self::ApplicationMismatch => formatter.write_str("application identities differ"),
            Self::EpochMismatch => formatter.write_str("compatibility epochs differ"),
            Self::StateSchemaMismatch => formatter.write_str("state schemas differ"),
            Self::SequenceMismatch => {
                formatter.write_str("release sequences do not move in the required direction")
            }
            Self::ReleaseChainMismatch => {
                formatter.write_str("candidate does not name the active release as previous")
            }
            Self::RollbackUnsafe => {
                formatter.write_str("active release does not declare rollback safe")
            }
        }
    }
}

impl std::error::Error for CompatibilityError {}

impl From<PbocError> for CompatibilityError {
    fn from(value: PbocError) -> Self {
        Self::InvalidManifest(value)
    }
}

pub fn verify_rolling_transition(
    active: &PbocManifest,
    candidate: &PbocManifest,
) -> Result<CompatibilityReceipt, CompatibilityError> {
    validate_pair(active, candidate)?;
    if candidate.compatibility.sequence <= active.compatibility.sequence {
        return Err(CompatibilityError::SequenceMismatch);
    }
    if candidate.compatibility.previous_release_id.as_deref()
        != Some(active.build.release_id.as_str())
    {
        return Err(CompatibilityError::ReleaseChainMismatch);
    }
    Ok(receipt(CompatibilityDirection::Rolling, active, candidate))
}

pub fn verify_rollback_transition(
    active: &PbocManifest,
    target: &PbocManifest,
) -> Result<CompatibilityReceipt, CompatibilityError> {
    validate_pair(active, target)?;
    if !active.compatibility.rollback_safe {
        return Err(CompatibilityError::RollbackUnsafe);
    }
    if target.compatibility.sequence >= active.compatibility.sequence {
        return Err(CompatibilityError::SequenceMismatch);
    }
    if active.compatibility.previous_release_id.as_deref() != Some(target.build.release_id.as_str())
    {
        return Err(CompatibilityError::ReleaseChainMismatch);
    }
    Ok(receipt(CompatibilityDirection::Rollback, active, target))
}

fn validate_pair(
    active: &PbocManifest,
    candidate: &PbocManifest,
) -> Result<(), CompatibilityError> {
    validate_manifest(active)?;
    validate_manifest(candidate)?;
    if active.build.application_id != candidate.build.application_id {
        return Err(CompatibilityError::ApplicationMismatch);
    }
    if active.compatibility.epoch != candidate.compatibility.epoch {
        return Err(CompatibilityError::EpochMismatch);
    }
    if active.compatibility.state_schema != candidate.compatibility.state_schema {
        return Err(CompatibilityError::StateSchemaMismatch);
    }
    Ok(())
}

fn receipt(
    direction: CompatibilityDirection,
    from: &PbocManifest,
    to: &PbocManifest,
) -> CompatibilityReceipt {
    CompatibilityReceipt {
        contract: "dev.pliegors.pboc-compatibility/v1".to_owned(),
        direction,
        application_id: from.build.application_id.clone(),
        from_release_id: from.build.release_id.clone(),
        to_release_id: to.build.release_id.clone(),
        epoch: from.compatibility.epoch,
        state_schema: from.compatibility.state_schema.clone(),
        from_sequence: from.compatibility.sequence,
        to_sequence: to.compatibility.sequence,
    }
}
