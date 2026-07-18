// SPDX-License-Identifier: Apache-2.0

//! OpenSDK admission, capability, and evidence contracts.
//!
//! Validation is deliberately separate from execution. A host must admit the
//! exact extension bytes under an explicit policy before constructing a guest
//! runtime, so incompatible or overpowered extensions fail without executing.

mod capability;
mod compatibility;
mod component;
mod manifest;
#[cfg(feature = "runtime")]
mod runtime;
mod tooling;
mod wit;

pub use capability::{
    CapabilityDenial, CapabilityPolicy, EffectBroker, EffectError, EffectFailure, EffectOutcome,
    EffectReceipt,
};
pub use compatibility::{
    CompatibilityError, CompatibilityMatrix, CompatibilitySurface, CompatibilityToolchain,
    Deprecation, DeprecationState, HostCompatibility, preview_compatibility_matrix,
};
pub use component::{ComponentInspection, inspect_component};
pub use manifest::{
    AdmissionError, AdmissionReceipt, Budget, Capability, Determinism, EntryKind, ExtensionEntry,
    ExtensionIdentity, ExtensionManifest, HostContract, Lifecycle, OPENSDK_API_VERSION,
    OPENSDK_MANIFEST_SCHEMA, Plane, ValidatedExtension,
};
#[cfg(feature = "runtime")]
pub use runtime::{
    BuildTransformExecution, BuildTransformInput, BuildTransformOutput, BuildTransformReceipt,
    ComponentHost, ComponentHostError, ComponentHostReceipt, EffectExecutor,
};
pub use tooling::{
    JSON_RPC_VERSION, MCP_PROTOCOL_VERSION, McpHost, RpcError, RpcHost, RpcRequest, RpcResponse,
    TOOLING_PROTOCOL_VERSION,
};
pub use wit::{WitPackageReport, validate_wit_package};
