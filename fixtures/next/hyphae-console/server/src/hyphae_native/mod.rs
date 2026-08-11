// SPDX-License-Identifier: AGPL-3.0-only

mod http;
mod process;
mod wire;

use std::fmt;

pub use http::{NativeHttpClient, TransportError};
pub use process::{HyphaeInstallation, HyphaeSidecar, SidecarAuthority};
pub use wire::ProductCapabilities;
use wire::TransactionStatus;

#[cfg(test)]
use process::AdmissionError;
#[cfg(test)]
use wire::{
    ErrorCategory, ProductLimits, ProductOperation, ProductResponse, RetryClass, UnknownDetail,
    WireError, decode_error, decode_response, encode_request,
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
        self.admit().await?;
        let key = tenant_key(tenant_id)?;
        match self.client.set(&key, value, idempotency_token, 3).await {
            Ok(()) => Ok(()),
            Err(error) if mutation_outcome_requires_resolution(&error) => {
                self.resolve_mutation(idempotency_token).await
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn resolve_mutation(&self, idempotency_token: u128) -> Result<(), ConsoleStoreError> {
        match self.client.resolve_idempotency(idempotency_token, 4).await {
            Ok(TransactionStatus::Committed(receipt)) if receipt.durability == 0 => Ok(()),
            Ok(TransactionStatus::RolledBack(_)) => Err(ConsoleStoreError::MutationRolledBack),
            Ok(TransactionStatus::Unknown | TransactionStatus::OutcomeUnknown(_)) | Err(_) => {
                Err(ConsoleStoreError::OutcomeUnknown)
            }
            Ok(TransactionStatus::Committed(_)) => Err(ConsoleStoreError::NonStrictCommit),
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
        }
    }
}

fn mutation_outcome_requires_resolution(error: &TransportError) -> bool {
    matches!(
        error,
        TransportError::Timeout
            | TransportError::Http(_)
            | TransportError::Body
            | TransportError::MediaType(_)
            | TransportError::RequestId
            | TransportError::Protocol(_)
            | TransportError::UnexpectedResponse
            | TransportError::OutcomeUnknown(_)
    )
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
