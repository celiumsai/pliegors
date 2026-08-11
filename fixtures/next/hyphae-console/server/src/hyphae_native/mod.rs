// SPDX-License-Identifier: GPL-3.0-only

mod http;
mod process;
mod wire;

use std::fmt;

pub use http::{NativeHttpClient, TransportError};
pub use process::{HyphaeInstallation, HyphaeSidecar, SidecarAuthority};
pub use wire::ProductCapabilities;

#[cfg(test)]
use process::AdmissionError;
#[cfg(test)]
use wire::{
    ErrorCategory, ProductLimits, ProductOperation, ProductResponse, RetryClass, UnknownDetail,
    WireError, decode_error, decode_response, encode_request,
};

const CONSOLE_KEY: &[u8] = b"pliegors-console";

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

    pub async fn get(&self) -> Result<Option<Vec<u8>>, ConsoleStoreError> {
        self.admit().await?;
        self.client.get(CONSOLE_KEY, 2).await.map_err(Into::into)
    }

    pub async fn put(
        &self,
        value: &[u8],
        idempotency_token: u128,
    ) -> Result<(), ConsoleStoreError> {
        if value.len() > 1024 * 1024 {
            return Err(ConsoleStoreError::ValueTooLarge);
        }
        self.admit().await?;
        self.client
            .set(CONSOLE_KEY, value, idempotency_token, 3)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug)]
pub enum ConsoleStoreError {
    Transport(TransportError),
    IncompatibleCapabilities(ProductCapabilities),
    ValueTooLarge,
}

impl fmt::Display for ConsoleStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "Hyphae sidecar request failed: {error}"),
            Self::IncompatibleCapabilities(_) => {
                formatter.write_str("Hyphae sidecar capabilities are incompatible")
            }
            Self::ValueTooLarge => formatter.write_str("Console state exceeds its local bound"),
        }
    }
}

impl std::error::Error for ConsoleStoreError {}

impl From<TransportError> for ConsoleStoreError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

#[cfg(test)]
mod tests;
