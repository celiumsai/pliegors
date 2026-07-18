// SPDX-License-Identifier: Apache-2.0

use crate::CapabilityPolicy;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

pub const OPENSDK_API_VERSION: &str = "0.1.0-preview.1";
pub const OPENSDK_MANIFEST_SCHEMA: &str = "dev.pliegors.sdk-extension/v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Plane {
    Build,
    Server,
    Browser,
    Tooling,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntryKind {
    WasmComponent,
    BrowserEsm,
    JsonRpcProcess,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Determinism {
    Pure,
    RecordedEffect,
    NativeTrusted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    FilesystemRead,
    FilesystemWrite,
    Network,
    Environment,
    Clock,
    Random,
    Http,
    EffectBroker,
    Dom,
    Motion,
    SmoothScroll,
    Audio,
    Video,
    Webgl,
    Webgpu,
    HighFrequencyRaf,
}

impl Capability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FilesystemRead => "filesystem-read",
            Self::FilesystemWrite => "filesystem-write",
            Self::Network => "network",
            Self::Environment => "environment",
            Self::Clock => "clock",
            Self::Random => "random",
            Self::Http => "http",
            Self::EffectBroker => "effect-broker",
            Self::Dom => "dom",
            Self::Motion => "motion",
            Self::SmoothScroll => "smooth-scroll",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Webgl => "webgl",
            Self::Webgpu => "webgpu",
            Self::HighFrequencyRaf => "high-frequency-raf",
        }
    }

    fn is_browser_only(self) -> bool {
        matches!(
            self,
            Self::Dom
                | Self::Motion
                | Self::SmoothScroll
                | Self::Audio
                | Self::Video
                | Self::Webgl
                | Self::Webgpu
                | Self::HighFrequencyRaf
        )
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Capability {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "filesystem-read" => Ok(Self::FilesystemRead),
            "filesystem-write" => Ok(Self::FilesystemWrite),
            "network" => Ok(Self::Network),
            "environment" => Ok(Self::Environment),
            "clock" => Ok(Self::Clock),
            "random" => Ok(Self::Random),
            "http" => Ok(Self::Http),
            "effect-broker" => Ok(Self::EffectBroker),
            "dom" => Ok(Self::Dom),
            "motion" => Ok(Self::Motion),
            "smooth-scroll" => Ok(Self::SmoothScroll),
            "audio" => Ok(Self::Audio),
            "video" => Ok(Self::Video),
            "webgl" => Ok(Self::Webgl),
            "webgpu" => Ok(Self::Webgpu),
            "high-frequency-raf" => Ok(Self::HighFrequencyRaf),
            _ => Err(format!("unknown OpenSDK capability `{value}`")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionIdentity {
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionEntry {
    pub kind: EntryKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_element: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    pub cpu_ms: u64,
    pub wall_time_ms: u64,
    pub memory_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lifecycle {
    pub init: bool,
    pub update: bool,
    pub suspend: bool,
    pub resume: bool,
    pub dispose: bool,
    pub hmr: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionManifest {
    pub schema: String,
    pub api_version: String,
    pub host_version: String,
    pub plane: Plane,
    pub identity: ExtensionIdentity,
    pub entry: ExtensionEntry,
    pub determinism: Determinism,
    pub imports: Vec<String>,
    pub exports: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub required_features: Vec<String>,
    pub optional_features: Vec<String>,
    pub budgets: Budget,
    pub lifecycle: Lifecycle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostContract {
    pub version: Version,
    pub api_version: Version,
    pub features: BTreeSet<String>,
    pub policy: CapabilityPolicy,
}

impl HostContract {
    pub fn preview(version: Version, policy: CapabilityPolicy) -> Self {
        Self {
            version,
            api_version: Version::parse(OPENSDK_API_VERSION)
                .expect("the built-in OpenSDK API version is valid"),
            features: BTreeSet::new(),
            policy,
        }
    }

    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.features.insert(feature.into());
        self
    }

    pub fn admit(
        &self,
        manifest: ExtensionManifest,
        extension_bytes: &[u8],
    ) -> Result<ValidatedExtension, AdmissionError> {
        validate_manifest_shape(&manifest)?;
        let extension_version = Version::parse(&manifest.identity.version)
            .map_err(|error| AdmissionError::InvalidVersion(error.to_string()))?;
        let api_version = Version::parse(&manifest.api_version)
            .map_err(|error| AdmissionError::InvalidApiVersion(error.to_string()))?;
        if api_version != self.api_version {
            return Err(AdmissionError::IncompatibleApi {
                extension: api_version,
                host: self.api_version.clone(),
            });
        }
        let host_requirement = VersionReq::parse(&manifest.host_version)
            .map_err(|error| AdmissionError::InvalidHostRequirement(error.to_string()))?;
        if !host_requirement.matches(&self.version) {
            return Err(AdmissionError::IncompatibleHost {
                requirement: manifest.host_version.clone(),
                host: self.version.clone(),
            });
        }
        let actual_digest = format!("sha256:{:x}", Sha256::digest(extension_bytes));
        if manifest.identity.digest != actual_digest {
            return Err(AdmissionError::DigestMismatch {
                declared: manifest.identity.digest.clone(),
                actual: actual_digest,
            });
        }
        for feature in &manifest.required_features {
            if !self.features.contains(feature) {
                return Err(AdmissionError::MissingFeature(feature.clone()));
            }
        }
        for capability in &manifest.capabilities {
            self.policy
                .require(*capability, "extension admission")
                .map_err(|_| AdmissionError::CapabilityDenied(*capability))?;
        }
        let receipt = AdmissionReceipt {
            schema: "dev.pliegors.sdk-admission/v1".to_owned(),
            extension: format!(
                "{}:{}/{}",
                manifest.identity.namespace, manifest.identity.name, extension_version
            ),
            digest: manifest.identity.digest.clone(),
            host_version: self.version.to_string(),
            api_version: self.api_version.to_string(),
            granted_capabilities: manifest.capabilities.clone(),
            negotiated_features: manifest
                .required_features
                .iter()
                .chain(manifest.optional_features.iter())
                .filter(|feature| self.features.contains(*feature))
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        };
        Ok(ValidatedExtension { manifest, receipt })
    }

    pub fn admit_component(
        &self,
        manifest: ExtensionManifest,
        extension_bytes: &[u8],
    ) -> Result<ValidatedExtension, AdmissionError> {
        let inspection =
            crate::inspect_component(extension_bytes).map_err(AdmissionError::InvalidComponent)?;
        inspection
            .verify_manifest(&manifest)
            .map_err(AdmissionError::InvalidComponent)?;
        self.admit(manifest, extension_bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedExtension {
    manifest: ExtensionManifest,
    receipt: AdmissionReceipt,
}

impl ValidatedExtension {
    pub fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }

    pub fn receipt(&self) -> &AdmissionReceipt {
        &self.receipt
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionReceipt {
    pub schema: String,
    pub extension: String,
    pub digest: String,
    pub host_version: String,
    pub api_version: String,
    pub granted_capabilities: Vec<Capability>,
    pub negotiated_features: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    UnknownSchema(String),
    InvalidApiVersion(String),
    InvalidHostRequirement(String),
    InvalidVersion(String),
    InvalidIdentity(String),
    InvalidEntry(String),
    InvalidContract(String),
    InvalidBudget(String),
    InvalidComponent(String),
    IncompatibleApi { extension: Version, host: Version },
    IncompatibleHost { requirement: String, host: Version },
    DigestMismatch { declared: String, actual: String },
    MissingFeature(String),
    CapabilityDenied(Capability),
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSchema(schema) => {
                write!(formatter, "unsupported extension schema `{schema}`")
            }
            Self::InvalidApiVersion(error) => {
                write!(formatter, "invalid OpenSDK api_version: {error}")
            }
            Self::InvalidHostRequirement(error) => {
                write!(formatter, "invalid host_version requirement: {error}")
            }
            Self::InvalidVersion(error) => write!(formatter, "invalid extension version: {error}"),
            Self::InvalidIdentity(error) => {
                write!(formatter, "invalid extension identity: {error}")
            }
            Self::InvalidEntry(error) => write!(formatter, "invalid extension entry: {error}"),
            Self::InvalidContract(error) => {
                write!(formatter, "invalid extension contract: {error}")
            }
            Self::InvalidBudget(error) => write!(formatter, "invalid extension budget: {error}"),
            Self::InvalidComponent(error) => {
                write!(formatter, "invalid extension component: {error}")
            }
            Self::IncompatibleApi { extension, host } => write!(
                formatter,
                "extension OpenSDK API {extension} is incompatible with host API {host}"
            ),
            Self::IncompatibleHost { requirement, host } => write!(
                formatter,
                "extension requires host `{requirement}`, current host is {host}"
            ),
            Self::DigestMismatch { declared, actual } => write!(
                formatter,
                "extension digest mismatch: declared {declared}, actual {actual}"
            ),
            Self::MissingFeature(feature) => write!(
                formatter,
                "required host feature `{feature}` is unavailable"
            ),
            Self::CapabilityDenied(capability) => write!(
                formatter,
                "capability `{}` was not granted",
                capability.as_str()
            ),
        }
    }
}

impl std::error::Error for AdmissionError {}

fn validate_manifest_shape(manifest: &ExtensionManifest) -> Result<(), AdmissionError> {
    if manifest.schema != OPENSDK_MANIFEST_SCHEMA {
        return Err(AdmissionError::UnknownSchema(manifest.schema.clone()));
    }
    validate_identifier(&manifest.identity.namespace, "namespace")?;
    validate_identifier(&manifest.identity.name, "name")?;
    validate_digest(&manifest.identity.digest)?;
    if manifest.identity.version.len() > 64
        || manifest.api_version.len() > 64
        || manifest.host_version.is_empty()
        || manifest.host_version.len() > 128
    {
        return Err(AdmissionError::InvalidVersion(
            "version fields exceed preview bounds".to_owned(),
        ));
    }
    validate_entry(manifest)?;
    ensure_sorted_unique(&manifest.imports, "imports", validate_contract_name)?;
    ensure_sorted_unique(&manifest.exports, "exports", validate_contract_name)?;
    ensure_sorted_unique(
        &manifest.required_features,
        "required_features",
        validate_feature,
    )?;
    ensure_sorted_unique(
        &manifest.optional_features,
        "optional_features",
        validate_feature,
    )?;
    if manifest
        .required_features
        .iter()
        .any(|feature| manifest.optional_features.binary_search(feature).is_ok())
    {
        return Err(AdmissionError::InvalidContract(
            "a feature cannot be both required and optional".to_owned(),
        ));
    }
    if !manifest
        .capabilities
        .windows(2)
        .all(|pair| pair[0] < pair[1])
    {
        return Err(AdmissionError::InvalidContract(
            "capabilities must be sorted and unique".to_owned(),
        ));
    }
    if manifest.plane != Plane::Browser
        && manifest
            .capabilities
            .iter()
            .any(|value| value.is_browser_only())
    {
        return Err(AdmissionError::InvalidContract(
            "browser capabilities are only valid in the browser plane".to_owned(),
        ));
    }
    if manifest.determinism == Determinism::Pure && !manifest.capabilities.is_empty() {
        return Err(AdmissionError::InvalidContract(
            "pure extensions cannot request capabilities".to_owned(),
        ));
    }
    if manifest.determinism == Determinism::RecordedEffect
        && !manifest.capabilities.contains(&Capability::EffectBroker)
    {
        return Err(AdmissionError::InvalidContract(
            "recorded-effect extensions must request effect-broker".to_owned(),
        ));
    }
    validate_lifecycle(manifest)?;
    validate_budget(&manifest.budgets)
}

fn validate_identifier(value: &str, label: &str) -> Result<(), AdmissionError> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AdmissionError::InvalidIdentity(format!(
            "{label} must be 1-64 lowercase ASCII letters, digits, or interior hyphens"
        )));
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), AdmissionError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(AdmissionError::InvalidIdentity(
            "digest must use sha256:<64 lowercase hex>".to_owned(),
        ));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AdmissionError::InvalidIdentity(
            "digest must use sha256:<64 lowercase hex>".to_owned(),
        ));
    }
    Ok(())
}

fn validate_entry(manifest: &ExtensionManifest) -> Result<(), AdmissionError> {
    let expected = match manifest.plane {
        Plane::Build | Plane::Server => EntryKind::WasmComponent,
        Plane::Browser => EntryKind::BrowserEsm,
        Plane::Tooling => EntryKind::JsonRpcProcess,
    };
    if manifest.entry.kind != expected {
        return Err(AdmissionError::InvalidEntry(format!(
            "plane {:?} requires entry kind {:?}",
            manifest.plane, expected
        )));
    }
    let path = manifest.entry.path.as_str();
    if path.is_empty()
        || path.len() > 1024
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(AdmissionError::InvalidEntry(
            "entry path must be a normalized relative slash path".to_owned(),
        ));
    }
    match manifest.entry.kind {
        EntryKind::WasmComponent => {
            let world = manifest.entry.world.as_deref().ok_or_else(|| {
                AdmissionError::InvalidEntry(
                    "Wasm component entries require a WIT world".to_owned(),
                )
            })?;
            validate_contract_name(world)?;
            if manifest.entry.custom_element.is_some() {
                return Err(AdmissionError::InvalidEntry(
                    "Wasm component entries cannot declare a Custom Element".to_owned(),
                ));
            }
        }
        EntryKind::BrowserEsm => {
            if manifest.entry.world.is_some() {
                return Err(AdmissionError::InvalidEntry(
                    "browser ESM entries cannot declare a WIT world".to_owned(),
                ));
            }
            validate_custom_element(manifest.entry.custom_element.as_deref().ok_or_else(
                || {
                    AdmissionError::InvalidEntry(
                        "browser ESM entries require customElement".to_owned(),
                    )
                },
            )?)?;
        }
        EntryKind::JsonRpcProcess => {
            if manifest.entry.world.is_some() || manifest.entry.custom_element.is_some() {
                return Err(AdmissionError::InvalidEntry(
                    "JSON-RPC entries cannot declare a WIT world or Custom Element".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_custom_element(value: &str) -> Result<(), AdmissionError> {
    const RESERVED: [&str; 8] = [
        "annotation-xml",
        "color-profile",
        "font-face",
        "font-face-src",
        "font-face-uri",
        "font-face-format",
        "font-face-name",
        "missing-glyph",
    ];
    if value.len() > 128
        || !value.contains('-')
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        || value.starts_with("xml")
        || RESERVED.contains(&value)
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_')
        })
    {
        return Err(AdmissionError::InvalidEntry(
            "customElement must be a non-reserved lowercase ASCII Custom Element name".to_owned(),
        ));
    }
    Ok(())
}

fn validate_lifecycle(manifest: &ExtensionManifest) -> Result<(), AdmissionError> {
    let lifecycle = &manifest.lifecycle;
    if !lifecycle.init || !lifecycle.dispose {
        return Err(AdmissionError::InvalidContract(
            "every preview extension must declare init and dispose".to_owned(),
        ));
    }
    if lifecycle.suspend != lifecycle.resume {
        return Err(AdmissionError::InvalidContract(
            "suspend and resume must be declared together".to_owned(),
        ));
    }
    if lifecycle.hmr && (!lifecycle.update || manifest.plane != Plane::Browser) {
        return Err(AdmissionError::InvalidContract(
            "HMR requires browser update lifecycle support".to_owned(),
        ));
    }
    Ok(())
}

fn validate_budget(budget: &Budget) -> Result<(), AdmissionError> {
    const MAX_CPU_MS: u64 = 60_000;
    const MAX_WALL_MS: u64 = 300_000;
    const MAX_MEMORY: u64 = 1024 * 1024 * 1024;
    const MAX_OUTPUT: u64 = 1024 * 1024 * 1024;
    if !(1..=MAX_CPU_MS).contains(&budget.cpu_ms)
        || !(1..=MAX_WALL_MS).contains(&budget.wall_time_ms)
        || !(64 * 1024..=MAX_MEMORY).contains(&budget.memory_bytes)
        || !(1..=MAX_OUTPUT).contains(&budget.output_bytes)
    {
        return Err(AdmissionError::InvalidBudget(
            "budgets exceed preview bounds".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_sorted_unique(
    values: &[String],
    label: &str,
    validate: fn(&str) -> Result<(), AdmissionError>,
) -> Result<(), AdmissionError> {
    if values.len() > 256 || !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(AdmissionError::InvalidContract(format!(
            "{label} must contain at most 256 sorted unique values"
        )));
    }
    for value in values {
        validate(value)?;
    }
    Ok(())
}

fn validate_contract_name(value: &str) -> Result<(), AdmissionError> {
    if value.is_empty()
        || value.len() > 256
        || value.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(AdmissionError::InvalidContract(
            "contract names must contain 1-256 non-control bytes".to_owned(),
        ));
    }
    Ok(())
}

fn validate_feature(value: &str) -> Result<(), AdmissionError> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AdmissionError::InvalidContract(
            "feature IDs use 1-64 lowercase ASCII letters, digits, and interior hyphens".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(bytes: &[u8]) -> ExtensionManifest {
        ExtensionManifest {
            schema: OPENSDK_MANIFEST_SCHEMA.to_owned(),
            api_version: OPENSDK_API_VERSION.to_owned(),
            host_version: ">=0.1.0-preview.1, <0.2.0".to_owned(),
            plane: Plane::Build,
            identity: ExtensionIdentity {
                namespace: "celiums".to_owned(),
                name: "uppercase".to_owned(),
                version: "0.1.0".to_owned(),
                digest: format!("sha256:{:x}", Sha256::digest(bytes)),
            },
            entry: ExtensionEntry {
                kind: EntryKind::WasmComponent,
                path: "component.wasm".to_owned(),
                world: Some("pliego:build/transformer@0.1.0".to_owned()),
                custom_element: None,
            },
            determinism: Determinism::Pure,
            imports: Vec::new(),
            exports: vec!["pliego:build/transform".to_owned()],
            capabilities: Vec::new(),
            required_features: Vec::new(),
            optional_features: vec!["stream-input".to_owned()],
            budgets: Budget {
                cpu_ms: 100,
                wall_time_ms: 500,
                memory_bytes: 16 * 1024 * 1024,
                output_bytes: 1024 * 1024,
            },
            lifecycle: Lifecycle {
                init: true,
                update: false,
                suspend: false,
                resume: false,
                dispose: true,
                hmr: false,
            },
        }
    }

    #[test]
    fn admission_binds_bytes_versions_and_features() {
        let bytes = b"component";
        let host = HostContract::preview(
            Version::parse("0.1.0-preview.1").unwrap(),
            CapabilityPolicy::deny_all(),
        )
        .with_feature("stream-input");
        let admitted = host.admit(manifest(bytes), bytes).unwrap();
        assert_eq!(admitted.receipt().negotiated_features, ["stream-input"]);
        assert_eq!(admitted.manifest().identity.name, "uppercase");
    }

    #[test]
    fn incompatible_or_tampered_extensions_fail_before_admission() {
        let host = HostContract::preview(
            Version::parse("0.1.0-preview.1").unwrap(),
            CapabilityPolicy::deny_all(),
        );
        assert!(matches!(
            host.admit(manifest(b"component"), b"tampered"),
            Err(AdmissionError::DigestMismatch { .. })
        ));
        let mut incompatible = manifest(b"component");
        incompatible.host_version = ">=1.0.0".to_owned();
        assert!(matches!(
            host.admit(incompatible, b"component"),
            Err(AdmissionError::IncompatibleHost { .. })
        ));
    }

    #[test]
    fn pure_extensions_and_undeclared_capabilities_fail_closed() {
        let bytes = b"component";
        let host = HostContract::preview(
            Version::parse("0.1.0-preview.1").unwrap(),
            CapabilityPolicy::deny_all(),
        );
        let mut overpowered = manifest(bytes);
        overpowered.determinism = Determinism::RecordedEffect;
        overpowered.capabilities = vec![Capability::Network];
        assert!(matches!(
            host.admit(overpowered, bytes),
            Err(AdmissionError::InvalidContract(_))
        ));

        let mut denied = manifest(bytes);
        denied.determinism = Determinism::RecordedEffect;
        denied.capabilities = vec![Capability::Network, Capability::EffectBroker];
        denied.capabilities.sort();
        assert!(matches!(
            host.admit(denied, bytes),
            Err(AdmissionError::CapabilityDenied(_))
        ));
    }

    #[test]
    fn browser_identity_and_lifecycle_are_explicit_and_bounded() {
        let bytes = b"export const pliegoComponent = {}";
        let mut browser = manifest(bytes);
        browser.plane = Plane::Browser;
        browser.entry.kind = EntryKind::BrowserEsm;
        browser.entry.world = None;
        browser.entry.custom_element = Some("pliego-status".to_owned());
        browser.determinism = Determinism::NativeTrusted;
        browser.capabilities = vec![Capability::Dom];
        browser.exports = vec!["pliegoComponent".to_owned()];
        browser.lifecycle.update = true;
        browser.lifecycle.hmr = true;
        let host = HostContract::preview(
            Version::parse(OPENSDK_API_VERSION).unwrap(),
            CapabilityPolicy::deny_all().grant(Capability::Dom),
        );
        host.admit(browser.clone(), bytes).unwrap();

        browser.entry.custom_element = Some("missing-glyph".to_owned());
        assert!(matches!(
            host.admit(browser.clone(), bytes),
            Err(AdmissionError::InvalidEntry(_))
        ));
        browser.entry.custom_element = Some("pliego-status".to_owned());
        browser.lifecycle.dispose = false;
        assert!(matches!(
            host.admit(browser.clone(), bytes),
            Err(AdmissionError::InvalidContract(_))
        ));
        browser.lifecycle.dispose = true;
        browser.required_features = vec!["INVALID".to_owned()];
        assert!(matches!(
            host.admit(browser, bytes),
            Err(AdmissionError::InvalidContract(_))
        ));
    }

    #[test]
    fn manifest_json_requires_every_contract_vector() {
        let bytes = b"fixture";
        let mut value = serde_json::to_value(manifest(bytes)).unwrap();
        value.as_object_mut().unwrap().remove("imports");
        assert!(serde_json::from_value::<ExtensionManifest>(value).is_err());
    }

    #[test]
    fn negotiated_features_are_canonical_across_required_and_optional_sets() {
        let bytes = b"component";
        let mut extension = manifest(bytes);
        extension.required_features = vec!["z-feature".to_owned()];
        extension.optional_features = vec!["a-feature".to_owned()];
        let host = HostContract::preview(
            Version::parse(OPENSDK_API_VERSION).unwrap(),
            CapabilityPolicy::deny_all(),
        )
        .with_feature("z-feature")
        .with_feature("a-feature");
        let admitted = host.admit(extension, bytes).unwrap();
        assert_eq!(
            admitted.receipt().negotiated_features,
            ["a-feature", "z-feature"]
        );
    }
}
