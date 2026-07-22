// SPDX-License-Identifier: Apache-2.0

use crate::validate::{HostAdmission, PbocError, encode_manifest, validate_manifest};
use pliego_router::RouteSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PbocManifest {
    pub schema: String,
    pub framework: FrameworkIdentity,
    pub build: BuildIdentity,
    pub compatibility: CompatibilityIdentity,
    pub capabilities: Vec<FeatureRequirement>,
    pub artifacts: Vec<PbocArtifact>,
    pub targets: Vec<DeploymentTarget>,
    pub assets: Vec<PbocAsset>,
    pub routes: Vec<PbocRoute>,
    pub functions: Vec<PbocFunction>,
    pub cache_policies: Vec<CachePolicy>,
    pub permissions: Vec<ResourcePermission>,
    pub secret_references: Vec<SecretReference>,
    pub telemetry_hooks: Vec<TelemetryHook>,
}

impl PbocManifest {
    pub fn validate(&self) -> Result<(), PbocError> {
        validate_manifest(self)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, PbocError> {
        encode_manifest(self)
    }

    pub fn sha256(&self) -> Result<String, PbocError> {
        Ok(crate::util::sha256_bytes(&self.canonical_bytes()?))
    }

    pub fn admit(&self, host: &HostProfile) -> Result<HostAdmission, PbocError> {
        HostAdmission::new(self, host)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameworkIdentity {
    pub name: String,
    pub version: String,
    pub source_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildIdentity {
    pub application_id: String,
    pub release_id: String,
    pub route_graph_sha256: String,
    pub runtime_contract_sha256: String,
    pub artifact_ledger_sha256: String,
    pub provenance: ProvenanceIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvenanceIdentity {
    pub sbom_path: String,
    pub provenance_path: String,
    pub source_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityIdentity {
    pub epoch: u32,
    pub sequence: u64,
    pub state_schema: String,
    pub previous_release_id: Option<String>,
    pub rollback_safe: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeatureRequirement {
    pub id: String,
    pub version: u32,
    pub required: bool,
}

impl FeatureRequirement {
    pub fn required(id: impl Into<String>, version: u32) -> Self {
        Self {
            id: id.into(),
            version,
            required: true,
        }
    }

    pub fn optional(id: impl Into<String>, version: u32) -> Self {
        Self {
            id: id.into(),
            version,
            required: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactRole {
    StaticAsset,
    NativeExecutable,
    CloudflareModule,
    Sbom,
    Provenance,
    Configuration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PbocArtifact {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub role: ArtifactRole,
    pub media_type: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostKind {
    NativeOci,
    CloudflareWorkers,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentTarget {
    pub id: String,
    pub host_kind: HostKind,
    pub artifact_paths: Vec<String>,
    pub required_features: Vec<FeatureRequirement>,
    pub optional_features: Vec<FeatureRequirement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PbocAsset {
    pub request_path: String,
    pub artifact_path: String,
    pub cache_policy_id: String,
    pub immutable: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteKind {
    Static,
    Dynamic,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderMode {
    Complete,
    Ordered,
    Boundary,
    Resource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PbocRoute {
    pub id: String,
    pub method: String,
    pub pattern: String,
    pub kind: RouteKind,
    pub asset_path: Option<String>,
    pub function_id: Option<String>,
    pub render_mode: RenderMode,
    pub cache_policy_id: Option<String>,
    pub required_features: Vec<FeatureRequirement>,
}

impl PbocRoute {
    pub fn dynamic(
        route: &RouteSpec,
        function_id: impl Into<String>,
        render_mode: RenderMode,
        required_features: Vec<FeatureRequirement>,
    ) -> Self {
        Self {
            id: route.id().to_owned(),
            method: route.method().as_str().to_owned(),
            pattern: route.pattern().canonical().to_owned(),
            kind: RouteKind::Dynamic,
            asset_path: None,
            function_id: Some(function_id.into()),
            render_mode,
            cache_policy_id: route.cache_policy_id().map(str::to_owned),
            required_features,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PbocFunction {
    pub id: String,
    pub entrypoint: String,
    pub render_modes: Vec<RenderMode>,
    pub max_response_bytes: u64,
    pub secret_references: Vec<String>,
    pub permission_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheDomain {
    Public,
    Private,
    Session,
    Request,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheRevalidation {
    Immutable,
    TimeBound,
    TagBound,
    NoStore,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CachePolicy {
    pub id: String,
    pub domain: CacheDomain,
    pub revalidation: CacheRevalidation,
    pub max_age_seconds: Option<u64>,
    pub stale_while_revalidate_seconds: Option<u64>,
    pub vary_headers: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourcePermission {
    pub id: String,
    pub resource: String,
    pub capabilities: Vec<String>,
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretReference {
    pub id: String,
    pub purpose: String,
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelemetryHook {
    pub id: String,
    pub signal: String,
    pub required: bool,
    pub redacted_fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostProfile {
    pub host_id: String,
    pub host_version: String,
    pub target_id: String,
    pub host_kind: HostKind,
    pub features: Vec<FeatureRequirement>,
    pub max_artifact_bytes: u64,
    pub max_bundle_bytes: u64,
}

impl HostProfile {
    pub(crate) fn feature_set(&self) -> BTreeSet<(String, u32)> {
        self.features
            .iter()
            .map(|feature| (feature.id.clone(), feature.version))
            .collect()
    }
}
