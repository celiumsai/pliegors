// SPDX-License-Identifier: AGPL-3.0-only

use crate::wire::{ProductCapabilities, ProductError, WireError};
use http::StatusCode;
use std::fmt;
use std::net::SocketAddr;

/// A bounded, redacted failure from installation admission, sidecar lifecycle,
/// transport, or product execution.
pub struct TransportError {
    pub(crate) kind: Box<TransportErrorKind>,
}

pub(crate) enum TransportErrorKind {
    NonLoopbackEndpoint(SocketAddr),
    Timeout,
    Io(std::io::Error),
    Http(hyper::Error),
    Body,
    MediaType,
    RequestId,
    Protocol(WireError),
    Product {
        status: StatusCode,
        error: ProductError,
    },
    UnexpectedResponse,
    OutcomeUnknown(Option<u128>),
    MutationRolledBack,
    NonStrictCommit,
    IncompatibleCapabilities(ProductCapabilities),
    ValueTooLarge {
        actual: usize,
        maximum: usize,
    },
    InvalidIdempotencyToken,
    MissingEnvironment(&'static str),
    MissingExecutable,
    AuthorityMalformed,
    AuthorityMismatch,
    UnsupportedPlatform,
    ExecutableType,
    ExecutableDigest {
        expected: String,
        actual: String,
    },
    VersionCommand,
    VersionMismatch,
    OutputBound,
    InvalidPath,
    Initialization,
    Termination,
    EarlyExit(String),
    Readiness,
    Clock,
}

impl TransportError {
    pub(crate) fn new(kind: TransportErrorKind) -> Self {
        Self {
            kind: Box::new(kind),
        }
    }

    pub(crate) fn product(status: StatusCode, error: ProductError) -> Self {
        Self::new(TransportErrorKind::Product { status, error })
    }

    /// Returns a stable adapter-owned diagnostic code.
    pub fn code(&self) -> &'static str {
        match self.kind.as_ref() {
            TransportErrorKind::NonLoopbackEndpoint(_) => "non_loopback_endpoint",
            TransportErrorKind::Timeout => "timeout",
            TransportErrorKind::Io(_) => "io",
            TransportErrorKind::Http(_) | TransportErrorKind::Body => "http",
            TransportErrorKind::MediaType => "media_type",
            TransportErrorKind::RequestId => "request_id",
            TransportErrorKind::Protocol(_) => "protocol",
            TransportErrorKind::Product { .. } => "product",
            TransportErrorKind::UnexpectedResponse => "unexpected_response",
            TransportErrorKind::OutcomeUnknown(_) => "outcome_unknown",
            TransportErrorKind::MutationRolledBack => "mutation_rolled_back",
            TransportErrorKind::NonStrictCommit => "non_strict_commit",
            TransportErrorKind::IncompatibleCapabilities(_) => "incompatible_capabilities",
            TransportErrorKind::ValueTooLarge { .. } => "value_too_large",
            TransportErrorKind::InvalidIdempotencyToken => "invalid_idempotency_token",
            TransportErrorKind::MissingEnvironment(_) => "missing_environment",
            TransportErrorKind::MissingExecutable => "missing_executable",
            TransportErrorKind::AuthorityMalformed => "authority_malformed",
            TransportErrorKind::AuthorityMismatch => "authority_mismatch",
            TransportErrorKind::UnsupportedPlatform => "unsupported_platform",
            TransportErrorKind::ExecutableType => "executable_type",
            TransportErrorKind::ExecutableDigest { .. } => "executable_digest",
            TransportErrorKind::VersionCommand => "version_command",
            TransportErrorKind::VersionMismatch => "version_mismatch",
            TransportErrorKind::OutputBound => "output_bound",
            TransportErrorKind::InvalidPath => "invalid_path",
            TransportErrorKind::Initialization => "initialization",
            TransportErrorKind::Termination => "termination",
            TransportErrorKind::EarlyExit(_) => "early_exit",
            TransportErrorKind::Readiness => "readiness",
            TransportErrorKind::Clock => "clock",
        }
    }

    /// Returns the bounded Hyphae product error code when the sidecar rejected
    /// an operation.
    pub fn product_code(&self) -> Option<&str> {
        match self.kind.as_ref() {
            TransportErrorKind::Product { error, .. } => Some(&error.code),
            _ => None,
        }
    }

    /// Returns the related transaction identity when one is available.
    pub fn transaction_id(&self) -> Option<u128> {
        match self.kind.as_ref() {
            TransportErrorKind::Product { error, .. } => error.transaction_id,
            TransportErrorKind::OutcomeUnknown(transaction_id) => *transaction_id,
            _ => None,
        }
    }

    /// Returns the HTTP status attached to a typed product error.
    pub fn product_status(&self) -> Option<StatusCode> {
        match self.kind.as_ref() {
            TransportErrorKind::Product { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub(crate) fn mutation_resolution(&self) -> Option<Option<u128>> {
        match self.kind.as_ref() {
            TransportErrorKind::Timeout
            | TransportErrorKind::Io(_)
            | TransportErrorKind::Http(_)
            | TransportErrorKind::Body
            | TransportErrorKind::MediaType
            | TransportErrorKind::RequestId
            | TransportErrorKind::Protocol(_)
            | TransportErrorKind::UnexpectedResponse => Some(None),
            TransportErrorKind::OutcomeUnknown(transaction_id) => Some(*transaction_id),
            TransportErrorKind::Product { status, error }
                if *status == StatusCode::SERVICE_UNAVAILABLE
                    && error.code == "unknown_commit"
                    && error.category.name() == "unavailable"
                    && error.retry.name() == "unknown_commit"
                    && error.transaction_state.name() == "outcome_unknown"
                    && error.message == "native transaction publication outcome is unknown"
                    && error.unknown_details.is_empty() =>
            {
                error.transaction_id.map(Some)
            }
            _ => None,
        }
    }
}

impl fmt::Debug for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("TransportError");
        debug.field("code", &self.code());
        match self.kind.as_ref() {
            TransportErrorKind::NonLoopbackEndpoint(endpoint) => {
                debug.field("endpoint", endpoint);
            }
            TransportErrorKind::Product { status, error } => {
                debug
                    .field("status", status)
                    .field("product_code", &error.code)
                    .field("request_id", &error.request_id)
                    .field("transaction_id", &error.transaction_id)
                    .field("unknown_detail_count", &error.unknown_details.len());
            }
            TransportErrorKind::OutcomeUnknown(transaction_id) => {
                debug.field("transaction_id", transaction_id);
            }
            TransportErrorKind::IncompatibleCapabilities(capabilities) => {
                debug
                    .field("product_api_version", &capabilities.product_api_version)
                    .field(
                        "native_directory_format",
                        &capabilities.native_directory_format,
                    );
            }
            TransportErrorKind::ValueTooLarge { actual, maximum } => {
                debug.field("actual", actual).field("maximum", maximum);
            }
            TransportErrorKind::MissingEnvironment(name) => {
                debug.field("name", name);
            }
            TransportErrorKind::ExecutableDigest { expected, actual } => {
                debug.field("expected", expected).field("actual", actual);
            }
            TransportErrorKind::EarlyExit(status) => {
                debug.field("status", status);
            }
            TransportErrorKind::Io(_)
            | TransportErrorKind::Http(_)
            | TransportErrorKind::MediaType
            | TransportErrorKind::Protocol(_)
            | TransportErrorKind::Timeout
            | TransportErrorKind::Body
            | TransportErrorKind::RequestId
            | TransportErrorKind::UnexpectedResponse
            | TransportErrorKind::MutationRolledBack
            | TransportErrorKind::NonStrictCommit
            | TransportErrorKind::InvalidIdempotencyToken
            | TransportErrorKind::MissingExecutable
            | TransportErrorKind::AuthorityMalformed
            | TransportErrorKind::AuthorityMismatch
            | TransportErrorKind::UnsupportedPlatform
            | TransportErrorKind::ExecutableType
            | TransportErrorKind::VersionCommand
            | TransportErrorKind::VersionMismatch
            | TransportErrorKind::OutputBound
            | TransportErrorKind::InvalidPath
            | TransportErrorKind::Initialization
            | TransportErrorKind::Termination
            | TransportErrorKind::Readiness
            | TransportErrorKind::Clock => {}
        }
        debug.finish()
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind.as_ref() {
            TransportErrorKind::NonLoopbackEndpoint(_) => "Hyphae endpoint is not loopback",
            TransportErrorKind::Timeout => "Hyphae operation timed out",
            TransportErrorKind::Io(_) => "Hyphae I/O operation failed",
            TransportErrorKind::Http(_) | TransportErrorKind::Body => "Hyphae HTTP exchange failed",
            TransportErrorKind::MediaType => "Hyphae response media type differs",
            TransportErrorKind::RequestId => "Hyphae response request identity differs",
            TransportErrorKind::Protocol(_) => "Hyphae wire value is invalid",
            TransportErrorKind::Product { .. } => "Hyphae rejected the product operation",
            TransportErrorKind::UnexpectedResponse => "Hyphae returned the wrong response kind",
            TransportErrorKind::OutcomeUnknown(_) => "Hyphae mutation outcome remains unknown",
            TransportErrorKind::MutationRolledBack => "Hyphae mutation was rolled back",
            TransportErrorKind::NonStrictCommit => "Hyphae mutation was not strictly durable",
            TransportErrorKind::IncompatibleCapabilities(_) => {
                "Hyphae sidecar capabilities are incompatible"
            }
            TransportErrorKind::ValueTooLarge { .. } => "Hyphae value exceeds the adapter bound",
            TransportErrorKind::InvalidIdempotencyToken => {
                "Hyphae idempotency token must be nonzero"
            }
            TransportErrorKind::MissingEnvironment(_) => {
                "Hyphae installation environment is incomplete"
            }
            TransportErrorKind::MissingExecutable => "Hyphae executable path is missing",
            TransportErrorKind::AuthorityMalformed | TransportErrorKind::AuthorityMismatch => {
                "Hyphae authority is invalid"
            }
            TransportErrorKind::UnsupportedPlatform => "Hyphae platform is not reviewed",
            TransportErrorKind::ExecutableType => "Hyphae executable is not a regular file",
            TransportErrorKind::ExecutableDigest { .. } => "Hyphae executable digest differs",
            TransportErrorKind::VersionCommand | TransportErrorKind::VersionMismatch => {
                "Hyphae version identity differs"
            }
            TransportErrorKind::OutputBound => "Hyphae process output exceeds its bound",
            TransportErrorKind::InvalidPath => "Hyphae data path is not Unicode",
            TransportErrorKind::Initialization => "Hyphae data directory initialization failed",
            TransportErrorKind::Termination => {
                "Hyphae sidecar process tree could not be terminated"
            }
            TransportErrorKind::EarlyExit(_) => "Hyphae sidecar exited before readiness",
            TransportErrorKind::Readiness => "Hyphae sidecar readiness timed out",
            TransportErrorKind::Clock => "system clock cannot produce a Hyphae deadline",
        })
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.kind.as_ref() {
            TransportErrorKind::Io(error) => Some(error),
            TransportErrorKind::Http(error) => Some(error),
            TransportErrorKind::Protocol(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TransportErrorKind> for TransportError {
    fn from(kind: TransportErrorKind) -> Self {
        Self::new(kind)
    }
}

impl From<WireError> for TransportError {
    fn from(error: WireError) -> Self {
        TransportErrorKind::Protocol(error).into()
    }
}
