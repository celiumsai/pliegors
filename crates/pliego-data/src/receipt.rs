// SPDX-License-Identifier: Apache-2.0

use crate::DataCancelReason;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataOperation {
    ResourceLease,
    Loader,
    Action,
    Session,
    OutboundHttp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataOutcome {
    Success,
    Rejected,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataDurationBucket {
    Under1Millisecond,
    Under10Milliseconds,
    Under50Milliseconds,
    Under250Milliseconds,
    AtLeast250Milliseconds,
}

impl DataDurationBucket {
    pub(crate) fn from_duration(duration: Duration) -> Self {
        if duration < Duration::from_millis(1) {
            Self::Under1Millisecond
        } else if duration < Duration::from_millis(10) {
            Self::Under10Milliseconds
        } else if duration < Duration::from_millis(50) {
            Self::Under50Milliseconds
        } else if duration < Duration::from_millis(250) {
            Self::Under250Milliseconds
        } else {
            Self::AtLeast250Milliseconds
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataSizeBucket {
    None,
    Under1Kibibyte,
    Under16Kibibytes,
    Under256Kibibytes,
    AtLeast256Kibibytes,
}

impl DataSizeBucket {
    pub(crate) fn from_bytes(bytes: usize) -> Self {
        match bytes {
            0 => Self::None,
            1..=1_023 => Self::Under1Kibibyte,
            1_024..=16_383 => Self::Under16Kibibytes,
            16_384..=262_143 => Self::Under256Kibibytes,
            _ => Self::AtLeast256Kibibytes,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataReceipt {
    pub contract: String,
    pub operation: DataOperation,
    pub operation_id: String,
    pub semantic_revision: u32,
    pub outcome: DataOutcome,
    pub duration_bucket: DataDurationBucket,
    pub output_size_bucket: DataSizeBucket,
    pub deduplicated: bool,
    pub cancel_reason: Option<DataCancelReason>,
    pub diagnostic_code: Option<String>,
}
