// SPDX-License-Identifier: AGPL-3.0-only

//! Server-only integration with the pinned Hyphae Native sidecar.
//!
//! This source preview supervises and admits an exact Hyphae `v1.0.1`
//! executable, then exposes its reviewed scalar Product API subset over
//! loopback HTTP `/v2`. It does not link any `hyphae-*` Rust crate and must not
//! be exposed directly to browser code.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod admit;
mod error;
mod http;
#[cfg(test)]
mod keys;
mod process;
mod wire;

pub use error::TransportError;
pub use http::{MAX_VALUE_BYTES, NativeHttpClient};
pub use process::{HyphaeInstallation, HyphaeSidecar, SidecarAuthority};
pub use wire::{ProductCapabilities, TransactionStatus};

#[cfg(test)]
mod tests;
