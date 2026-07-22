// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataError {
    InvalidStableId(String),
    DuplicateResource(String),
    ResourceUnavailable(String),
    ResourceTypeMismatch(String),
    MissingCapability {
        resource: String,
        capability: String,
    },
    ContextClosed,
    RequestValues(String),
    PolicyNotGranted(String),
    Cancelled,
    Deadline,
    InvalidLoaderPolicy(String),
    LoaderInput(String),
    LoaderOutput {
        actual: usize,
        maximum: usize,
    },
    LoaderFailure(String),
    ActionAdmission(String),
    ActionInput(String),
    ActionOutput {
        actual: usize,
        maximum: usize,
    },
    InvalidActionState(String),
    ActionOutcomeUnknown,
    ActionIdempotencyConflict,
    ActionInProgress,
    ActionFailure(String),
    Serialization,
    CleanupLimit(usize),
}

impl DataError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidStableId(_) => "PLG-DAT-001",
            Self::DuplicateResource(_) => "PLG-DAT-002",
            Self::ResourceUnavailable(_) => "PLG-DAT-101",
            Self::ResourceTypeMismatch(_) => "PLG-DAT-102",
            Self::MissingCapability { .. } => "PLG-DAT-103",
            Self::ContextClosed => "PLG-DAT-104",
            Self::RequestValues(_) => "PLG-DAT-105",
            Self::PolicyNotGranted(_) => "PLG-DAT-106",
            Self::InvalidLoaderPolicy(_) => "PLG-DAT-200",
            Self::LoaderInput(_) => "PLG-DAT-201",
            Self::LoaderOutput { .. } => "PLG-DAT-202",
            Self::Cancelled | Self::Deadline => "PLG-DAT-408",
            Self::LoaderFailure(_) | Self::Serialization => "PLG-DAT-500",
            Self::ActionAdmission(_) => "PLG-ACT-101",
            Self::ActionInput(_) => "PLG-ACT-201",
            Self::ActionOutput { .. } => "PLG-ACT-202",
            Self::InvalidActionState(_) => "PLG-ACT-301",
            Self::ActionOutcomeUnknown => "PLG-ACT-409",
            Self::ActionIdempotencyConflict => "PLG-ACT-409",
            Self::ActionInProgress => "PLG-ACT-425",
            Self::ActionFailure(_) => "PLG-ACT-500",
            Self::CleanupLimit(_) => "PLG-DAT-501",
        }
    }
}

impl Display for DataError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStableId(value) => write!(formatter, "invalid stable data ID {value:?}"),
            Self::DuplicateResource(value) => write!(formatter, "duplicate resource {value}"),
            Self::ResourceUnavailable(value) => {
                write!(formatter, "resource {value} is unavailable")
            }
            Self::ResourceTypeMismatch(value) => {
                write!(formatter, "resource {value} has a different concrete type")
            }
            Self::MissingCapability {
                resource,
                capability,
            } => write!(
                formatter,
                "resource {resource} does not grant capability {capability}"
            ),
            Self::ContextClosed => formatter.write_str("data context is closed"),
            Self::RequestValues(message) => {
                write!(formatter, "request data values were rejected: {message}")
            }
            Self::PolicyNotGranted(message) => {
                write!(formatter, "data policy is not granted: {message}")
            }
            Self::Cancelled => formatter.write_str("data operation was cancelled"),
            Self::Deadline => formatter.write_str("data operation exceeded its deadline"),
            Self::InvalidLoaderPolicy(message) => {
                write!(formatter, "invalid loader policy: {message}")
            }
            Self::LoaderInput(message) => write!(formatter, "loader input was rejected: {message}"),
            Self::LoaderOutput { actual, maximum } => write!(
                formatter,
                "loader output reached {actual} bytes; maximum is {maximum}"
            ),
            Self::LoaderFailure(message) => write!(formatter, "loader failed: {message}"),
            Self::ActionAdmission(message) => {
                write!(formatter, "action admission failed: {message}")
            }
            Self::ActionInput(message) => write!(formatter, "action input was rejected: {message}"),
            Self::ActionOutput { actual, maximum } => write!(
                formatter,
                "action output reached {actual} bytes; maximum is {maximum}"
            ),
            Self::InvalidActionState(message) => {
                write!(formatter, "invalid action state: {message}")
            }
            Self::ActionOutcomeUnknown => {
                formatter.write_str("action outcome is unknown after commit began")
            }
            Self::ActionIdempotencyConflict => {
                formatter.write_str("idempotency key was reused with different admitted input")
            }
            Self::ActionInProgress => {
                formatter.write_str("idempotent action is already in progress")
            }
            Self::ActionFailure(message) => write!(formatter, "action failed: {message}"),
            Self::Serialization => formatter.write_str("data serialization failed"),
            Self::CleanupLimit(maximum) => {
                write!(formatter, "data cleanup limit reached: {maximum}")
            }
        }
    }
}

impl std::error::Error for DataError {}
