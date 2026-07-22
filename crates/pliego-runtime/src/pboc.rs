// SPDX-License-Identifier: Apache-2.0

use pliego_pboc::{
    FeatureRequirement, HostAdmission, HostKind, HostProfile, PbocError, PbocManifest, feature,
};
use std::fmt::{Display, Formatter};

pub const NATIVE_PBOC_TARGET_ID: &str = "native-linux-oci";

pub fn native_pboc_host_profile(host_version: impl Into<String>) -> HostProfile {
    HostProfile {
        host_id: "pliego.native".to_owned(),
        host_version: host_version.into(),
        target_id: NATIVE_PBOC_TARGET_ID.to_owned(),
        host_kind: HostKind::NativeOci,
        features: vec![
            FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1),
            FeatureRequirement::required(feature::CACHE_PRIVATE, 1),
            FeatureRequirement::required(feature::CACHE_PUBLIC, 1),
            FeatureRequirement::required(feature::DEPLOYMENT_ROLLBACK, 1),
            FeatureRequirement::required(feature::DEPLOYMENT_ROLLING, 1),
            FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
            FeatureRequirement::required(feature::HTTP_STREAM_BOUNDARY, 1),
            FeatureRequirement::required(feature::HTTP_STREAM_ORDERED, 1),
            FeatureRequirement::required(feature::SECRETS_REFERENCES, 1),
            FeatureRequirement::required(feature::TELEMETRY_RECEIPTS, 1),
        ],
        max_artifact_bytes: 512 * 1024 * 1024,
        max_bundle_bytes: 4 * 1024 * 1024 * 1024,
    }
}

#[derive(Debug)]
pub enum NativePbocError {
    Contract(PbocError),
    RouteGraphMismatch { manifest: String, runtime: String },
    RuntimeContractMismatch { manifest: String, runtime: String },
}

impl Display for NativePbocError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(error) => Display::fmt(error, formatter),
            Self::RouteGraphMismatch { manifest, runtime } => write!(
                formatter,
                "PBOC route graph {manifest} differs from native runtime {runtime}"
            ),
            Self::RuntimeContractMismatch { manifest, runtime } => write!(
                formatter,
                "PBOC runtime contract {manifest} differs from native runtime {runtime}"
            ),
        }
    }
}

impl std::error::Error for NativePbocError {}

impl From<PbocError> for NativePbocError {
    fn from(value: PbocError) -> Self {
        Self::Contract(value)
    }
}

pub(crate) fn admit_native_pboc(
    manifest: &PbocManifest,
    host_version: impl Into<String>,
    route_graph_sha256: &str,
    runtime_contract_sha256: &str,
) -> Result<HostAdmission, NativePbocError> {
    if manifest.build.route_graph_sha256 != route_graph_sha256 {
        return Err(NativePbocError::RouteGraphMismatch {
            manifest: manifest.build.route_graph_sha256.clone(),
            runtime: route_graph_sha256.to_owned(),
        });
    }
    if manifest.build.runtime_contract_sha256 != runtime_contract_sha256 {
        return Err(NativePbocError::RuntimeContractMismatch {
            manifest: manifest.build.runtime_contract_sha256.clone(),
            runtime: runtime_contract_sha256.to_owned(),
        });
    }
    manifest
        .admit(&native_pboc_host_profile(host_version))
        .map_err(Into::into)
}
