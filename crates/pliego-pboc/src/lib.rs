// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![forbid(unsafe_code)]

//! Pliego Build Output Contract (PBOC).
//!
//! PBOC describes one sealed application build without making a deployment
//! provider authoritative. Validation and capability admission happen before
//! a host receives application bytes.

mod artifact;
mod compatibility;
mod model;
mod router;
mod util;
mod validate;

pub use artifact::{BundleVerification, verify_bundle};
pub use compatibility::{
    CompatibilityDirection, CompatibilityError, CompatibilityReceipt, verify_rollback_transition,
    verify_rolling_transition,
};
pub use model::{
    ArtifactRole, BuildIdentity, CacheDomain, CachePolicy, CacheRevalidation,
    CompatibilityIdentity, DeploymentTarget, FeatureRequirement, FrameworkIdentity, HostKind,
    HostProfile, PbocArtifact, PbocAsset, PbocFunction, PbocManifest, PbocRoute,
    ProvenanceIdentity, RenderMode, ResourcePermission, RouteKind, SecretReference, TelemetryHook,
};
pub use router::{PbocRouteMatch, PbocRouter, PbocRouterError};
pub use util::{RuntimeContractBinding, runtime_contract_sha256_v1};
pub use validate::{HostAdmission, PbocError, decode_manifest, encode_manifest, validate_manifest};

pub const PBOC_FILE_NAME: &str = "pliego.pboc.json";
pub const PBOC_SCHEMA: &str = "dev.pliegors.pboc/v1alpha1";
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

pub mod feature {
    pub const ASSETS_IMMUTABLE: &str = "assets.immutable";
    pub const CACHE_PRIVATE: &str = "cache.private";
    pub const CACHE_PUBLIC: &str = "cache.public";
    pub const DEPLOYMENT_ROLLBACK: &str = "deployment.rollback";
    pub const DEPLOYMENT_ROLLING: &str = "deployment.rolling";
    pub const HTTP_COMPLETE: &str = "http.complete";
    pub const HTTP_STREAM_BOUNDARY: &str = "http.stream.boundary";
    pub const HTTP_STREAM_ORDERED: &str = "http.stream.ordered";
    pub const SECRETS_REFERENCES: &str = "secrets.references";
    pub const TELEMETRY_RECEIPTS: &str = "telemetry.receipts";
}
