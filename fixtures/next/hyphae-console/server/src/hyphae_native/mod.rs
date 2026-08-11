// SPDX-License-Identifier: AGPL-3.0-only

mod http;
mod process;
mod wire;

use std::fmt;

pub use http::{NativeHttpClient, TransportError};
#[cfg(feature = "acceptance-harness")]
pub use process::SidecarObservation;
pub use process::{HyphaeInstallation, HyphaeSidecar, SidecarAuthority};
pub use wire::ProductCapabilities;
use wire::{ErrorCategory, RetryClass, TransactionState, TransactionStatus};

#[cfg(test)]
use process::AdmissionError;
#[cfg(test)]
use wire::{
    ProductLimits, ProductOperation, ProductResponse, UnknownDetail, WireError, decode_error,
    decode_response, encode_request,
};

const CONSOLE_KEY_PREFIX: &[u8] = b"pliegors-console/v1/";

#[derive(Clone)]
pub struct ConsoleStore {
    client: NativeHttpClient,
}

impl ConsoleStore {
    pub fn new(client: NativeHttpClient) -> Self {
        Self { client }
    }

    pub async fn admit(&self) -> Result<ProductCapabilities, ConsoleStoreError> {
        let capabilities = self.client.capabilities(1).await?;
        if capabilities.product_api_version != 1 || capabilities.native_directory_format != 1 {
            return Err(ConsoleStoreError::IncompatibleCapabilities(capabilities));
        }
        Ok(capabilities)
    }

    pub async fn get(&self, tenant_id: &str) -> Result<Option<Vec<u8>>, ConsoleStoreError> {
        self.admit().await?;
        let key = tenant_key(tenant_id)?;
        self.client.get(&key, 2).await.map_err(Into::into)
    }

    pub async fn put(
        &self,
        tenant_id: &str,
        value: &[u8],
        idempotency_token: u128,
    ) -> Result<(), ConsoleStoreError> {
        if value.len() > 1024 * 1024 {
            return Err(ConsoleStoreError::ValueTooLarge);
        }
        if idempotency_token == 0 {
            return Err(ConsoleStoreError::InvalidIdempotencyToken);
        }
        self.admit().await?;
        let key = tenant_key(tenant_id)?;
        match self.client.set(&key, value, idempotency_token, 3).await {
            Ok(()) => Ok(()),
            Err(error) => match mutation_resolution(&error) {
                Some(expected_transaction) => {
                    self.resolve_mutation(idempotency_token, expected_transaction)
                        .await
                }
                None => Err(error.into()),
            },
        }
    }

    async fn resolve_mutation(
        &self,
        idempotency_token: u128,
        expected_transaction: Option<u128>,
    ) -> Result<(), ConsoleStoreError> {
        match self.client.resolve_idempotency(idempotency_token, 4).await {
            Ok(TransactionStatus::Committed(receipt))
                if receipt.durability == 0
                    && transaction_matches(expected_transaction, receipt.transaction_id) =>
            {
                Ok(())
            }
            Ok(TransactionStatus::RolledBack(transaction_id))
                if transaction_matches(expected_transaction, transaction_id) =>
            {
                Err(ConsoleStoreError::MutationRolledBack)
            }
            Ok(TransactionStatus::Unknown) | Err(_) => Err(ConsoleStoreError::OutcomeUnknown),
            Ok(TransactionStatus::OutcomeUnknown(transaction_id))
                if transaction_matches(expected_transaction, transaction_id) =>
            {
                Err(ConsoleStoreError::OutcomeUnknown)
            }
            Ok(TransactionStatus::Committed(receipt))
                if transaction_matches(expected_transaction, receipt.transaction_id)
                    && receipt.durability != 0 =>
            {
                Err(ConsoleStoreError::NonStrictCommit)
            }
            Ok(
                TransactionStatus::Committed(_)
                | TransactionStatus::RolledBack(_)
                | TransactionStatus::OutcomeUnknown(_),
            ) => Err(ConsoleStoreError::OutcomeUnknown),
        }
    }
}

#[derive(Debug)]
pub enum ConsoleStoreError {
    Transport(TransportError),
    IncompatibleCapabilities(ProductCapabilities),
    InvalidTenant,
    MutationRolledBack,
    OutcomeUnknown,
    NonStrictCommit,
    ValueTooLarge,
    InvalidIdempotencyToken,
}

impl fmt::Display for ConsoleStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "Hyphae sidecar request failed: {error}"),
            Self::IncompatibleCapabilities(_) => {
                formatter.write_str("Hyphae sidecar capabilities are incompatible")
            }
            Self::InvalidTenant => formatter.write_str("Console tenant identity is invalid"),
            Self::MutationRolledBack => formatter.write_str("Console mutation was rolled back"),
            Self::OutcomeUnknown => formatter.write_str("Console mutation outcome is unknown"),
            Self::NonStrictCommit => {
                formatter.write_str("Console mutation was not strictly durable")
            }
            Self::ValueTooLarge => formatter.write_str("Console state exceeds its local bound"),
            Self::InvalidIdempotencyToken => {
                formatter.write_str("Console mutation identity is invalid")
            }
        }
    }
}

fn mutation_resolution(error: &TransportError) -> Option<Option<u128>> {
    match error {
        TransportError::Timeout
        | TransportError::Http(_)
        | TransportError::Body
        | TransportError::MediaType(_)
        | TransportError::RequestId
        | TransportError::Protocol(_)
        | TransportError::UnexpectedResponse => Some(None),
        TransportError::OutcomeUnknown(transaction_id) => Some(Some(*transaction_id)),
        TransportError::Product { status, error }
            if *status == ::http::StatusCode::SERVICE_UNAVAILABLE
                && error.code == "unknown_commit"
                && error.category == ErrorCategory::Unavailable
                && error.retry == RetryClass::UnknownCommit
                && error.transaction_state == TransactionState::OutcomeUnknown
                && error.message == "native transaction publication outcome is unknown"
                && error.request_id == Some(3)
                && error.unknown_details.is_empty() =>
        {
            error.transaction_id.map(Some)
        }
        _ => None,
    }
}

fn transaction_matches(expected: Option<u128>, actual: u128) -> bool {
    expected.is_none_or(|expected| expected == actual)
}

fn tenant_key(tenant_id: &str) -> Result<Vec<u8>, ConsoleStoreError> {
    if tenant_id.is_empty()
        || tenant_id.len() > 64
        || !tenant_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ConsoleStoreError::InvalidTenant);
    }
    let mut key = Vec::with_capacity(CONSOLE_KEY_PREFIX.len() + tenant_id.len());
    key.extend_from_slice(CONSOLE_KEY_PREFIX);
    key.extend_from_slice(tenant_id.as_bytes());
    Ok(key)
}

impl std::error::Error for ConsoleStoreError {}

impl From<TransportError> for ConsoleStoreError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

#[cfg(test)]
mod tests;
