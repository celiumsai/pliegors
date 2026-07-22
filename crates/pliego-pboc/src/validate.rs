// SPDX-License-Identifier: Apache-2.0

use crate::model::{
    CacheDomain, CacheRevalidation, DeploymentTarget, FeatureRequirement, HostProfile,
    PbocManifest, RenderMode, RouteKind,
};
use crate::{MAX_MANIFEST_BYTES, PBOC_SCHEMA};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

const MAX_COLLECTION_ITEMS: usize = 100_000;
const MAX_TEXT_BYTES: usize = 4_096;

#[derive(Debug)]
pub enum PbocError {
    ManifestTooLarge {
        actual: usize,
        maximum: usize,
    },
    Json(serde_json::Error),
    Invalid(String),
    UnsupportedTarget(String),
    UnsupportedFeatures(Vec<String>),
    Artifact(String),
    Io {
        path: String,
        source: std::io::Error,
    },
}

impl Display for PbocError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestTooLarge { actual, maximum } => {
                write!(formatter, "PBOC is {actual} bytes; maximum is {maximum}")
            }
            Self::Json(error) => write!(formatter, "invalid PBOC JSON: {error}"),
            Self::Invalid(message) => write!(formatter, "invalid PBOC: {message}"),
            Self::UnsupportedTarget(target) => {
                write!(formatter, "unsupported PBOC target: {target}")
            }
            Self::UnsupportedFeatures(features) => write!(
                formatter,
                "host lacks required PBOC features: {}",
                features.join(", ")
            ),
            Self::Artifact(message) => {
                write!(formatter, "PBOC artifact verification failed: {message}")
            }
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
        }
    }
}

impl std::error::Error for PbocError {}

impl From<serde_json::Error> for PbocError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn decode_manifest(bytes: &[u8]) -> Result<PbocManifest, PbocError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(PbocError::ManifestTooLarge {
            actual: bytes.len(),
            maximum: MAX_MANIFEST_BYTES,
        });
    }
    let manifest: PbocManifest = serde_json::from_slice(bytes)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn encode_manifest(manifest: &PbocManifest) -> Result<Vec<u8>, PbocError> {
    validate_manifest(manifest)?;
    let bytes = serde_json::to_vec(manifest)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(PbocError::ManifestTooLarge {
            actual: bytes.len(),
            maximum: MAX_MANIFEST_BYTES,
        });
    }
    Ok(bytes)
}

pub fn validate_manifest(manifest: &PbocManifest) -> Result<(), PbocError> {
    if manifest.schema != PBOC_SCHEMA {
        return invalid(format!("unsupported schema {}", manifest.schema));
    }
    bounded_id(&manifest.framework.name, "framework.name")?;
    bounded_text(&manifest.framework.version, "framework.version")?;
    revision(
        &manifest.framework.source_revision,
        "framework.sourceRevision",
    )?;
    bounded_id(&manifest.build.application_id, "build.applicationId")?;
    bounded_id(&manifest.build.release_id, "build.releaseId")?;
    sha256(&manifest.build.route_graph_sha256, "build.routeGraphSha256")?;
    sha256(
        &manifest.build.runtime_contract_sha256,
        "build.runtimeContractSha256",
    )?;
    sha256(
        &manifest.build.artifact_ledger_sha256,
        "build.artifactLedgerSha256",
    )?;
    revision(
        &manifest.build.provenance.source_revision,
        "build.provenance.sourceRevision",
    )?;
    if manifest.build.provenance.source_revision != manifest.framework.source_revision {
        return invalid("framework and provenance source revisions differ");
    }
    if manifest.compatibility.epoch == 0 || manifest.compatibility.sequence == 0 {
        return invalid("compatibility epoch and sequence must be non-zero");
    }
    bounded_id(
        &manifest.compatibility.state_schema,
        "compatibility.stateSchema",
    )?;
    if let Some(previous) = &manifest.compatibility.previous_release_id {
        bounded_id(previous, "compatibility.previousReleaseId")?;
        if previous == &manifest.build.release_id {
            return invalid("previous release cannot equal the current release");
        }
    }

    collection(&manifest.capabilities, "capabilities")?;
    collection(&manifest.artifacts, "artifacts")?;
    collection(&manifest.targets, "targets")?;
    collection(&manifest.assets, "assets")?;
    collection(&manifest.routes, "routes")?;
    collection(&manifest.functions, "functions")?;
    collection(&manifest.cache_policies, "cachePolicies")?;
    collection(&manifest.permissions, "permissions")?;
    collection(&manifest.secret_references, "secretReferences")?;
    collection(&manifest.telemetry_hooks, "telemetryHooks")?;

    require_sorted_unique(
        &manifest.capabilities,
        |value| value.id.as_str(),
        "capabilities",
    )?;
    for feature in &manifest.capabilities {
        validate_feature(feature, "capability")?;
    }

    require_sorted_unique(
        &manifest.artifacts,
        |value| value.path.as_str(),
        "artifacts",
    )?;
    let mut artifact_paths = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for artifact in &manifest.artifacts {
        portable_path(&artifact.path, "artifact.path")?;
        sha256(&artifact.sha256, "artifact.sha256")?;
        bounded_text(&artifact.media_type, "artifact.mediaType")?;
        if artifact.bytes == 0 {
            return invalid(format!("artifact {} has zero bytes", artifact.path));
        }
        total_bytes = total_bytes
            .checked_add(artifact.bytes)
            .ok_or_else(|| PbocError::Invalid("artifact byte total overflows u64".to_owned()))?;
        artifact_paths.insert(artifact.path.as_str());
    }
    if total_bytes == 0 {
        return invalid("PBOC contains no artifact bytes");
    }
    for (label, path) in [
        ("provenance.sbomPath", &manifest.build.provenance.sbom_path),
        (
            "provenance.provenancePath",
            &manifest.build.provenance.provenance_path,
        ),
    ] {
        portable_path(path, label)?;
        if !artifact_paths.contains(path.as_str()) {
            return invalid(format!("{label} references undeclared artifact {path}"));
        }
    }

    require_sorted_unique(&manifest.targets, |value| value.id.as_str(), "targets")?;
    if manifest.targets.is_empty() {
        return invalid("at least one deployment target is required");
    }
    for target in &manifest.targets {
        validate_target(target, &artifact_paths)?;
    }

    require_sorted_unique(
        &manifest.cache_policies,
        |value| value.id.as_str(),
        "cachePolicies",
    )?;
    let cache_ids: BTreeSet<_> = manifest
        .cache_policies
        .iter()
        .map(|value| value.id.as_str())
        .collect();
    for policy in &manifest.cache_policies {
        bounded_id(&policy.id, "cachePolicy.id")?;
        if policy.revalidation == CacheRevalidation::TimeBound && policy.max_age_seconds.is_none() {
            return invalid(format!("cache policy {} requires maxAgeSeconds", policy.id));
        }
        if matches!(policy.domain, CacheDomain::Private | CacheDomain::Session)
            && !policy
                .vary_headers
                .iter()
                .any(|value| value == "cookie" || value == "authorization")
        {
            return invalid(format!(
                "private cache policy {} must vary on cookie or authorization",
                policy.id
            ));
        }
        sorted_strings(&policy.vary_headers, "cachePolicy.varyHeaders", true)?;
        sorted_strings(&policy.tags, "cachePolicy.tags", false)?;
    }

    require_sorted_unique(
        &manifest.assets,
        |value| value.request_path.as_str(),
        "assets",
    )?;
    let mut asset_paths = BTreeSet::new();
    for asset in &manifest.assets {
        request_path(&asset.request_path, "asset.requestPath")?;
        portable_path(&asset.artifact_path, "asset.artifactPath")?;
        if !artifact_paths.contains(asset.artifact_path.as_str()) {
            return invalid(format!(
                "asset {} references undeclared bytes",
                asset.request_path
            ));
        }
        if !cache_ids.contains(asset.cache_policy_id.as_str()) {
            return invalid(format!(
                "asset {} references unknown cache policy",
                asset.request_path
            ));
        }
        asset_paths.insert(asset.artifact_path.as_str());
    }

    require_sorted_unique(
        &manifest.secret_references,
        |value| value.id.as_str(),
        "secretReferences",
    )?;
    let secret_ids: BTreeSet<_> = manifest
        .secret_references
        .iter()
        .map(|value| value.id.as_str())
        .collect();
    for secret in &manifest.secret_references {
        bounded_id(&secret.id, "secretReference.id")?;
        bounded_text(&secret.purpose, "secretReference.purpose")?;
        if secret.purpose.contains('=') || secret.purpose.contains("Bearer ") {
            return invalid(format!(
                "secret reference {} appears to contain a value",
                secret.id
            ));
        }
    }

    require_sorted_unique(
        &manifest.permissions,
        |value| value.id.as_str(),
        "permissions",
    )?;
    let permission_ids: BTreeSet<_> = manifest
        .permissions
        .iter()
        .map(|value| value.id.as_str())
        .collect();
    for permission in &manifest.permissions {
        bounded_id(&permission.id, "permission.id")?;
        bounded_id(&permission.resource, "permission.resource")?;
        sorted_strings(&permission.capabilities, "permission.capabilities", false)?;
        if permission.capabilities.is_empty() {
            return invalid(format!("permission {} has no capabilities", permission.id));
        }
    }

    require_sorted_unique(&manifest.functions, |value| value.id.as_str(), "functions")?;
    let function_ids: BTreeSet<_> = manifest
        .functions
        .iter()
        .map(|value| value.id.as_str())
        .collect();
    for function in &manifest.functions {
        bounded_id(&function.id, "function.id")?;
        bounded_id(&function.entrypoint, "function.entrypoint")?;
        if function.max_response_bytes == 0 {
            return invalid(format!("function {} has zero response budget", function.id));
        }
        if function.render_modes.is_empty() {
            return invalid(format!("function {} has no render modes", function.id));
        }
        require_sorted_unique_by_ord(&function.render_modes, "function.renderModes")?;
        sorted_strings(
            &function.secret_references,
            "function.secretReferences",
            false,
        )?;
        for secret in &function.secret_references {
            if !secret_ids.contains(secret.as_str()) {
                return invalid(format!(
                    "function {} references unknown secret {secret}",
                    function.id
                ));
            }
        }
        sorted_strings(&function.permission_ids, "function.permissionIds", false)?;
        for permission in &function.permission_ids {
            if !permission_ids.contains(permission.as_str()) {
                return invalid(format!(
                    "function {} references unknown permission {permission}",
                    function.id
                ));
            }
        }
    }

    let declared_features: BTreeSet<_> = manifest
        .capabilities
        .iter()
        .map(|feature| (feature.id.as_str(), feature.version))
        .collect();
    let mut route_keys = BTreeSet::new();
    let mut route_ids = BTreeSet::new();
    for route in &manifest.routes {
        bounded_id(&route.id, "route.id")?;
        method(&route.method)?;
        request_path(&route.pattern, "route.pattern")?;
        if !route_ids.insert(route.id.as_str()) {
            return invalid(format!("duplicate route ID {}", route.id));
        }
        if !route_keys.insert((route.method.as_str(), route.pattern.as_str())) {
            return invalid(format!(
                "duplicate route {} {}",
                route.method, route.pattern
            ));
        }
        match route.kind {
            RouteKind::Static => {
                if route.function_id.is_some() || route.asset_path.is_none() {
                    return invalid(format!("static route {} has invalid target", route.id));
                }
                let asset = route.asset_path.as_deref().expect("checked above");
                if !asset_paths.contains(asset) {
                    return invalid(format!(
                        "static route {} references unknown asset",
                        route.id
                    ));
                }
                if route.render_mode != RenderMode::Complete {
                    return invalid(format!(
                        "static route {} must use complete render mode",
                        route.id
                    ));
                }
            }
            RouteKind::Dynamic => {
                if route.asset_path.is_some() || route.function_id.is_none() {
                    return invalid(format!("dynamic route {} has invalid target", route.id));
                }
                let function_id = route.function_id.as_deref().expect("checked above");
                if !function_ids.contains(function_id) {
                    return invalid(format!(
                        "dynamic route {} references unknown function",
                        route.id
                    ));
                }
                let function = manifest
                    .functions
                    .iter()
                    .find(|candidate| candidate.id == function_id)
                    .expect("function ID set was checked");
                if !function.render_modes.contains(&route.render_mode) {
                    return invalid(format!(
                        "route {} render mode is not implemented by function {}",
                        route.id, function.id
                    ));
                }
            }
        }
        if route
            .cache_policy_id
            .as_ref()
            .is_some_and(|id| !cache_ids.contains(id.as_str()))
        {
            return invalid(format!(
                "route {} references unknown cache policy",
                route.id
            ));
        }
        require_sorted_unique(
            &route.required_features,
            |value| value.id.as_str(),
            "route.requiredFeatures",
        )?;
        for feature in &route.required_features {
            validate_feature(feature, "route feature")?;
            if !feature.required {
                return invalid(format!(
                    "route {} lists an optional feature as required",
                    route.id
                ));
            }
            if !declared_features.contains(&(feature.id.as_str(), feature.version)) {
                return invalid(format!(
                    "route {} feature {} is not globally declared",
                    route.id, feature.id
                ));
            }
        }
    }
    let route_order: Vec<_> = manifest
        .routes
        .iter()
        .map(|route| (&route.method, &route.pattern, &route.id))
        .collect();
    if !route_order.windows(2).all(|window| window[0] < window[1]) {
        return invalid("routes are not in canonical method/pattern/id order");
    }

    require_sorted_unique(
        &manifest.telemetry_hooks,
        |value| value.id.as_str(),
        "telemetryHooks",
    )?;
    for hook in &manifest.telemetry_hooks {
        bounded_id(&hook.id, "telemetryHook.id")?;
        bounded_id(&hook.signal, "telemetryHook.signal")?;
        sorted_strings(&hook.redacted_fields, "telemetryHook.redactedFields", false)?;
    }
    Ok(())
}

fn validate_target(target: &DeploymentTarget, artifacts: &BTreeSet<&str>) -> Result<(), PbocError> {
    bounded_id(&target.id, "target.id")?;
    sorted_strings(&target.artifact_paths, "target.artifactPaths", false)?;
    if target.artifact_paths.is_empty() {
        return invalid(format!("target {} contains no artifacts", target.id));
    }
    for path in &target.artifact_paths {
        if !artifacts.contains(path.as_str()) {
            return invalid(format!(
                "target {} references unknown artifact {path}",
                target.id
            ));
        }
    }
    require_sorted_unique(
        &target.required_features,
        |value| value.id.as_str(),
        "target.requiredFeatures",
    )?;
    require_sorted_unique(
        &target.optional_features,
        |value| value.id.as_str(),
        "target.optionalFeatures",
    )?;
    for feature in &target.required_features {
        validate_feature(feature, "target required feature")?;
        if !feature.required {
            return invalid(format!(
                "target {} required feature is marked optional",
                target.id
            ));
        }
    }
    for feature in &target.optional_features {
        validate_feature(feature, "target optional feature")?;
        if feature.required {
            return invalid(format!(
                "target {} optional feature is marked required",
                target.id
            ));
        }
    }
    let required: BTreeSet<_> = target
        .required_features
        .iter()
        .map(|value| (&value.id, value.version))
        .collect();
    if let Some(feature) = target
        .optional_features
        .iter()
        .find(|value| required.contains(&(&value.id, value.version)))
    {
        return invalid(format!(
            "target {} repeats feature {}",
            target.id, feature.id
        ));
    }
    Ok(())
}

fn validate_feature(feature: &FeatureRequirement, label: &str) -> Result<(), PbocError> {
    bounded_id(&feature.id, label)?;
    if feature.version == 0 {
        return invalid(format!("{label} {} has version zero", feature.id));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostAdmission {
    pub contract: String,
    pub manifest_sha256: String,
    pub host_id: String,
    pub host_version: String,
    pub target_id: String,
    pub artifact_paths: Vec<String>,
    pub unsupported_optional_features: Vec<String>,
    pub required_secret_references: Vec<String>,
}

impl HostAdmission {
    pub(crate) fn new(manifest: &PbocManifest, host: &HostProfile) -> Result<Self, PbocError> {
        validate_manifest(manifest)?;
        bounded_id(&host.host_id, "host.id")?;
        bounded_text(&host.host_version, "host.version")?;
        let target = manifest
            .targets
            .iter()
            .find(|target| target.id == host.target_id && target.host_kind == host.host_kind)
            .ok_or_else(|| PbocError::UnsupportedTarget(host.target_id.clone()))?;
        let available = host.feature_set();
        let required = manifest
            .capabilities
            .iter()
            .filter(|feature| feature.required)
            .chain(target.required_features.iter())
            .chain(
                manifest
                    .routes
                    .iter()
                    .flat_map(|route| route.required_features.iter()),
            );
        let missing: BTreeSet<_> = required
            .filter(|feature| !available.contains(&(feature.id.clone(), feature.version)))
            .map(|feature| format!("{}@{}", feature.id, feature.version))
            .collect();
        if !missing.is_empty() {
            return Err(PbocError::UnsupportedFeatures(
                missing.into_iter().collect(),
            ));
        }
        let optional = manifest
            .capabilities
            .iter()
            .filter(|feature| !feature.required)
            .chain(target.optional_features.iter());
        let unsupported_optional_features: BTreeSet<_> = optional
            .filter(|feature| !available.contains(&(feature.id.clone(), feature.version)))
            .map(|feature| format!("{}@{}", feature.id, feature.version))
            .collect();
        let by_path: BTreeMap<_, _> = manifest
            .artifacts
            .iter()
            .map(|artifact| (artifact.path.as_str(), artifact))
            .collect();
        let mut upload_paths: BTreeSet<String> = target.artifact_paths.iter().cloned().collect();
        upload_paths.extend(
            manifest
                .assets
                .iter()
                .map(|asset| asset.artifact_path.clone()),
        );
        upload_paths.insert(manifest.build.provenance.sbom_path.clone());
        upload_paths.insert(manifest.build.provenance.provenance_path.clone());
        let mut total = 0_u64;
        for path in &upload_paths {
            let artifact = by_path
                .get(path.as_str())
                .expect("manifest validation checks target paths");
            if artifact.bytes > host.max_artifact_bytes {
                return invalid(format!(
                    "artifact {} exceeds host limit {}",
                    artifact.path, host.max_artifact_bytes
                ));
            }
            total = total.checked_add(artifact.bytes).ok_or_else(|| {
                PbocError::Invalid("host upload byte total overflows u64".to_owned())
            })?;
        }
        if total > host.max_bundle_bytes {
            return invalid(format!(
                "host upload is {total} bytes; maximum is {}",
                host.max_bundle_bytes
            ));
        }
        let required_secret_references = manifest
            .secret_references
            .iter()
            .filter(|secret| secret.required)
            .map(|secret| secret.id.clone())
            .collect();
        Ok(Self {
            contract: "dev.pliegors.pboc-admission/v1".to_owned(),
            manifest_sha256: manifest.sha256()?,
            host_id: host.host_id.clone(),
            host_version: host.host_version.clone(),
            target_id: target.id.clone(),
            artifact_paths: upload_paths.into_iter().collect(),
            unsupported_optional_features: unsupported_optional_features.into_iter().collect(),
            required_secret_references,
        })
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, PbocError> {
    Err(PbocError::Invalid(message.into()))
}

fn bounded_text(value: &str, label: &str) -> Result<(), PbocError> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES || value.chars().any(char::is_control) {
        return invalid(format!("{label} is empty, too long, or contains controls"));
    }
    Ok(())
}

fn bounded_id(value: &str, label: &str) -> Result<(), PbocError> {
    bounded_text(value, label)?;
    let mut bytes = value.bytes();
    if value.len() > 128
        || !bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        || !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
        || value.ends_with(['-', '.'])
        || value.contains("--")
        || value.contains("..")
    {
        return invalid(format!("{label} is not a portable identifier: {value}"));
    }
    Ok(())
}

fn sha256(value: &str, label: &str) -> Result<(), PbocError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return invalid(format!("{label} is not lowercase SHA-256"));
    }
    Ok(())
}

fn revision(value: &str, label: &str) -> Result<(), PbocError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return invalid(format!("{label} is not a 40-character lowercase revision"));
    }
    Ok(())
}

fn portable_path(value: &str, label: &str) -> Result<(), PbocError> {
    crate::util::validate_portable_path(value)
        .map(|_| ())
        .map_err(|error| PbocError::Invalid(format!("{label}: {error}")))
}

fn request_path(value: &str, label: &str) -> Result<(), PbocError> {
    if value.is_empty()
        || value.len() > MAX_TEXT_BYTES
        || !value.starts_with('/')
        || (value != "/" && value.ends_with('/'))
        || value.contains("//")
        || value.contains('\\')
        || value.contains('%')
        || value.contains('?')
        || value.contains('#')
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return invalid(format!("{label} is not a canonical request path: {value}"));
    }
    Ok(())
}

fn method(value: &str) -> Result<(), PbocError> {
    if value.is_empty() || value.len() > 32 || !value.bytes().all(|byte| byte.is_ascii_uppercase())
    {
        return invalid(format!("invalid route method {value}"));
    }
    Ok(())
}

fn collection<T>(values: &[T], label: &str) -> Result<(), PbocError> {
    if values.len() > MAX_COLLECTION_ITEMS {
        return invalid(format!("{label} exceeds {MAX_COLLECTION_ITEMS} entries"));
    }
    Ok(())
}

fn require_sorted_unique<T, F>(values: &[T], key: F, label: &str) -> Result<(), PbocError>
where
    F: Fn(&T) -> &str,
{
    if !values
        .windows(2)
        .all(|window| key(&window[0]) < key(&window[1]))
    {
        return invalid(format!("{label} must be sorted with unique identifiers"));
    }
    Ok(())
}

fn require_sorted_unique_by_ord<T: Ord>(values: &[T], label: &str) -> Result<(), PbocError> {
    if !values.windows(2).all(|window| window[0] < window[1]) {
        return invalid(format!("{label} must be sorted and unique"));
    }
    Ok(())
}

fn sorted_strings(values: &[String], label: &str, lowercase: bool) -> Result<(), PbocError> {
    if !values.windows(2).all(|window| window[0] < window[1]) {
        return invalid(format!("{label} must be sorted and unique"));
    }
    for value in values {
        bounded_text(value, label)?;
        if lowercase && value.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return invalid(format!("{label} values must be lowercase"));
        }
    }
    Ok(())
}
