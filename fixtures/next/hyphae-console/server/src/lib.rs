// SPDX-License-Identifier: AGPL-3.0-only

mod application;
mod hyphae_native;

#[cfg(feature = "acceptance-harness")]
pub use application::build_console_acceptance_runtime;
pub use application::{ConsoleApplicationError, build_console_runtime};
#[cfg(feature = "acceptance-harness")]
pub use hyphae_native::SidecarObservation;
pub use hyphae_native::{
    ConsoleStore, ConsoleStoreError, HyphaeInstallation, HyphaeSidecar, ProductCapabilities,
    SidecarAuthority,
};
