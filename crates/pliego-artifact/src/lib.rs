// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

#![forbid(unsafe_code)]

//! Portable output namespaces and receipts that are verified from disk.

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, File as CapFile, OpenOptions as CapOpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

pub const BUILD_LEDGER_NAME: &str = "pliego.build.json";
pub const BUILD_GRAPH_NAME: &str = "pliego.graph.json";
pub const BUILD_GRAPH_VERSION: &str = "1.0.0";
pub const BUILD_CONTEXT_ENV: &str = "PLIEGO_BUILD_CONTEXT";
pub const BUILD_REPORT_VERSION: &str = "2.0.0";
pub const RECEIPT_VERSION: &str = "2.0.0";
pub const NAMESPACE_VERSION: &str = "1.0.0";
pub const MAX_LEDGER_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_CONTEXT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_BUILD_GRAPH_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_ARTIFACT_FILES: usize = 100_000;
pub const MAX_OUTPUT_FILE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_OUTPUT_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_NAMESPACE_COMPONENTS: usize = 262_144;
pub const MAX_NAMESPACE_PREFIX_BYTES: usize = 16 * 1024 * 1024;
const MAX_PORTABLE_PATH_BYTES: usize = 4096;
const MAX_COMPONENT_BYTES: usize = 255;
const MAX_PATH_COMPONENTS: usize = 128;

#[derive(Debug)]
pub enum ArtifactError {
    InvalidPath(String),
    NamespaceCollision {
        path: String,
        existing: String,
        detail: &'static str,
    },
    InvalidReceipt(String),
    Missing(String),
    Extra(String),
    Modified(String),
    Unstable(String),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json(serde_json::Error),
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => write!(formatter, "non-portable artifact path: {path}"),
            Self::NamespaceCollision {
                path,
                existing,
                detail,
            } => write!(
                formatter,
                "artifact namespace collision between {path:?} and {existing:?}: {detail}"
            ),
            Self::InvalidReceipt(message) => {
                write!(formatter, "invalid artifact receipt: {message}")
            }
            Self::Missing(path) => write!(formatter, "declared artifact is missing: {path}"),
            Self::Extra(path) => write!(formatter, "undeclared artifact exists: {path}"),
            Self::Modified(path) => {
                write!(formatter, "artifact bytes do not match receipt: {path}")
            }
            Self::Unstable(path) => write!(formatter, "artifact changed while hashing: {path}"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json(source) => write!(formatter, "invalid artifact JSON: {source}"),
        }
    }
}

impl std::error::Error for ArtifactError {}

impl From<serde_json::Error> for ArtifactError {
    fn from(source: serde_json::Error) -> Self {
        Self::Json(source)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PortablePath {
    value: String,
    collision_key: String,
    component_keys: Vec<String>,
}

impl PortablePath {
    pub fn parse(value: &str) -> Result<Self, ArtifactError> {
        if value.is_empty()
            || value.len() > MAX_PORTABLE_PATH_BYTES
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains('\\')
        {
            return Err(ArtifactError::InvalidPath(value.to_owned()));
        }
        let normalized: String = value.nfc().collect();
        if normalized.len() > MAX_PORTABLE_PATH_BYTES {
            return Err(ArtifactError::InvalidPath(value.to_owned()));
        }
        let components = normalized.split('/').collect::<Vec<_>>();
        if components.len() > MAX_PATH_COMPONENTS {
            return Err(ArtifactError::InvalidPath(value.to_owned()));
        }
        let mut component_keys = Vec::with_capacity(components.len());
        for component in components {
            validate_component(component, value)?;
            component_keys.push(portable_case_key(component));
        }
        let collision_key = component_keys.join("/");
        Ok(Self {
            value: normalized,
            collision_key,
            component_keys,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn collision_key(&self) -> &str {
        &self.collision_key
    }
}

impl fmt::Display for PortablePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

fn portable_case_key(value: &str) -> String {
    value.nfkc().case_fold().nfkc().collect()
}

fn validate_component(component: &str, full_path: &str) -> Result<(), ArtifactError> {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.len() > MAX_COMPONENT_BYTES
        || component.ends_with(['.', ' '])
        || component.chars().any(|character| {
            character.is_control()
                || matches!(character, '\0' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return Err(ArtifactError::InvalidPath(full_path.to_owned()));
    }
    let stem = component.split('.').next().unwrap_or(component);
    let reserved = portable_case_key(stem);
    if matches!(reserved.as_str(), "con" | "prn" | "aux" | "nul")
        || reserved.strip_prefix("com").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || reserved.strip_prefix("lpt").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
    {
        return Err(ArtifactError::InvalidPath(full_path.to_owned()));
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct OutputNamespace {
    files: BTreeMap<String, (String, String)>,
    directories: BTreeMap<String, String>,
    components: usize,
    prefix_bytes: usize,
}

impl OutputNamespace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_str(
        &mut self,
        path: &str,
        owner: impl Into<String>,
    ) -> Result<PortablePath, ArtifactError> {
        let path = PortablePath::parse(path)?;
        self.insert(path.clone(), owner)?;
        Ok(path)
    }

    pub fn insert(
        &mut self,
        path: PortablePath,
        owner: impl Into<String>,
    ) -> Result<(), ArtifactError> {
        let component_budget = self
            .components
            .checked_add(path.component_keys.len())
            .filter(|components| *components <= MAX_NAMESPACE_COMPONENTS)
            .ok_or_else(|| {
                ArtifactError::InvalidReceipt(format!(
                    "artifact namespace exceeds {MAX_NAMESPACE_COMPONENTS} path components"
                ))
            })?;
        if let Some((existing, _)) = self.files.get(path.collision_key()) {
            return Err(ArtifactError::NamespaceCollision {
                path: path.value,
                existing: existing.clone(),
                detail: "two files resolve to the same portable path",
            });
        }

        let components = path.value.split('/').collect::<Vec<_>>();
        let mut key_prefix = String::new();
        let mut spelling_prefix = String::new();
        for (index, (component, component_key)) in components
            .iter()
            .zip(&path.component_keys)
            .enumerate()
            .take(components.len().saturating_sub(1))
        {
            if index > 0 {
                key_prefix.push('/');
                spelling_prefix.push('/');
            }
            key_prefix.push_str(component_key);
            spelling_prefix.push_str(component);
            if let Some((existing, _)) = self.files.get(&key_prefix) {
                return Err(ArtifactError::NamespaceCollision {
                    path: path.value.clone(),
                    existing: existing.clone(),
                    detail: "a file is also used as a directory",
                });
            }
            match self.directories.get(&key_prefix) {
                Some(existing) if existing != &spelling_prefix => {
                    return Err(ArtifactError::NamespaceCollision {
                        path: path.value.clone(),
                        existing: existing.clone(),
                        detail: "a directory has ambiguous case or Unicode spelling",
                    });
                }
                Some(_) => {}
                None => {
                    self.prefix_bytes = self
                        .prefix_bytes
                        .checked_add(key_prefix.len())
                        .and_then(|bytes| bytes.checked_add(spelling_prefix.len()))
                        .filter(|bytes| *bytes <= MAX_NAMESPACE_PREFIX_BYTES)
                        .ok_or_else(|| {
                            ArtifactError::InvalidReceipt(format!(
                                "artifact namespace exceeds {MAX_NAMESPACE_PREFIX_BYTES} stored prefix bytes"
                            ))
                        })?;
                    self.directories
                        .insert(key_prefix.clone(), spelling_prefix.clone());
                }
            }
        }
        if let Some(existing) = self.directories.get(path.collision_key()) {
            return Err(ArtifactError::NamespaceCollision {
                path: path.value,
                existing: existing.clone(),
                detail: "a file is also used as a directory",
            });
        }
        self.files
            .insert(path.collision_key, (path.value, owner.into()));
        self.components = component_budget;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolchainEvidence {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ownership {
    pub project_id: String,
    pub site_package: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameworkEvidence {
    pub version: String,
    pub source_revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputMaterial {
    pub id: String,
    pub kind: String,
    pub selection: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputMaterialSpec {
    pub id: String,
    pub kind: String,
    pub root: PathBuf,
    pub included_paths: Vec<String>,
    pub excluded_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildContext {
    pub ownership: Ownership,
    pub framework: FrameworkEvidence,
    pub toolchain: Vec<ToolchainEvidence>,
    pub configuration: Vec<EvidenceFile>,
    pub sources: Vec<EvidenceFile>,
    pub materials: Vec<InputMaterial>,
    pub source_set_sha256: String,
    pub excluded_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildInvocation {
    pub context: BuildContext,
    pub project_root: PathBuf,
    pub output_path: String,
    pub material_specs: Vec<InputMaterialSpec>,
}

impl InputMaterialSpec {
    pub fn tree(
        id: impl Into<String>,
        kind: impl Into<String>,
        root: impl AsRef<Path>,
        excluded_paths: Vec<String>,
    ) -> Result<Self, ArtifactError> {
        Self::new(id, kind, root, Vec::new(), excluded_paths)
    }

    pub fn files(
        id: impl Into<String>,
        kind: impl Into<String>,
        root: impl AsRef<Path>,
        included_paths: Vec<String>,
    ) -> Result<Self, ArtifactError> {
        Self::new(id, kind, root, included_paths, Vec::new())
    }

    fn new(
        id: impl Into<String>,
        kind: impl Into<String>,
        root: impl AsRef<Path>,
        included_paths: Vec<String>,
        excluded_paths: Vec<String>,
    ) -> Result<Self, ArtifactError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|source| ArtifactError::Io {
                path: root.as_ref().to_owned(),
                source,
            })?;
        let mut spec = Self {
            id: id.into(),
            kind: kind.into(),
            root,
            included_paths,
            excluded_paths,
        };
        normalize_material_specs(std::slice::from_mut(&mut spec))?;
        Ok(spec)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputFile {
    pub path: String,
    pub kind: String,
    pub producer: String,
    pub bytes: u64,
    pub sha256: String,
}

/// A project source captured by the build receipt and addressable by the
/// causal build graph.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphSource {
    pub path: String,
    pub sha256: String,
}

/// Source dependencies for a route or standalone artifact.
///
/// `AllSources` is the compatibility fallback for producers that have not yet
/// declared precise inputs. It is deliberately visible rather than pretending
/// that an imprecise edge is incremental.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceDependencies {
    AllSources,
    Explicit { paths: Vec<String> },
}

/// One public route and the artifact paths it emits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphRoute {
    pub route: String,
    pub sources: SourceDependencies,
    pub artifacts: Vec<String>,
}

/// One emitted payload and its direct or route-mediated source dependencies.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphArtifact {
    pub path: String,
    pub kind: String,
    pub producer: String,
    pub route: Option<String>,
    pub sources: SourceDependencies,
    pub sha256: String,
}

/// Deterministic source -> route -> artifact evidence emitted beside the build
/// receipt. The graph itself is an output covered by that receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildGraph {
    pub graph_version: String,
    pub project_id: String,
    pub source_set_sha256: String,
    pub sources: Vec<GraphSource>,
    pub routes: Vec<GraphRoute>,
    pub artifacts: Vec<GraphArtifact>,
}

impl OutputFile {
    pub fn new(
        path: impl Into<String>,
        kind: impl Into<String>,
        producer: impl Into<String>,
        bytes: &[u8],
    ) -> Self {
        Self {
            path: path.into(),
            kind: kind.into(),
            producer: producer.into(),
            bytes: bytes.len() as u64,
            sha256: sha256_bytes(bytes),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputSet {
    pub files: Vec<OutputFile>,
    pub file_count: u64,
    pub total_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplacementPolicy {
    pub required_previous_project_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviousOwnership {
    pub project_id: String,
    pub site_package: String,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactReceipt {
    pub receipt_version: String,
    pub namespace_version: String,
    pub context: BuildContext,
    pub replacement_policy: ReplacementPolicy,
    pub previous_ownership: Option<PreviousOwnership>,
    pub outputs: OutputSet,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildReport {
    pub report_version: String,
    pub receipt_sha256: String,
    pub receipt: ArtifactReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedBuild {
    pub report: BuildReport,
    pub files: u64,
    pub bytes: u64,
}

impl ArtifactReceipt {
    pub fn from_context_and_files(
        context: BuildContext,
        files: Vec<OutputFile>,
    ) -> Result<Self, ArtifactError> {
        Self::from_context_files_and_previous(context, files, None)
    }

    pub fn from_context_files_and_previous(
        mut context: BuildContext,
        mut files: Vec<OutputFile>,
        previous_ownership: Option<PreviousOwnership>,
    ) -> Result<Self, ArtifactError> {
        normalize_context(&mut context)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        validate_outputs(&files)?;
        let file_count = files.len() as u64;
        let total_bytes = files.iter().try_fold(0_u64, |total, file| {
            total.checked_add(file.bytes).ok_or_else(|| {
                ArtifactError::InvalidReceipt("output byte count overflowed u64".to_owned())
            })
        })?;
        let sha256 = hash_json(&files)?;
        let replacement_policy = ReplacementPolicy {
            required_previous_project_id: context.ownership.project_id.clone(),
        };
        Ok(Self {
            receipt_version: RECEIPT_VERSION.to_owned(),
            namespace_version: NAMESPACE_VERSION.to_owned(),
            context,
            replacement_policy,
            previous_ownership,
            outputs: OutputSet {
                files,
                file_count,
                total_bytes,
                sha256,
            },
        })
    }

    pub fn same_artifact_core(&self, other: &Self) -> bool {
        self.receipt_version == other.receipt_version
            && self.namespace_version == other.namespace_version
            && self.context == other.context
            && self.replacement_policy == other.replacement_policy
            && self.outputs == other.outputs
    }
}

impl BuildReport {
    pub fn new(receipt: ArtifactReceipt) -> Result<Self, ArtifactError> {
        let receipt_sha256 = hash_json(&receipt)?;
        let report = Self {
            report_version: BUILD_REPORT_VERSION.to_owned(),
            receipt_sha256,
            receipt,
        };
        validate_report(&report)?;
        Ok(report)
    }
}

pub fn write_build_report(path: &Path, report: &BuildReport) -> Result<(), ArtifactError> {
    let bytes = encode_build_report(report)?;
    write_bounded_json(path, &bytes, MAX_LEDGER_BYTES)
}

pub fn encode_build_report(report: &BuildReport) -> Result<Vec<u8>, ArtifactError> {
    validate_report(report)?;
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_LEDGER_BYTES {
        return Err(ArtifactError::InvalidReceipt(format!(
            "build report exceeds {MAX_LEDGER_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

pub fn encode_build_graph(graph: &BuildGraph) -> Result<Vec<u8>, ArtifactError> {
    validate_build_graph(graph)?;
    let mut bytes = serde_json::to_vec_pretty(graph)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_BUILD_GRAPH_BYTES {
        return Err(ArtifactError::InvalidReceipt(format!(
            "build graph exceeds {MAX_BUILD_GRAPH_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

pub fn decode_build_graph(bytes: &[u8]) -> Result<BuildGraph, ArtifactError> {
    if bytes.len() as u64 > MAX_BUILD_GRAPH_BYTES {
        return Err(ArtifactError::InvalidReceipt(format!(
            "build graph exceeds {MAX_BUILD_GRAPH_BYTES} bytes"
        )));
    }
    let graph: BuildGraph = serde_json::from_slice(bytes)?;
    validate_build_graph(&graph)?;
    Ok(graph)
}

pub fn read_build_report(output_root: &Path) -> Result<BuildReport, ArtifactError> {
    let bytes = read_regular_bounded(
        &output_root.join(BUILD_LEDGER_NAME),
        MAX_LEDGER_BYTES,
        "build ledger",
    )?;
    let report: BuildReport = match serde_json::from_slice(&bytes) {
        Ok(report) => report,
        Err(source) => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                let version = value.get("reportVersion").and_then(|value| value.as_str());
                if version.is_some_and(|version| version != BUILD_REPORT_VERSION) {
                    return Err(ArtifactError::InvalidReceipt(format!(
                        "unsupported build report version {version:?}; move the legacy output aside and rebuild with the current Pliego CLI"
                    )));
                }
            }
            return Err(ArtifactError::Json(source));
        }
    };
    validate_report(&report)?;
    Ok(report)
}

pub fn write_build_context(path: &Path, context: &BuildContext) -> Result<(), ArtifactError> {
    let mut context = context.clone();
    normalize_context(&mut context)?;
    let mut bytes = serde_json::to_vec_pretty(&context)?;
    bytes.push(b'\n');
    write_bounded_json(path, &bytes, MAX_CONTEXT_BYTES)
}

pub fn read_build_context(path: &Path) -> Result<BuildContext, ArtifactError> {
    let bytes = read_regular_bounded(path, MAX_CONTEXT_BYTES, "build context")?;
    let original: BuildContext = serde_json::from_slice(&bytes)?;
    let mut context = original.clone();
    normalize_context(&mut context)?;
    if context != original {
        return Err(ArtifactError::InvalidReceipt(
            "build context is not canonically ordered".to_owned(),
        ));
    }
    Ok(context)
}

pub fn context_from_environment() -> Result<Option<BuildContext>, ArtifactError> {
    invocation_from_environment().map(|invocation| invocation.map(|invocation| invocation.context))
}

pub fn write_build_invocation(
    path: &Path,
    invocation: &BuildInvocation,
) -> Result<(), ArtifactError> {
    let mut invocation = invocation.clone();
    normalize_context(&mut invocation.context)?;
    normalize_material_specs(&mut invocation.material_specs)?;
    validate_invocation(&invocation)?;
    let mut bytes = serde_json::to_vec_pretty(&invocation)?;
    bytes.push(b'\n');
    write_bounded_json(path, &bytes, MAX_CONTEXT_BYTES)
}

pub fn read_build_invocation(path: &Path) -> Result<BuildInvocation, ArtifactError> {
    let bytes = read_regular_bounded(path, MAX_CONTEXT_BYTES, "build invocation")?;
    let original: BuildInvocation = serde_json::from_slice(&bytes)?;
    let mut invocation = original.clone();
    normalize_context(&mut invocation.context)?;
    normalize_material_specs(&mut invocation.material_specs)?;
    if invocation != original {
        return Err(ArtifactError::InvalidReceipt(
            "build invocation is not canonically ordered".to_owned(),
        ));
    }
    validate_invocation(&invocation)?;
    Ok(invocation)
}

pub fn invocation_from_environment() -> Result<Option<BuildInvocation>, ArtifactError> {
    let Some(path) = std::env::var_os(BUILD_CONTEXT_ENV) else {
        return Ok(None);
    };
    Ok(Some(read_build_invocation(Path::new(&path))?))
}

pub fn verify_build_report(output_root: &Path) -> Result<VerifiedBuild, ArtifactError> {
    validate_root_directory(output_root)?;
    let report = read_build_report(output_root)?;
    let actual = collect_output_layout(output_root)?;
    let expected = report
        .receipt
        .outputs
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();

    for path in expected.keys() {
        if !actual.files.contains(*path) {
            return Err(ArtifactError::Missing((*path).to_owned()));
        }
    }
    for path in &actual.files {
        if !expected.contains_key(path.as_str()) {
            return Err(ArtifactError::Extra(path.clone()));
        }
    }
    let expected_directories = expected_output_directories(expected.keys().copied())?;
    for directory in &actual.directories {
        if !expected_directories.contains(directory) {
            return Err(ArtifactError::Extra(format!("{directory}/")));
        }
    }
    for path in &actual.files {
        let expected_file = expected[path.as_str()];
        let actual_file = evidence_for_output_relative(output_root, expected_file)?;
        if actual_file.bytes != expected_file.bytes || actual_file.sha256 != expected_file.sha256 {
            return Err(ArtifactError::Modified(path.clone()));
        }
    }
    let files = actual.files.len() as u64;
    let bytes = report.receipt.outputs.total_bytes;
    Ok(VerifiedBuild {
        report,
        files,
        bytes,
    })
}

pub fn capture_build_context(
    project_root: &Path,
    ownership: Ownership,
    framework: FrameworkEvidence,
    configuration_paths: &[String],
    excluded_paths: &[String],
) -> Result<BuildContext, ArtifactError> {
    validate_root_directory(project_root)?;
    validate_ownership(&ownership)?;
    let mut excluded = Vec::new();
    for path in excluded_paths {
        excluded.push(PortablePath::parse(path)?.as_str().to_owned());
    }
    excluded.sort();
    excluded.dedup();

    let configuration_set = configuration_paths
        .iter()
        .map(|path| PortablePath::parse(path).map(|path| path.as_str().to_owned()))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut configuration = Vec::new();
    for path in &configuration_set {
        configuration.push(evidence_for_relative(project_root, path)?);
    }

    let mut sources = Vec::new();
    collect_project_sources(
        project_root,
        project_root,
        &configuration_set,
        &excluded,
        &mut sources,
    )?;
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    if sources.len() > MAX_ARTIFACT_FILES {
        return Err(ArtifactError::InvalidReceipt(format!(
            "source set exceeds {MAX_ARTIFACT_FILES} files"
        )));
    }
    let materials = Vec::new();
    let source_set_sha256 = source_set_sha256(&sources, &materials)?;
    let toolchain = vec![
        toolchain_version(project_root, "rustc", &["-vV"])?,
        toolchain_version(project_root, "cargo", &["-Vv"])?,
    ];
    let mut context = BuildContext {
        ownership,
        framework,
        toolchain,
        configuration,
        sources,
        materials,
        source_set_sha256,
        excluded_paths: excluded,
    };
    normalize_context(&mut context)?;
    Ok(context)
}

pub fn capture_build_context_with_materials(
    project_root: &Path,
    ownership: Ownership,
    framework: FrameworkEvidence,
    configuration_paths: &[String],
    excluded_paths: &[String],
    material_specs: &[InputMaterialSpec],
) -> Result<BuildContext, ArtifactError> {
    let mut context = capture_build_context(
        project_root,
        ownership,
        framework,
        configuration_paths,
        excluded_paths,
    )?;
    let mut specs = material_specs.to_vec();
    normalize_material_specs(&mut specs)?;
    context.materials = specs
        .iter()
        .map(capture_input_material)
        .collect::<Result<Vec<_>, _>>()?;
    let file_count = context
        .materials
        .iter()
        .try_fold(context.sources.len() as u64, |total, material| {
            total.checked_add(material.file_count)
        });
    if file_count.is_none_or(|count| count > MAX_ARTIFACT_FILES as u64) {
        return Err(ArtifactError::InvalidReceipt(format!(
            "input set exceeds {MAX_ARTIFACT_FILES} files"
        )));
    }
    context.source_set_sha256 = source_set_sha256(&context.sources, &context.materials)?;
    normalize_context(&mut context)?;
    Ok(context)
}

pub fn verify_build_context(
    project_root: &Path,
    expected: &BuildContext,
) -> Result<(), ArtifactError> {
    verify_build_context_with_materials(project_root, &[], expected)
}

pub fn verify_build_context_with_materials(
    project_root: &Path,
    material_specs: &[InputMaterialSpec],
    expected: &BuildContext,
) -> Result<(), ArtifactError> {
    let configuration = expected
        .configuration
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let mut current = capture_build_context_with_materials(
        project_root,
        expected.ownership.clone(),
        expected.framework.clone(),
        &configuration,
        &expected.excluded_paths,
        material_specs,
    )?;
    for toolchain in &expected.toolchain {
        if current
            .toolchain
            .iter()
            .any(|current| current.name == toolchain.name)
        {
            continue;
        }
        let refreshed = match toolchain.name.as_str() {
            "wasm-bindgen" => toolchain_version(project_root, "wasm-bindgen", &["--version"]),
            name => {
                return Err(ArtifactError::InvalidReceipt(format!(
                    "cannot recalculate unknown toolchain {name:?}"
                )));
            }
        }?;
        current.toolchain.push(refreshed);
    }
    current
        .toolchain
        .sort_by(|left, right| left.name.cmp(&right.name));
    if &current != expected {
        return Err(ArtifactError::Modified(
            "build configuration, sources, or toolchain".to_owned(),
        ));
    }
    Ok(())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_report(report: &BuildReport) -> Result<(), ArtifactError> {
    if report.report_version != BUILD_REPORT_VERSION {
        return Err(ArtifactError::InvalidReceipt(format!(
            "unsupported report version {:?}",
            report.report_version
        )));
    }
    if report.receipt.receipt_version != RECEIPT_VERSION
        || report.receipt.namespace_version != NAMESPACE_VERSION
    {
        return Err(ArtifactError::InvalidReceipt(
            "unsupported receipt or namespace version".to_owned(),
        ));
    }
    let mut context = report.receipt.context.clone();
    normalize_context(&mut context)?;
    if context != report.receipt.context {
        return Err(ArtifactError::InvalidReceipt(
            "context is not canonically ordered".to_owned(),
        ));
    }
    validate_outputs(&report.receipt.outputs.files)?;
    let total_bytes = report
        .receipt
        .outputs
        .files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.bytes))
        .ok_or_else(|| {
            ArtifactError::InvalidReceipt("output byte count overflowed u64".to_owned())
        })?;
    if report.receipt.outputs.file_count != report.receipt.outputs.files.len() as u64
        || report.receipt.outputs.total_bytes != total_bytes
        || report.receipt.outputs.sha256 != hash_json(&report.receipt.outputs.files)?
    {
        return Err(ArtifactError::InvalidReceipt(
            "output-set aggregate does not match declared files".to_owned(),
        ));
    }
    if report.receipt_sha256 != hash_json(&report.receipt)? {
        return Err(ArtifactError::InvalidReceipt(
            "receipt SHA-256 mismatch".to_owned(),
        ));
    }
    validate_project_id(
        &report
            .receipt
            .replacement_policy
            .required_previous_project_id,
    )?;
    if report
        .receipt
        .replacement_policy
        .required_previous_project_id
        != report.receipt.context.ownership.project_id
    {
        return Err(ArtifactError::InvalidReceipt(
            "previous ownership policy must match the receipt project".to_owned(),
        ));
    }
    if let Some(previous) = &report.receipt.previous_ownership {
        validate_project_id(&previous.project_id)?;
        validate_sha256(&previous.receipt_sha256, "previous receipt")?;
        if previous.site_package.trim().is_empty()
            || previous.project_id
                != report
                    .receipt
                    .replacement_policy
                    .required_previous_project_id
            || previous.receipt_sha256 == report.receipt_sha256
        {
            return Err(ArtifactError::InvalidReceipt(
                "previous ownership must identify a distinct receipt from the required project"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_invocation(invocation: &BuildInvocation) -> Result<(), ArtifactError> {
    validate_absolute_canonical_root(&invocation.project_root, "build invocation project root")?;
    let output_path = PortablePath::parse(&invocation.output_path)?;
    if output_path.as_str() != invocation.output_path {
        return Err(ArtifactError::InvalidReceipt(
            "build invocation output path is not canonical NFC".to_owned(),
        ));
    }
    if !invocation
        .context
        .excluded_paths
        .iter()
        .any(|excluded| excluded == output_path.as_str())
    {
        return Err(ArtifactError::InvalidReceipt(
            "build invocation output path is not excluded from its input context".to_owned(),
        ));
    }
    if invocation.material_specs.len() != invocation.context.materials.len() {
        return Err(ArtifactError::InvalidReceipt(
            "build invocation material roots do not match its receipt context".to_owned(),
        ));
    }
    for (spec, material) in invocation
        .material_specs
        .iter()
        .zip(&invocation.context.materials)
    {
        if spec.id != material.id
            || spec.kind != material.kind
            || material.selection != material_selection(spec)
        {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build invocation material spec {:?} does not match its evidence",
                spec.id
            )));
        }
    }
    let tree_roots = invocation
        .material_specs
        .iter()
        .filter(|spec| spec.included_paths.is_empty())
        .map(|spec| spec.root.as_path())
        .collect::<Vec<_>>();
    for (index, root) in tree_roots.iter().enumerate() {
        if root.starts_with(&invocation.project_root)
            || invocation.project_root.starts_with(root)
            || tree_roots[index + 1..]
                .iter()
                .any(|other| root.starts_with(other) || other.starts_with(root))
        {
            return Err(ArtifactError::InvalidReceipt(
                "recursive material roots cannot overlap each other or the project root".to_owned(),
            ));
        }
    }
    Ok(())
}

fn normalize_material_specs(specs: &mut [InputMaterialSpec]) -> Result<(), ArtifactError> {
    for spec in specs.iter_mut() {
        validate_material_token(&spec.id, "material id")?;
        validate_material_token(&spec.kind, "material kind")?;
        validate_absolute_canonical_root(&spec.root, "material root")?;
        normalize_portable_paths(&mut spec.included_paths, "included material path")?;
        normalize_portable_paths(&mut spec.excluded_paths, "excluded material path")?;
        let mut included_namespace = OutputNamespace::new();
        for path in &spec.included_paths {
            included_namespace.insert_str(path, "included material path")?;
        }
        let mut excluded_keys = BTreeSet::new();
        for path in &spec.excluded_paths {
            let portable = PortablePath::parse(path)?;
            if !excluded_keys.insert(portable.collision_key().to_owned()) {
                return Err(ArtifactError::InvalidReceipt(
                    "excluded material paths contain a portable alias".to_owned(),
                ));
            }
        }
        if !spec.included_paths.is_empty() && !spec.excluded_paths.is_empty() {
            return Err(ArtifactError::InvalidReceipt(format!(
                "material {:?} cannot combine an exact file selection with exclusions",
                spec.id
            )));
        }
    }
    specs.sort_by(|left, right| left.id.cmp(&right.id));
    if specs.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(ArtifactError::InvalidReceipt(
            "material ids must be unique".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_portable_paths(paths: &mut [String], label: &str) -> Result<(), ArtifactError> {
    for path in paths.iter_mut() {
        let portable = PortablePath::parse(path)?;
        if portable.as_str() != path {
            return Err(ArtifactError::InvalidReceipt(format!(
                "{label} is not canonical NFC: {path:?}"
            )));
        }
    }
    paths.sort();
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ArtifactError::InvalidReceipt(format!(
            "{label}s must be unique"
        )));
    }
    Ok(())
}

fn validate_material_token(value: &str, label: &str) -> Result<(), ArtifactError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'@' | b'+' | b'/')
        })
        || value.contains("..")
    {
        return Err(ArtifactError::InvalidReceipt(format!(
            "invalid {label} {value:?}"
        )));
    }
    Ok(())
}

fn capture_input_material(spec: &InputMaterialSpec) -> Result<InputMaterial, ArtifactError> {
    let mut files = if spec.included_paths.is_empty() {
        let mut files = Vec::new();
        collect_project_sources(
            &spec.root,
            &spec.root,
            &BTreeSet::new(),
            &spec.excluded_paths,
            &mut files,
        )?;
        files
    } else {
        spec.included_paths
            .iter()
            .map(|path| evidence_for_relative(&spec.root, path))
            .collect::<Result<Vec<_>, _>>()?
    };
    files.sort_by(|left, right| left.path.cmp(&right.path));
    validate_evidence_set(&files, "material")?;
    let sha256 = hash_json(&files)?;
    let total_bytes = files.iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.bytes).ok_or_else(|| {
            ArtifactError::InvalidReceipt("material byte count overflowed u64".to_owned())
        })
    })?;
    Ok(InputMaterial {
        id: spec.id.clone(),
        kind: spec.kind.clone(),
        selection: material_selection(spec).to_owned(),
        file_count: files.len() as u64,
        total_bytes,
        sha256,
    })
}

fn material_selection(spec: &InputMaterialSpec) -> &'static str {
    if spec.included_paths.is_empty() {
        "tree-v1"
    } else {
        "exact-files-v1"
    }
}

fn source_set_sha256(
    sources: &[EvidenceFile],
    materials: &[InputMaterial],
) -> Result<String, ArtifactError> {
    hash_json(&(sources, materials))
}

fn normalize_context(context: &mut BuildContext) -> Result<(), ArtifactError> {
    validate_ownership(&context.ownership)?;
    if context.framework.version.trim().is_empty()
        || context.framework.source_revision.trim().is_empty()
    {
        return Err(ArtifactError::InvalidReceipt(
            "framework version and source revision are required".to_owned(),
        ));
    }
    if context.toolchain.is_empty() {
        return Err(ArtifactError::InvalidReceipt(
            "at least one toolchain entry is required".to_owned(),
        ));
    }
    context
        .toolchain
        .sort_by(|left, right| left.name.cmp(&right.name));
    context
        .configuration
        .sort_by(|left, right| left.path.cmp(&right.path));
    context
        .sources
        .sort_by(|left, right| left.path.cmp(&right.path));
    context
        .materials
        .sort_by(|left, right| left.id.cmp(&right.id));
    context.excluded_paths.sort();
    validate_evidence_set(&context.configuration, "configuration")?;
    validate_evidence_set(&context.sources, "source")?;
    let mut previous_material = None;
    for material in &context.materials {
        validate_material_token(&material.id, "material id")?;
        validate_material_token(&material.kind, "material kind")?;
        if previous_material.is_some_and(|previous: &str| previous >= material.id.as_str()) {
            return Err(ArtifactError::InvalidReceipt(
                "materials are duplicated or not strictly ordered".to_owned(),
            ));
        }
        if !matches!(material.selection.as_str(), "tree-v1" | "exact-files-v1")
            || material.file_count == 0
        {
            return Err(ArtifactError::InvalidReceipt(format!(
                "material {:?} has an invalid or empty selection",
                material.id
            )));
        }
        validate_sha256(&material.sha256, "material")?;
        previous_material = Some(material.id.as_str());
    }
    let input_files = context
        .configuration
        .len()
        .checked_add(context.sources.len())
        .map(|count| count as u64)
        .and_then(|total| {
            context.materials.iter().try_fold(total, |total, material| {
                total.checked_add(material.file_count)
            })
        })
        .ok_or_else(|| {
            ArtifactError::InvalidReceipt("input file count overflowed u64".to_owned())
        })?;
    if input_files > MAX_ARTIFACT_FILES as u64 {
        return Err(ArtifactError::InvalidReceipt(format!(
            "input set exceeds {MAX_ARTIFACT_FILES} files"
        )));
    }
    if context
        .toolchain
        .windows(2)
        .any(|pair| pair[0].name >= pair[1].name)
    {
        return Err(ArtifactError::InvalidReceipt(
            "toolchain entries are duplicated or not strictly ordered".to_owned(),
        ));
    }
    let toolchain_names = context
        .toolchain
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<BTreeSet<_>>();
    if !toolchain_names.contains("cargo")
        || !toolchain_names.contains("rustc")
        || toolchain_names
            .iter()
            .any(|name| !matches!(*name, "cargo" | "rustc" | "wasm-bindgen"))
    {
        return Err(ArtifactError::InvalidReceipt(
            "toolchain must contain cargo and rustc, plus optional wasm-bindgen".to_owned(),
        ));
    }
    let mut input_namespace = OutputNamespace::new();
    for file in context.configuration.iter().chain(&context.sources) {
        input_namespace.insert_str(&file.path, "build input")?;
    }
    let mut previous_excluded = None;
    for path in &context.excluded_paths {
        let portable = PortablePath::parse(path)?;
        if portable.as_str() != path
            || previous_excluded.is_some_and(|previous: &str| previous >= path)
        {
            return Err(ArtifactError::InvalidReceipt(
                "excluded paths are non-canonical, duplicated, or not strictly ordered".to_owned(),
            ));
        }
        previous_excluded = Some(path.as_str());
    }
    if context.source_set_sha256 != source_set_sha256(&context.sources, &context.materials)? {
        return Err(ArtifactError::InvalidReceipt(
            "source-set SHA-256 mismatch".to_owned(),
        ));
    }
    for entry in &context.toolchain {
        if entry.name.trim().is_empty() || entry.version.trim().is_empty() {
            return Err(ArtifactError::InvalidReceipt(
                "toolchain name and version are required".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_ownership(ownership: &Ownership) -> Result<(), ArtifactError> {
    validate_project_id(&ownership.project_id)?;
    if ownership.site_package.trim().is_empty() {
        return Err(ArtifactError::InvalidReceipt(
            "site package is required".to_owned(),
        ));
    }
    Ok(())
}

fn validate_project_id(value: &str) -> Result<(), ArtifactError> {
    if value.len() > 64
        || !value.starts_with(|character: char| character.is_ascii_lowercase())
        || !value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err(ArtifactError::InvalidReceipt(format!(
            "invalid project id {value:?}"
        )));
    }
    Ok(())
}

fn validate_evidence_set(files: &[EvidenceFile], label: &str) -> Result<(), ArtifactError> {
    let mut namespace = OutputNamespace::new();
    let mut previous = None;
    for file in files {
        if previous.is_some_and(|path: &str| path >= file.path.as_str()) {
            return Err(ArtifactError::InvalidReceipt(format!(
                "{label} files are duplicated or not strictly ordered"
            )));
        }
        let portable = namespace.insert_str(&file.path, label)?;
        if portable.as_str() != file.path {
            return Err(ArtifactError::InvalidReceipt(format!(
                "{label} path is not canonical NFC: {:?}",
                file.path
            )));
        }
        validate_sha256(&file.sha256, label)?;
        previous = Some(&file.path);
    }
    Ok(())
}

fn validate_outputs(files: &[OutputFile]) -> Result<(), ArtifactError> {
    if files.len() > MAX_ARTIFACT_FILES {
        return Err(ArtifactError::InvalidReceipt(format!(
            "output set exceeds {MAX_ARTIFACT_FILES} files"
        )));
    }
    let mut namespace = OutputNamespace::new();
    namespace.insert_str(BUILD_LEDGER_NAME, "framework ledger")?;
    let mut previous = None;
    let mut total_bytes = 0_u64;
    for file in files {
        if previous.is_some_and(|path: &str| path >= file.path.as_str()) {
            return Err(ArtifactError::InvalidReceipt(
                "outputs are duplicated or not strictly ordered".to_owned(),
            ));
        }
        let portable = namespace.insert_str(&file.path, &file.producer)?;
        if portable.as_str() != file.path {
            return Err(ArtifactError::InvalidReceipt(format!(
                "output path is not canonical NFC: {:?}",
                file.path
            )));
        }
        if file.kind.trim().is_empty() || file.producer.trim().is_empty() {
            return Err(ArtifactError::InvalidReceipt(
                "output kind and producer are required".to_owned(),
            ));
        }
        if file.bytes > MAX_OUTPUT_FILE_BYTES {
            return Err(ArtifactError::InvalidReceipt(format!(
                "output {:?} exceeds the per-file limit of {MAX_OUTPUT_FILE_BYTES} bytes",
                file.path
            )));
        }
        total_bytes = total_bytes
            .checked_add(file.bytes)
            .filter(|total| *total <= MAX_OUTPUT_TOTAL_BYTES)
            .ok_or_else(|| {
                ArtifactError::InvalidReceipt(format!(
                    "output set exceeds the aggregate limit of {MAX_OUTPUT_TOTAL_BYTES} bytes"
                ))
            })?;
        validate_sha256(&file.sha256, "output")?;
        previous = Some(&file.path);
    }
    Ok(())
}

fn validate_build_graph(graph: &BuildGraph) -> Result<(), ArtifactError> {
    if graph.graph_version != BUILD_GRAPH_VERSION {
        return Err(ArtifactError::InvalidReceipt(format!(
            "unsupported build graph version {:?}",
            graph.graph_version
        )));
    }
    validate_project_id(&graph.project_id)?;
    validate_sha256(&graph.source_set_sha256, "build graph source set")?;
    if graph.sources.len() > MAX_ARTIFACT_FILES
        || graph.routes.len() > MAX_ARTIFACT_FILES
        || graph.artifacts.len() > MAX_ARTIFACT_FILES
    {
        return Err(ArtifactError::InvalidReceipt(format!(
            "build graph exceeds {MAX_ARTIFACT_FILES} nodes per kind"
        )));
    }

    let mut source_paths = BTreeSet::new();
    let mut previous_source = None;
    for source in &graph.sources {
        let portable = PortablePath::parse(&source.path)?;
        if portable.as_str() != source.path
            || previous_source.is_some_and(|previous: &str| previous >= source.path.as_str())
        {
            return Err(ArtifactError::InvalidReceipt(
                "build graph sources are duplicated, non-canonical, or not strictly ordered"
                    .to_owned(),
            ));
        }
        validate_sha256(&source.sha256, "build graph source")?;
        source_paths.insert(source.path.as_str());
        previous_source = Some(source.path.as_str());
    }

    let mut route_names = BTreeSet::new();
    let mut previous_route = None;
    for route in &graph.routes {
        if route.route.len() > MAX_PORTABLE_PATH_BYTES
            || !route.route.starts_with('/')
            || route.route.contains(['\\', '\0'])
            || previous_route.is_some_and(|previous: &str| previous >= route.route.as_str())
        {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build graph route is invalid, duplicated, or not strictly ordered: {:?}",
                route.route
            )));
        }
        validate_graph_dependencies(&route.sources, &source_paths)?;
        validate_strict_portable_paths(&route.artifacts, "route artifacts")?;
        route_names.insert(route.route.as_str());
        previous_route = Some(route.route.as_str());
    }

    let mut artifact_paths = BTreeSet::new();
    let mut previous_artifact = None;
    for artifact in &graph.artifacts {
        let portable = PortablePath::parse(&artifact.path)?;
        if portable.as_str() != artifact.path
            || previous_artifact.is_some_and(|previous: &str| previous >= artifact.path.as_str())
            || artifact.kind.is_empty()
            || artifact.kind.len() > 64
            || artifact.producer.is_empty()
            || artifact.producer.len() > MAX_PORTABLE_PATH_BYTES
        {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build graph artifact is invalid, duplicated, or not strictly ordered: {:?}",
                artifact.path
            )));
        }
        if let Some(route) = artifact.route.as_deref() {
            if !route_names.contains(route) {
                return Err(ArtifactError::InvalidReceipt(format!(
                    "build graph artifact {:?} references unknown route {route:?}",
                    artifact.path
                )));
            }
        }
        validate_graph_dependencies(&artifact.sources, &source_paths)?;
        validate_sha256(&artifact.sha256, "build graph artifact")?;
        artifact_paths.insert(artifact.path.as_str());
        previous_artifact = Some(artifact.path.as_str());
    }

    for route in &graph.routes {
        for artifact in &route.artifacts {
            if !artifact_paths.contains(artifact.as_str()) {
                return Err(ArtifactError::InvalidReceipt(format!(
                    "build graph route {:?} references unknown artifact {artifact:?}",
                    route.route
                )));
            }
            let node = graph
                .artifacts
                .iter()
                .find(|node| node.path == *artifact)
                .expect("artifact path was checked against the graph set");
            if node.route.as_deref() != Some(route.route.as_str()) || node.sources != route.sources
            {
                return Err(ArtifactError::InvalidReceipt(format!(
                    "build graph route {:?} and artifact {artifact:?} disagree on their causal edge",
                    route.route
                )));
            }
        }
    }
    Ok(())
}

fn validate_graph_dependencies<'a>(
    dependencies: &'a SourceDependencies,
    sources: &BTreeSet<&'a str>,
) -> Result<(), ArtifactError> {
    let SourceDependencies::Explicit { paths } = dependencies else {
        return Ok(());
    };
    if paths.is_empty() {
        return Err(ArtifactError::InvalidReceipt(
            "explicit build graph dependencies cannot be empty".to_owned(),
        ));
    }
    validate_strict_portable_paths(paths, "source dependencies")?;
    for path in paths {
        if !sources.contains(path.as_str()) {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build graph dependency {path:?} is not a captured project source"
            )));
        }
    }
    Ok(())
}

fn validate_strict_portable_paths(paths: &[String], label: &str) -> Result<(), ArtifactError> {
    let mut previous = None;
    for path in paths {
        let portable = PortablePath::parse(path)?;
        if portable.as_str() != path
            || previous.is_some_and(|previous: &str| previous >= path.as_str())
        {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build graph {label} are duplicated, non-canonical, or not strictly ordered"
            )));
        }
        previous = Some(path.as_str());
    }
    Ok(())
}

pub fn validate_build_graph_against_report(
    graph: &BuildGraph,
    report: &BuildReport,
) -> Result<(), ArtifactError> {
    validate_build_graph(graph)?;
    validate_report(report)?;
    if graph.project_id != report.receipt.context.ownership.project_id
        || graph.source_set_sha256 != report.receipt.context.source_set_sha256
    {
        return Err(ArtifactError::InvalidReceipt(
            "build graph identity does not match the artifact receipt".to_owned(),
        ));
    }
    let expected_sources = report
        .receipt
        .context
        .sources
        .iter()
        .map(|source| GraphSource {
            path: source.path.clone(),
            sha256: source.sha256.clone(),
        })
        .collect::<Vec<_>>();
    if graph.sources != expected_sources {
        return Err(ArtifactError::InvalidReceipt(
            "build graph sources do not match the artifact receipt".to_owned(),
        ));
    }
    let expected_artifacts = report
        .receipt
        .outputs
        .files
        .iter()
        .filter(|file| file.path != BUILD_GRAPH_NAME)
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    if graph.artifacts.len() != expected_artifacts.len() {
        return Err(ArtifactError::InvalidReceipt(
            "build graph does not cover the exact payload artifact set".to_owned(),
        ));
    }
    for artifact in &graph.artifacts {
        let Some(output) = expected_artifacts.get(artifact.path.as_str()) else {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build graph contains undeclared artifact {:?}",
                artifact.path
            )));
        };
        if artifact.kind != output.kind
            || artifact.producer != output.producer
            || artifact.sha256 != output.sha256
        {
            return Err(ArtifactError::InvalidReceipt(format!(
                "build graph metadata does not match artifact {:?}",
                artifact.path
            )));
        }
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), ArtifactError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ArtifactError::InvalidReceipt(format!(
            "{label} has an invalid SHA-256"
        )));
    }
    Ok(())
}

fn hash_json<T: Serialize>(value: &T) -> Result<String, ArtifactError> {
    Ok(sha256_bytes(&serde_json::to_vec(value)?))
}

fn write_bounded_json(path: &Path, bytes: &[u8], limit: u64) -> Result<(), ArtifactError> {
    if bytes.len() as u64 > limit {
        return Err(ArtifactError::InvalidReceipt(format!(
            "JSON exceeds {limit} bytes"
        )));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| ArtifactError::Io {
        path: parent.to_owned(),
        source,
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| ArtifactError::InvalidPath(path.display().to_string()))?;
    let directory =
        Dir::open_ambient_dir(parent, ambient_authority()).map_err(|source| ArtifactError::Io {
            path: parent.to_owned(),
            source,
        })?;
    let mut options = CapOpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|source| ArtifactError::Io {
            path: path.to_owned(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| ArtifactError::Io {
        path: path.to_owned(),
        source,
    })?;
    ensure_single_hard_link(&metadata, path)?;
    file.write_all(bytes).map_err(|source| ArtifactError::Io {
        path: path.to_owned(),
        source,
    })?;
    file.sync_all().map_err(|source| ArtifactError::Io {
        path: path.to_owned(),
        source,
    })
}

fn read_regular_bounded(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, ArtifactError> {
    let mut file = open_regular_nofollow(path)?;
    let metadata = file.metadata().map_err(|source| ArtifactError::Io {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(ArtifactError::InvalidReceipt(format!(
            "{label} must be a regular file no larger than {limit} bytes"
        )));
    }
    ensure_single_hard_link(&metadata, path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| ArtifactError::Io {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(ArtifactError::InvalidReceipt(format!(
            "{label} grew beyond {limit} bytes while reading"
        )));
    }
    Ok(bytes)
}

fn validate_root_directory(root: &Path) -> Result<(), ArtifactError> {
    let metadata = fs::symlink_metadata(root).map_err(|source| ArtifactError::Io {
        path: root.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ArtifactError::InvalidPath(root.display().to_string()));
    }
    Ok(())
}

fn validate_absolute_canonical_root(root: &Path, label: &str) -> Result<(), ArtifactError> {
    validate_root_directory(root)?;
    if !root.is_absolute() {
        return Err(ArtifactError::InvalidPath(format!(
            "{label} must be absolute and canonical: {}",
            root.display()
        )));
    }
    let canonical = root.canonicalize().map_err(|source| ArtifactError::Io {
        path: root.to_owned(),
        source,
    })?;
    if canonical != root {
        return Err(ArtifactError::InvalidPath(format!(
            "{label} must be absolute and canonical: {}",
            root.display()
        )));
    }
    Ok(())
}

struct OutputLayout {
    files: BTreeSet<String>,
    directories: BTreeSet<String>,
    entries: usize,
    components: usize,
    path_bytes: usize,
}

impl OutputLayout {
    fn account_stored_path(&mut self, relative: &str) -> Result<(), ArtifactError> {
        self.components = self
            .components
            .checked_add(relative.split('/').count())
            .filter(|components| *components <= MAX_NAMESPACE_COMPONENTS)
            .ok_or_else(|| {
                ArtifactError::InvalidReceipt(format!(
                    "output layout exceeds {MAX_NAMESPACE_COMPONENTS} stored path components"
                ))
            })?;
        self.path_bytes = self
            .path_bytes
            .checked_add(relative.len())
            .filter(|bytes| *bytes <= MAX_NAMESPACE_PREFIX_BYTES)
            .ok_or_else(|| {
                ArtifactError::InvalidReceipt(format!(
                    "output layout exceeds {MAX_NAMESPACE_PREFIX_BYTES} stored path bytes"
                ))
            })?;
        Ok(())
    }
}

fn collect_output_layout(root: &Path) -> Result<OutputLayout, ArtifactError> {
    let mut layout = OutputLayout {
        files: BTreeSet::new(),
        directories: BTreeSet::new(),
        entries: 0,
        components: 0,
        path_bytes: 0,
    };
    let directory =
        Dir::open_ambient_dir(root, ambient_authority()).map_err(|source| ArtifactError::Io {
            path: root.to_owned(),
            source,
        })?;
    collect_output_directory(root, &directory, "", &mut layout)?;
    validate_output_layout_namespace(&layout)?;
    Ok(layout)
}

fn collect_output_directory(
    root: &Path,
    directory: &Dir,
    prefix: &str,
    layout: &mut OutputLayout,
) -> Result<(), ArtifactError> {
    let directory_path = display_relative_path(root, prefix);
    let entries = directory.entries().map_err(|source| ArtifactError::Io {
        path: directory_path.clone(),
        source,
    })?;
    for entry in entries {
        layout.entries = layout.entries.saturating_add(1);
        if layout.entries > MAX_ARTIFACT_FILES {
            return Err(ArtifactError::InvalidReceipt(format!(
                "output exceeds {MAX_ARTIFACT_FILES} entries"
            )));
        }
        let entry = entry.map_err(|source| ArtifactError::Io {
            path: directory_path.clone(),
            source,
        })?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| ArtifactError::InvalidPath(directory_path.display().to_string()))?;
        let raw_relative = join_portable(prefix, name);
        let relative = require_canonical_disk_path(&raw_relative)?;
        let path = display_relative_path(root, &relative);
        let file_type = entry.file_type().map_err(|source| ArtifactError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(ArtifactError::InvalidPath(format!(
                "linked output {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            layout.account_stored_path(&relative)?;
            if !layout.directories.insert(relative.clone()) {
                return Err(ArtifactError::NamespaceCollision {
                    path: relative.clone(),
                    existing: relative,
                    detail: "duplicate directory discovered on disk",
                });
            }
            let child = directory
                .open_dir_nofollow(name)
                .map_err(|source| ArtifactError::Io {
                    path: path.clone(),
                    source,
                })?;
            collect_output_directory(root, &child, &relative, layout)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(ArtifactError::InvalidPath(format!(
                "non-regular output {}",
                path.display()
            )));
        }
        if relative == BUILD_LEDGER_NAME {
            continue;
        }
        layout.account_stored_path(&relative)?;
        if !layout.files.insert(relative.clone()) {
            return Err(ArtifactError::NamespaceCollision {
                path: relative.clone(),
                existing: relative,
                detail: "duplicate output discovered on disk",
            });
        }
    }
    Ok(())
}

fn validate_output_layout_namespace(layout: &OutputLayout) -> Result<(), ArtifactError> {
    let mut namespace = OutputNamespace::new();
    for path in &layout.files {
        namespace.insert_str(path, "disk output")?;
    }
    let mut directory_keys = BTreeMap::<String, String>::new();
    for directory in &layout.directories {
        let portable = PortablePath::parse(directory)?;
        if let Some(existing) = directory_keys.insert(
            portable.collision_key().to_owned(),
            portable.as_str().to_owned(),
        ) {
            return Err(ArtifactError::NamespaceCollision {
                path: portable.as_str().to_owned(),
                existing,
                detail: "two directories resolve to the same portable path",
            });
        }
        if let Some((existing, _)) = namespace.files.get(portable.collision_key()) {
            return Err(ArtifactError::NamespaceCollision {
                path: portable.as_str().to_owned(),
                existing: existing.clone(),
                detail: "a path is both a file and a directory",
            });
        }
        if let Some(existing) = namespace.directories.get(portable.collision_key()) {
            if existing != portable.as_str() {
                return Err(ArtifactError::NamespaceCollision {
                    path: portable.as_str().to_owned(),
                    existing: existing.clone(),
                    detail: "an explicit directory aliases a directory implied by a file",
                });
            }
        }
    }
    Ok(())
}

fn expected_output_directories<'a>(
    paths: impl Iterator<Item = &'a str>,
) -> Result<BTreeSet<String>, ArtifactError> {
    let mut directories = BTreeSet::new();
    let mut components = 0_usize;
    let mut prefix_bytes = 0_usize;
    for path in paths {
        let portable = PortablePath::parse(path)?;
        let path_components = portable.as_str().split('/').collect::<Vec<_>>();
        components = components
            .checked_add(path_components.len())
            .filter(|components| *components <= MAX_NAMESPACE_COMPONENTS)
            .ok_or_else(|| {
                ArtifactError::InvalidReceipt(format!(
                    "expected output directories exceed {MAX_NAMESPACE_COMPONENTS} path components"
                ))
            })?;
        for length in 1..path_components.len() {
            let prefix = path_components[..length].join("/");
            if !directories.contains(&prefix) {
                let portable = PortablePath::parse(&prefix)?;
                prefix_bytes = prefix_bytes
                    .checked_add(portable.as_str().len())
                    .and_then(|bytes| bytes.checked_add(portable.collision_key().len()))
                    .filter(|bytes| *bytes <= MAX_NAMESPACE_PREFIX_BYTES)
                    .ok_or_else(|| {
                        ArtifactError::InvalidReceipt(format!(
                            "expected output directories exceed {MAX_NAMESPACE_PREFIX_BYTES} stored prefix bytes"
                        ))
                    })?;
                directories.insert(prefix);
            }
            if directories.len() > MAX_NAMESPACE_COMPONENTS {
                return Err(ArtifactError::InvalidReceipt(format!(
                    "expected output directories exceed {MAX_NAMESPACE_COMPONENTS} nodes"
                )));
            }
        }
    }
    Ok(directories)
}

fn evidence_for_relative(root: &Path, relative: &str) -> Result<EvidenceFile, ArtifactError> {
    let portable = PortablePath::parse(relative)?;
    reject_sensitive_input_path(portable.as_str())?;
    let file = open_regular_beneath(root, &portable)?;
    evidence_from_open_file(
        file,
        display_relative_path(root, portable.as_str()),
        portable.as_str().to_owned(),
    )
}

fn evidence_for_output_relative(
    root: &Path,
    expected: &OutputFile,
) -> Result<EvidenceFile, ArtifactError> {
    let portable = PortablePath::parse(&expected.path)?;
    let file = open_regular_beneath(root, &portable)?;
    let path = display_relative_path(root, portable.as_str());
    let metadata = file.metadata().map_err(|source| ArtifactError::Io {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ArtifactError::InvalidPath(format!(
            "output must be a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() != expected.bytes {
        return Err(ArtifactError::Modified(expected.path.clone()));
    }
    let (bytes, sha256) = hash_open_file_stable_bounded(file, &path, MAX_OUTPUT_FILE_BYTES)?;
    Ok(EvidenceFile {
        path: portable.as_str().to_owned(),
        bytes,
        sha256,
    })
}

fn evidence_from_open_file(
    file: CapFile,
    path: PathBuf,
    relative: String,
) -> Result<EvidenceFile, ArtifactError> {
    let metadata = file.metadata().map_err(|source| ArtifactError::Io {
        path: path.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(ArtifactError::InvalidPath(format!(
            "evidence input must be a regular file: {}",
            path.display()
        )));
    }
    let (bytes, sha256) = hash_open_file_stable(file, &path)?;
    Ok(EvidenceFile {
        path: relative,
        bytes,
        sha256,
    })
}

fn hash_open_file_stable(mut file: CapFile, path: &Path) -> Result<(u64, String), ArtifactError> {
    hash_open_file_stable_inner(&mut file, path, None)
}

fn hash_open_file_stable_bounded(
    mut file: CapFile,
    path: &Path,
    limit: u64,
) -> Result<(u64, String), ArtifactError> {
    hash_open_file_stable_inner(&mut file, path, Some(limit))
}

fn hash_open_file_stable_inner(
    file: &mut CapFile,
    path: &Path,
    limit: Option<u64>,
) -> Result<(u64, String), ArtifactError> {
    let before = file.metadata().map_err(|source| ArtifactError::Io {
        path: path.to_owned(),
        source,
    })?;
    ensure_single_hard_link(&before, path)?;
    if limit.is_some_and(|limit| before.len() > limit) {
        return Err(ArtifactError::InvalidReceipt(format!(
            "file exceeds the verification limit of {} bytes: {}",
            limit.unwrap_or_default(),
            path.display()
        )));
    }
    let before_modified = before.modified().ok();
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|source| ArtifactError::Io {
            path: path.to_owned(),
            source,
        })?;
        if read == 0 {
            break;
        }
        bytes = bytes.checked_add(read as u64).ok_or_else(|| {
            ArtifactError::InvalidReceipt("file byte count overflowed u64".to_owned())
        })?;
        if limit.is_some_and(|limit| bytes > limit) {
            return Err(ArtifactError::InvalidReceipt(format!(
                "file grew beyond the verification limit while hashing: {}",
                path.display()
            )));
        }
        digest.update(&buffer[..read]);
    }
    let after = file.metadata().map_err(|source| ArtifactError::Io {
        path: path.to_owned(),
        source,
    })?;
    ensure_single_hard_link(&after, path)?;
    if before.len() != after.len()
        || before_modified != after.modified().ok()
        || bytes != after.len()
    {
        return Err(ArtifactError::Unstable(path.display().to_string()));
    }
    Ok((bytes, format!("{:x}", digest.finalize())))
}

fn ensure_single_hard_link(
    metadata: &cap_std::fs::Metadata,
    path: &Path,
) -> Result<(), ArtifactError> {
    let links = metadata.nlink();
    if links != 1 {
        return Err(ArtifactError::InvalidPath(format!(
            "hard-linked file is not independently owned: {}",
            path.display()
        )));
    }
    Ok(())
}

fn open_regular_nofollow(path: &Path) -> Result<CapFile, ArtifactError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| ArtifactError::InvalidPath(path.display().to_string()))?;
    let directory =
        Dir::open_ambient_dir(parent, ambient_authority()).map_err(|source| ArtifactError::Io {
            path: parent.to_owned(),
            source,
        })?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    directory
        .open_with(name, &options)
        .map_err(|source| ArtifactError::Io {
            path: path.to_owned(),
            source,
        })
}

fn open_regular_beneath(root: &Path, relative: &PortablePath) -> Result<CapFile, ArtifactError> {
    let mut directory =
        Dir::open_ambient_dir(root, ambient_authority()).map_err(|source| ArtifactError::Io {
            path: root.to_owned(),
            source,
        })?;
    let components = relative.as_str().split('/').collect::<Vec<_>>();
    for (index, component) in components[..components.len().saturating_sub(1)]
        .iter()
        .enumerate()
    {
        let path = display_relative_path(root, &components[..=index].join("/"));
        directory = directory
            .open_dir_nofollow(component)
            .map_err(|source| ArtifactError::Io { path, source })?;
    }
    let name = components
        .last()
        .ok_or_else(|| ArtifactError::InvalidPath(relative.as_str().to_owned()))?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    directory
        .open_with(name, &options)
        .map_err(|source| ArtifactError::Io {
            path: display_relative_path(root, relative.as_str()),
            source,
        })
}

fn join_portable(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

fn display_relative_path(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|component| !component.is_empty())
        .fold(root.to_path_buf(), |path, component| path.join(component))
}

fn require_canonical_disk_path(raw: &str) -> Result<String, ArtifactError> {
    let portable = PortablePath::parse(raw)?;
    if portable.as_str() != raw {
        return Err(ArtifactError::InvalidPath(raw.to_owned()));
    }
    Ok(raw.to_owned())
}

fn collect_project_sources(
    root: &Path,
    directory: &Path,
    configuration: &BTreeSet<String>,
    excluded: &[String],
    sources: &mut Vec<EvidenceFile>,
) -> Result<(), ArtifactError> {
    if directory != root {
        return Err(ArtifactError::InvalidPath(format!(
            "source traversal must start at its declared root: {}",
            directory.display()
        )));
    }
    let directory =
        Dir::open_ambient_dir(root, ambient_authority()).map_err(|source| ArtifactError::Io {
            path: root.to_owned(),
            source,
        })?;
    let mut entries = 0_usize;
    collect_project_directory(
        root,
        &directory,
        "",
        configuration,
        excluded,
        sources,
        &mut entries,
    )
}

fn collect_project_directory(
    root: &Path,
    directory: &Dir,
    prefix: &str,
    configuration: &BTreeSet<String>,
    excluded: &[String],
    sources: &mut Vec<EvidenceFile>,
    entries: &mut usize,
) -> Result<(), ArtifactError> {
    let directory_path = display_relative_path(root, prefix);
    let children = directory.entries().map_err(|source| ArtifactError::Io {
        path: directory_path.clone(),
        source,
    })?;
    for child in children {
        *entries = entries.saturating_add(1);
        if *entries > MAX_ARTIFACT_FILES {
            return Err(ArtifactError::InvalidReceipt(format!(
                "input tree exceeds {MAX_ARTIFACT_FILES} entries"
            )));
        }
        let child = child.map_err(|source| ArtifactError::Io {
            path: directory_path.clone(),
            source,
        })?;
        let name = child.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| ArtifactError::InvalidPath(directory_path.display().to_string()))?;
        if prefix.is_empty() {
            reject_reserved_root_alias(name)?;
        }
        let raw_relative = join_portable(prefix, name);
        let relative = require_canonical_disk_path(&raw_relative)?;
        if ignored_project_path(&relative, excluded) {
            continue;
        }
        let path = display_relative_path(root, &relative);
        let file_type = child.file_type().map_err(|source| ArtifactError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_symlink() {
            return Err(ArtifactError::InvalidPath(format!(
                "linked project input {}",
                path.display()
            )));
        }
        if file_type.is_dir() {
            let nested = directory
                .open_dir_nofollow(name)
                .map_err(|source| ArtifactError::Io {
                    path: path.clone(),
                    source,
                })?;
            collect_project_directory(
                root,
                &nested,
                &relative,
                configuration,
                excluded,
                sources,
                entries,
            )?;
        } else if file_type.is_file() && !configuration.contains(&relative) {
            reject_sensitive_input_path(&relative)?;
            let mut options = CapOpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let file = directory
                .open_with(name, &options)
                .map_err(|source| ArtifactError::Io {
                    path: path.clone(),
                    source,
                })?;
            sources.push(evidence_from_open_file(file, path, relative)?);
        } else if !file_type.is_file() {
            return Err(ArtifactError::InvalidPath(format!(
                "non-regular project input {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn ignored_project_path(path: &str, excluded: &[String]) -> bool {
    let first = path.split('/').next().unwrap_or(path);
    matches!(first, ".git" | "target" | "node_modules")
        || excluded
            .iter()
            .any(|excluded| path == excluded || path.starts_with(&(excluded.to_owned() + "/")))
}

fn reject_reserved_root_alias(name: &str) -> Result<(), ArtifactError> {
    const RESERVED: [&str; 3] = [".git", "target", "node_modules"];
    if RESERVED.contains(&name) {
        return Ok(());
    }
    let key = portable_case_key(name);
    if RESERVED
        .iter()
        .any(|reserved| portable_case_key(reserved) == key)
    {
        return Err(ArtifactError::InvalidPath(format!(
            "root input {name:?} aliases reserved directory spelling"
        )));
    }
    Ok(())
}

fn reject_sensitive_input_path(path: &str) -> Result<(), ArtifactError> {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let folded = portable_case_key(file_name);
    let dotenv = folded == ".env"
        || folded.starts_with(".env.")
            && !matches!(
                folded.as_str(),
                ".env.example" | ".env.sample" | ".env.template"
            );
    let sensitive = dotenv
        || matches!(
            folded.as_str(),
            ".npmrc"
                | ".netrc"
                | ".pypirc"
                | ".git-credentials"
                | "credentials"
                | "credentials.json"
                | "credentials.toml"
                | "credentials.yaml"
                | "credentials.yml"
                | "id_rsa"
                | "id_ed25519"
                | "id_ecdsa"
                | "id_dsa"
        )
        || [".key", ".pem", ".p12", ".pfx"]
            .iter()
            .any(|extension| folded.ends_with(extension));
    if sensitive {
        return Err(ArtifactError::InvalidPath(format!(
            "refusing to publish path and hash evidence for sensitive-looking input {path:?}; keep runtime secrets outside build input roots"
        )));
    }
    Ok(())
}

fn toolchain_version(
    project_root: &Path,
    name: &str,
    arguments: &[&str],
) -> Result<ToolchainEvidence, ArtifactError> {
    let output = Command::new(name)
        .args(arguments)
        .current_dir(project_root)
        .output()
        .map_err(|source| ArtifactError::Io {
            path: PathBuf::from(name),
            source,
        })?;
    if !output.status.success() {
        return Err(ArtifactError::InvalidReceipt(format!(
            "{name} version command exited with {}",
            output.status
        )));
    }
    let version = String::from_utf8(output.stdout)
        .map_err(|_| ArtifactError::InvalidReceipt(format!("{name} returned non-UTF-8")))?
        .trim()
        .to_owned();
    if version.is_empty() {
        return Err(ArtifactError::InvalidReceipt(format!(
            "{name} returned an empty version"
        )));
    }
    Ok(ToolchainEvidence {
        name: name.to_owned(),
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pliego-artifact-{label}-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn context() -> BuildContext {
        let sources = vec![EvidenceFile {
            path: "src/main.rs".to_owned(),
            bytes: 2,
            sha256: sha256_bytes(b"rs"),
        }];
        BuildContext {
            ownership: Ownership {
                project_id: "artifact-test".to_owned(),
                site_package: "artifact-test".to_owned(),
            },
            framework: FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test-revision".to_owned(),
            },
            toolchain: vec![
                ToolchainEvidence {
                    name: "cargo".to_owned(),
                    version: "cargo test".to_owned(),
                },
                ToolchainEvidence {
                    name: "rustc".to_owned(),
                    version: "rustc test".to_owned(),
                },
            ],
            configuration: vec![EvidenceFile {
                path: "pliego.toml".to_owned(),
                bytes: 3,
                sha256: sha256_bytes(b"cfg"),
            }],
            source_set_sha256: source_set_sha256(&sources, &[]).unwrap(),
            sources,
            materials: Vec::new(),
            excluded_paths: vec!["target/site".to_owned()],
        }
    }

    fn write_fixture(root: &Path) -> BuildReport {
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("index.html"), b"home").unwrap();
        fs::write(root.join("assets/site.css"), b"body{}").unwrap();
        let receipt = ArtifactReceipt::from_context_and_files(
            context(),
            vec![
                OutputFile::new("index.html", "route", "/", b"home"),
                OutputFile::new("assets/site.css", "asset", "assets/site.css", b"body{}"),
            ],
        )
        .unwrap();
        let report = BuildReport::new(receipt).unwrap();
        write_build_report(&root.join(BUILD_LEDGER_NAME), &report).unwrap();
        report
    }

    fn causal_graph_fixture() -> (BuildGraph, BuildReport) {
        let context = context();
        let graph = BuildGraph {
            graph_version: BUILD_GRAPH_VERSION.to_owned(),
            project_id: context.ownership.project_id.clone(),
            source_set_sha256: context.source_set_sha256.clone(),
            sources: vec![GraphSource {
                path: "src/main.rs".to_owned(),
                sha256: sha256_bytes(b"rs"),
            }],
            routes: vec![GraphRoute {
                route: "/".to_owned(),
                sources: SourceDependencies::Explicit {
                    paths: vec!["src/main.rs".to_owned()],
                },
                artifacts: vec!["index.html".to_owned()],
            }],
            artifacts: vec![GraphArtifact {
                path: "index.html".to_owned(),
                kind: "route".to_owned(),
                producer: "/".to_owned(),
                route: Some("/".to_owned()),
                sources: SourceDependencies::Explicit {
                    paths: vec!["src/main.rs".to_owned()],
                },
                sha256: sha256_bytes(b"home"),
            }],
        };
        let graph_bytes = encode_build_graph(&graph).unwrap();
        let receipt = ArtifactReceipt::from_context_and_files(
            context,
            vec![
                OutputFile::new("index.html", "route", "/", b"home"),
                OutputFile::new(
                    BUILD_GRAPH_NAME,
                    "framework",
                    "causal-build-graph",
                    &graph_bytes,
                ),
            ],
        )
        .unwrap();
        (graph, BuildReport::new(receipt).unwrap())
    }

    #[test]
    fn build_graph_is_canonical_bounded_and_receipt_bound() {
        let (graph, report) = causal_graph_fixture();
        let encoded = encode_build_graph(&graph).unwrap();
        assert_eq!(decode_build_graph(&encoded).unwrap(), graph);
        validate_build_graph_against_report(&graph, &report).unwrap();

        let mut unknown_source = graph.clone();
        unknown_source.routes[0].sources = SourceDependencies::Explicit {
            paths: vec!["src/missing.rs".to_owned()],
        };
        assert!(encode_build_graph(&unknown_source).is_err());

        let mut wrong_output = graph.clone();
        wrong_output.artifacts[0].sha256 = sha256_bytes(b"tampered");
        assert!(validate_build_graph_against_report(&wrong_output, &report).is_err());

        assert!(decode_build_graph(&encoded[..encoded.len() / 2]).is_err());
    }

    #[test]
    fn portable_paths_reject_aliases_and_reserved_names() {
        for invalid in [
            "",
            "/root",
            "tail/",
            "../escape",
            "a//b",
            "a\\b",
            "CON",
            "aux.txt",
            "LPT1.css",
            "trailing.",
            "trailing ",
            "bad:name",
        ] {
            assert!(
                PortablePath::parse(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        let composed = PortablePath::parse("caf\u{e9}.html").unwrap();
        let decomposed = PortablePath::parse("cafe\u{301}.html").unwrap();
        assert_eq!(composed.collision_key(), decomposed.collision_key());
        assert_eq!(
            PortablePath::parse("Stra\u{df}e.html")
                .unwrap()
                .collision_key(),
            PortablePath::parse("STRASSE.html").unwrap().collision_key()
        );
        let expands_during_nfc = (0..128)
            .map(|_| "\u{344}".repeat(15))
            .collect::<Vec<_>>()
            .join("/");
        assert!(expands_during_nfc.len() <= MAX_PORTABLE_PATH_BYTES);
        assert!(PortablePath::parse(&expands_during_nfc).is_err());
    }

    #[test]
    fn namespace_rejects_exact_case_unicode_and_file_directory_collisions() {
        for (first, second) in [
            ("guide/index.html", "guide/index.html"),
            ("Guide/index.html", "guide/index.html"),
            ("caf\u{e9}.html", "cafe\u{301}.html"),
            ("Stra\u{df}e.html", "STRASSE.html"),
            ("guide", "guide/index.html"),
            ("Assets/a.css", "assets/b.css"),
        ] {
            let mut namespace = OutputNamespace::new();
            namespace.insert_str(first, "first").unwrap();
            assert!(
                namespace.insert_str(second, "second").is_err(),
                "accepted collision {first:?} versus {second:?}"
            );
        }
    }

    #[test]
    fn receipt_is_order_independent_and_strictly_self_verifying() {
        let left = ArtifactReceipt::from_context_and_files(
            context(),
            vec![
                OutputFile::new("index.html", "route", "/", b"home"),
                OutputFile::new("assets/site.css", "asset", "css", b"body{}"),
            ],
        )
        .unwrap();
        let right = ArtifactReceipt::from_context_and_files(
            context(),
            vec![
                OutputFile::new("assets/site.css", "asset", "css", b"body{}"),
                OutputFile::new("index.html", "route", "/", b"home"),
            ],
        )
        .unwrap();
        assert_eq!(
            BuildReport::new(left).unwrap(),
            BuildReport::new(right).unwrap()
        );
    }

    #[test]
    fn encoded_build_report_is_deterministic_and_matches_the_persisted_ledger() {
        let root = temp_root("encoded-report");
        let report = write_fixture(&root);
        let first = encode_build_report(&report).unwrap();
        let second = encode_build_report(&report).unwrap();
        assert_eq!(first, second);
        assert_eq!(first, fs::read(root.join(BUILD_LEDGER_NAME)).unwrap());
        assert_eq!(
            serde_json::from_slice::<BuildReport>(&first).unwrap(),
            report
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn environment_context_reads_the_invocation_sidecar() {
        const CHILD_MARKER: &str = "PLIEGO_TEST_INVOCATION_CONTEXT_CHILD";
        if std::env::var_os(CHILD_MARKER).is_some() {
            assert_eq!(context_from_environment().unwrap(), Some(context()));
            return;
        }

        let root = temp_root("environment-invocation");
        fs::create_dir_all(&root).unwrap();
        let invocation = BuildInvocation {
            context: context(),
            project_root: root.canonicalize().unwrap(),
            output_path: "target/site".to_owned(),
            material_specs: Vec::new(),
        };
        let sidecar = root.join("invocation.json");
        write_build_invocation(&sidecar, &invocation).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::environment_context_reads_the_invocation_sidecar",
                "--nocapture",
            ])
            .env(BUILD_CONTEXT_ENV, &sidecar)
            .env(CHILD_MARKER, "1")
            .status()
            .unwrap();
        assert!(status.success());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verification_detects_missing_extra_and_modified_outputs() {
        for case in ["missing", "extra", "empty-directory", "modified"] {
            let root = temp_root(case);
            write_fixture(&root);
            match case {
                "missing" => fs::remove_file(root.join("index.html")).unwrap(),
                "extra" => fs::write(root.join("extra.txt"), b"extra").unwrap(),
                "empty-directory" => fs::create_dir(root.join("extra-directory")).unwrap(),
                "modified" => fs::write(root.join("index.html"), b"evil").unwrap(),
                _ => unreachable!(),
            }
            assert!(verify_build_report(&root).is_err());
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn undeclared_sparse_file_is_rejected_before_its_bytes_are_read() {
        let root = temp_root("sparse-extra");
        write_fixture(&root);
        let file = fs::File::create(root.join("sparse.bin")).unwrap();
        file.set_len(16 * 1024 * 1024).unwrap();
        assert!(matches!(
            verify_build_report(&root),
            Err(ArtifactError::Extra(path)) if path == "sparse.bin"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn namespace_validation_scales_without_pairwise_scans() {
        let mut namespace = OutputNamespace::new();
        for index in 0..25_000 {
            namespace
                .insert_str(&format!("assets/{index:05}.bin"), "stress")
                .unwrap();
        }
    }

    #[test]
    fn namespace_rejects_component_and_prefix_byte_amplification() {
        let shared = (0..127)
            .map(|index| format!("d{index:03}"))
            .collect::<Vec<_>>()
            .join("/");
        let mut component_limited = OutputNamespace::new();
        let mut component_error = None;
        for index in 0..3_000 {
            match component_limited.insert_str(&format!("{shared}/f{index:04}"), "stress") {
                Ok(_) => {}
                Err(error) => {
                    component_error = Some(error);
                    break;
                }
            }
        }
        assert!(
            component_error
                .unwrap()
                .to_string()
                .contains("path components")
        );

        let tail = (0..127)
            .map(|index| format!("component-{index:03}-xxxxx"))
            .collect::<Vec<_>>()
            .join("/");
        let mut prefix_limited = OutputNamespace::new();
        let mut prefix_error = None;
        for index in 0..200 {
            match prefix_limited.insert_str(&format!("root-{index:03}/{tail}"), "stress") {
                Ok(_) => {}
                Err(error) => {
                    prefix_error = Some(error);
                    break;
                }
            }
        }
        assert!(
            prefix_error
                .unwrap()
                .to_string()
                .contains("stored prefix bytes")
        );
    }

    #[test]
    fn output_layout_rejects_stored_path_amplification_before_insertion() {
        let long_tail = (0..15)
            .map(|_| "x".repeat(250))
            .collect::<Vec<_>>()
            .join("/");
        let mut layout = OutputLayout {
            files: BTreeSet::new(),
            directories: BTreeSet::new(),
            entries: 0,
            components: 0,
            path_bytes: 0,
        };
        let mut error = None;
        for index in 0..10_000 {
            let path = format!("f{index:05}/{long_tail}");
            match layout.account_stored_path(&path) {
                Ok(()) => {}
                Err(found) => {
                    error = Some(found);
                    break;
                }
            }
        }
        assert!(error.unwrap().to_string().contains("stored path bytes"));
    }

    #[test]
    fn oversized_and_size_mismatched_sparse_outputs_fail_before_hashing() {
        let oversized = temp_root("oversized-declared");
        let mut report = write_fixture(&oversized);
        let declared = MAX_OUTPUT_FILE_BYTES + 1;
        fs::OpenOptions::new()
            .write(true)
            .open(oversized.join("index.html"))
            .unwrap()
            .set_len(declared)
            .unwrap();
        let output = report
            .receipt
            .outputs
            .files
            .iter_mut()
            .find(|output| output.path == "index.html")
            .unwrap();
        output.bytes = declared;
        output.sha256 = sha256_bytes(b"self-signed-but-oversized");
        report.receipt.outputs.total_bytes = report
            .receipt
            .outputs
            .files
            .iter()
            .map(|output| output.bytes)
            .sum();
        report.receipt.outputs.sha256 = hash_json(&report.receipt.outputs.files).unwrap();
        report.receipt_sha256 = hash_json(&report.receipt).unwrap();
        fs::write(
            oversized.join(BUILD_LEDGER_NAME),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            verify_build_report(&oversized),
            Err(ArtifactError::InvalidReceipt(message)) if message.contains("per-file limit")
        ));
        let _ = fs::remove_dir_all(oversized);

        let mismatched = temp_root("oversized-actual");
        write_fixture(&mismatched);
        fs::OpenOptions::new()
            .write(true)
            .open(mismatched.join("index.html"))
            .unwrap()
            .set_len(MAX_OUTPUT_FILE_BYTES + 1)
            .unwrap();
        assert!(matches!(
            verify_build_report(&mismatched),
            Err(ArtifactError::Modified(path)) if path == "index.html"
        ));
        let _ = fs::remove_dir_all(mismatched);
    }

    #[test]
    fn output_and_ledger_hardlinks_are_rejected() {
        let outside = temp_root("hardlink-outside");
        fs::create_dir_all(&outside).unwrap();

        let output_root = temp_root("hardlinked-output");
        write_fixture(&output_root);
        fs::hard_link(
            output_root.join("index.html"),
            outside.join("external-output-alias"),
        )
        .unwrap();
        assert!(matches!(
            verify_build_report(&output_root),
            Err(ArtifactError::InvalidPath(message)) if message.contains("hard-linked file")
        ));
        let _ = fs::remove_dir_all(output_root);

        let ledger_root = temp_root("hardlinked-ledger");
        write_fixture(&ledger_root);
        fs::hard_link(
            ledger_root.join(BUILD_LEDGER_NAME),
            outside.join("external-ledger-alias"),
        )
        .unwrap();
        assert!(matches!(
            verify_build_report(&ledger_root),
            Err(ArtifactError::InvalidPath(message)) if message.contains("hard-linked")
        ));
        let _ = fs::remove_dir_all(ledger_root);

        let internal_root = temp_root("internal-hardlinks");
        fs::create_dir_all(&internal_root).unwrap();
        fs::write(internal_root.join("first.bin"), b"same").unwrap();
        fs::hard_link(
            internal_root.join("first.bin"),
            internal_root.join("second.bin"),
        )
        .unwrap();
        let receipt = ArtifactReceipt::from_context_and_files(
            context(),
            vec![
                OutputFile::new("first.bin", "asset", "first", b"same"),
                OutputFile::new("second.bin", "asset", "second", b"same"),
            ],
        )
        .unwrap();
        write_build_report(
            &internal_root.join(BUILD_LEDGER_NAME),
            &BuildReport::new(receipt).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            verify_build_report(&internal_root),
            Err(ArtifactError::InvalidPath(message)) if message.contains("hard-linked file")
        ));
        let _ = fs::remove_dir_all(internal_root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn hostile_byte_totals_fail_without_panicking() {
        let root = temp_root("overflow");
        let mut report = write_fixture(&root);
        report.receipt.outputs.files[0].bytes = u64::MAX;
        report.receipt.outputs.files[1].bytes = 1;
        report.receipt.outputs.sha256 = hash_json(&report.receipt.outputs.files).unwrap();
        report.receipt_sha256 = hash_json(&report.receipt).unwrap();
        let outcome =
            std::panic::catch_unwind(|| write_build_report(&root.join(BUILD_LEDGER_NAME), &report));
        assert!(outcome.is_ok());
        assert!(outcome.unwrap().is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verification_rejects_self_inconsistent_receipts_and_unknown_fields() {
        let root = temp_root("forged");
        let mut report = write_fixture(&root);
        report.receipt.outputs.files[0].sha256 = sha256_bytes(b"forged");
        fs::write(
            root.join(BUILD_LEDGER_NAME),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        assert!(read_build_report(&root).is_err());

        let mut value = serde_json::to_value(&report).unwrap();
        value["unknown"] = serde_json::json!(true);
        fs::write(
            root.join(BUILD_LEDGER_NAME),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(read_build_report(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn report_constructor_rejects_invalid_lineage_immediately() {
        let mut receipt = ArtifactReceipt::from_context_and_files(
            context(),
            vec![OutputFile::new("index.html", "route", "/", b"home")],
        )
        .unwrap();
        receipt.previous_ownership = Some(PreviousOwnership {
            project_id: "artifact-test".to_owned(),
            site_package: "artifact-test".to_owned(),
            receipt_sha256: "not-a-sha256".to_owned(),
        });
        assert!(BuildReport::new(receipt).is_err());
    }

    #[test]
    fn verification_accepts_only_regular_ledger_and_output_files() {
        let root = temp_root("links");
        write_fixture(&root);
        let outside = root.with_extension("outside");
        fs::write(&outside, b"outside").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, root.join("linked.txt"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("linked.txt"));
        if linked.is_ok() {
            assert!(verify_build_report(&root).is_err());
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn capture_and_reverification_detect_source_drift() {
        let root = temp_root("context");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("pliego.toml"), b"project = true").unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(root.join(".env"), b"TOKEN=private").unwrap();
        fs::write(root.join("deploy.pem"), b"private key").unwrap();
        fs::write(root.join("credentials.toml"), b"token = 'private'").unwrap();
        let capture = || {
            capture_build_context(
                &root,
                Ownership {
                    project_id: "context-test".to_owned(),
                    site_package: "context-test".to_owned(),
                },
                FrameworkEvidence {
                    version: "0.0.1".to_owned(),
                    source_revision: "test".to_owned(),
                },
                &["pliego.toml".to_owned()],
                &["target/site".to_owned()],
            )
        };
        assert!(capture().is_err());
        fs::remove_file(root.join(".env")).unwrap();
        fs::remove_file(root.join("deploy.pem")).unwrap();
        fs::remove_file(root.join("credentials.toml")).unwrap();
        let captured = capture().unwrap();
        verify_build_context(&root, &captured).unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() { panic!() }").unwrap();
        assert!(verify_build_context(&root, &captured).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_sources_and_configuration_reject_innocent_hardlink_aliases() {
        let outside = temp_root("linked-project-secrets");
        fs::create_dir_all(&outside).unwrap();

        for kind in ["source", "configuration"] {
            let root = temp_root(&format!("linked-project-{kind}"));
            fs::create_dir_all(root.join("src")).unwrap();
            if kind == "source" {
                fs::write(root.join("pliego.toml"), b"project = true").unwrap();
                let sensitive = outside.join("private-source.pem");
                fs::write(&sensitive, b"private source").unwrap();
                fs::hard_link(&sensitive, root.join("src/main.rs")).unwrap();
            } else {
                fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
                let sensitive = outside.join("private-configuration.key");
                fs::write(&sensitive, b"private configuration").unwrap();
                fs::hard_link(&sensitive, root.join("pliego.toml")).unwrap();
            }

            let result = capture_build_context(
                &root,
                Ownership {
                    project_id: format!("linked-{kind}"),
                    site_package: format!("linked-{kind}"),
                },
                FrameworkEvidence {
                    version: "0.0.1".to_owned(),
                    source_revision: "test".to_owned(),
                },
                &["pliego.toml".to_owned()],
                &["target/site".to_owned()],
            );
            assert!(matches!(
                result,
                Err(ArtifactError::InvalidPath(message))
                    if message.contains("hard-linked file")
            ));
            let _ = fs::remove_dir_all(root);
        }
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn publication_like_source_names_are_included_and_reverified() {
        let root = temp_root("publication-like-source");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("pliego.toml"), b"project = true").unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(
            root.join("src/build-context-production.json"),
            b"{\"mode\":\"production\"}",
        )
        .unwrap();
        fs::create_dir(root.join("src/Target")).unwrap();
        fs::write(root.join("src/Target/source.rs"), b"pub fn legitimate() {}").unwrap();
        let captured = capture_build_context(
            &root,
            Ownership {
                project_id: "publication-like-source".to_owned(),
                site_package: "publication-like-source".to_owned(),
            },
            FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &["target/site".to_owned()],
        )
        .unwrap();
        assert!(
            captured
                .sources
                .iter()
                .any(|source| source.path == "src/build-context-production.json")
        );
        assert!(
            captured
                .sources
                .iter()
                .any(|source| source.path == "src/Target/source.rs")
        );
        fs::write(
            root.join("src/build-context-production.json"),
            b"{\"mode\":\"changed\"}",
        )
        .unwrap();
        assert!(verify_build_context(&root, &captured).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reserved_root_directories_require_exact_portable_spelling() {
        for alias in [
            ".GIT",
            "Target",
            "Node_Modules",
            "\u{ff54}\u{ff41}\u{ff52}\u{ff47}\u{ff45}\u{ff54}",
        ] {
            let root = temp_root("reserved-root-alias");
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("pliego.toml"), b"project = true").unwrap();
            fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
            fs::create_dir(root.join(alias)).unwrap();
            fs::write(root.join(alias).join("ignored.rs"), b"alias").unwrap();
            let result = capture_build_context(
                &root,
                Ownership {
                    project_id: "reserved-root-alias".to_owned(),
                    site_package: "reserved-root-alias".to_owned(),
                },
                FrameworkEvidence {
                    version: "0.0.1".to_owned(),
                    source_revision: "test".to_owned(),
                },
                &["pliego.toml".to_owned()],
                &["target/site".to_owned()],
            );
            assert!(matches!(
                result,
                Err(ArtifactError::InvalidPath(message))
                    if message.contains("aliases reserved directory spelling")
            ));
            let _ = fs::remove_dir_all(root);
        }

        let root = temp_root("reserved-root-exact");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("pliego.toml"), b"project = true").unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        for reserved in [".git", "target", "node_modules"] {
            fs::create_dir(root.join(reserved)).unwrap();
            fs::write(root.join(reserved).join("ignored.rs"), b"ignored").unwrap();
        }
        let captured = capture_build_context(
            &root,
            Ownership {
                project_id: "reserved-root-exact".to_owned(),
                site_package: "reserved-root-exact".to_owned(),
            },
            FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &["target/site".to_owned()],
        )
        .unwrap();
        assert!(
            captured
                .sources
                .iter()
                .all(|source| !source.path.ends_with("ignored.rs"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn source_capture_rejects_noncanonical_unicode_disk_spelling() {
        let root = temp_root("nfd-source");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("pliego.toml"), b"project = true").unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(root.join("src").join("cafe\u{301}.rs"), b"mod nfd {}").unwrap();
        let result = capture_build_context(
            &root,
            Ownership {
                project_id: "nfd-source".to_owned(),
                site_package: "nfd-source".to_owned(),
            },
            FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &["target/site".to_owned()],
        );
        assert!(matches!(result, Err(ArtifactError::InvalidPath(_))));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_material_selection_rejects_portable_aliases_and_file_directory_overlap() {
        let root = temp_root("material-selection");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), b"pub fn material() {}").unwrap();
        assert!(
            InputMaterialSpec::files(
                "noncanonical-single",
                "test",
                &root,
                vec!["cafe\u{301}.rs".to_owned()],
            )
            .is_err()
        );
        assert!(
            InputMaterialSpec::files(
                "portable-alias",
                "test",
                &root,
                vec!["caf\u{e9}.rs".to_owned(), "cafe\u{301}.rs".to_owned()],
            )
            .is_err()
        );
        assert!(
            InputMaterialSpec::files(
                "file-directory",
                "test",
                &root,
                vec!["src".to_owned(), "src/lib.rs".to_owned()],
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_material_selection_rejects_linked_ancestor() {
        let root = temp_root("material-linked-parent");
        let outside = temp_root("material-linked-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("input.rs"), b"pub fn outside() {}").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, root.join("linked"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("linked"));
        if linked.is_ok() {
            let spec = InputMaterialSpec::files(
                "linked-ancestor",
                "test",
                &root,
                vec!["linked/input.rs".to_owned()],
            )
            .unwrap();
            assert!(capture_input_material(&spec).is_err());
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn exact_and_tree_materials_reject_innocent_hardlink_aliases() {
        let outside = temp_root("linked-material-secret");
        let exact_root = temp_root("linked-material-exact");
        let tree_root = temp_root("linked-material-tree");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(exact_root.join("src")).unwrap();
        fs::create_dir_all(tree_root.join("src")).unwrap();
        let sensitive = outside.join("credentials.pem");
        fs::write(&sensitive, b"private material").unwrap();
        fs::hard_link(&sensitive, exact_root.join("src/lib.rs")).unwrap();
        fs::hard_link(&sensitive, tree_root.join("src/lib.rs")).unwrap();

        let exact = InputMaterialSpec::files(
            "hardlink-exact",
            "test",
            &exact_root,
            vec!["src/lib.rs".to_owned()],
        )
        .unwrap();
        assert!(matches!(
            capture_input_material(&exact),
            Err(ArtifactError::InvalidPath(message)) if message.contains("hard-linked file")
        ));

        let tree =
            InputMaterialSpec::tree("hardlink-tree", "test", &tree_root, Vec::new()).unwrap();
        assert!(matches!(
            capture_input_material(&tree),
            Err(ArtifactError::InvalidPath(message)) if message.contains("hard-linked file")
        ));

        let _ = fs::remove_dir_all(exact_root);
        let _ = fs::remove_dir_all(tree_root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn external_materials_bind_transitive_inputs_without_publishing_roots_or_leaf_names() {
        let root = temp_root("material-project");
        let material_root = temp_root("material-dependency");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(material_root.join("src")).unwrap();
        fs::write(root.join("pliego.toml"), b"project = true").unwrap();
        fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(material_root.join("Cargo.toml"), b"[package]").unwrap();
        fs::write(material_root.join("src/lib.rs"), b"pub fn dependency() {}").unwrap();
        let spec = InputMaterialSpec::tree(
            "cargo-path/dependency@0.1.0",
            "cargo-path-package",
            &material_root,
            Vec::new(),
        )
        .unwrap();
        let context = capture_build_context_with_materials(
            &root,
            Ownership {
                project_id: "material-test".to_owned(),
                site_package: "material-test".to_owned(),
            },
            FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &["target/site".to_owned()],
            std::slice::from_ref(&spec),
        )
        .unwrap();
        assert_eq!(context.materials[0].file_count, 2);
        let receipt_json = serde_json::to_string(&context).unwrap();
        assert!(!receipt_json.contains(&material_root.display().to_string()));
        assert!(!receipt_json.contains("src/lib.rs"));
        verify_build_context_with_materials(&root, std::slice::from_ref(&spec), &context).unwrap();

        let invocation = BuildInvocation {
            context: context.clone(),
            project_root: root.canonicalize().unwrap(),
            output_path: "target/site".to_owned(),
            material_specs: vec![spec.clone()],
        };
        let invocation_path = root.join("target/.pliego/invocation.json");
        write_build_invocation(&invocation_path, &invocation).unwrap();
        assert_eq!(read_build_invocation(&invocation_path).unwrap(), invocation);
        let invocation_json = serde_json::to_string(&invocation).unwrap();
        assert!(invocation_json.contains(r#""outputPath":"target/site""#));
        assert!(!invocation_json.contains(&root.join("target/site").display().to_string()));

        let mut invalid_output = invocation.clone();
        invalid_output.output_path = "target/../other".to_owned();
        assert!(
            write_build_invocation(
                &root.join("target/.pliego/invalid-output.json"),
                &invalid_output,
            )
            .is_err()
        );

        let mut unbound_output = invocation.clone();
        unbound_output.output_path = "target/other".to_owned();
        assert!(
            write_build_invocation(
                &root.join("target/.pliego/unbound-output.json"),
                &unbound_output,
            )
            .is_err()
        );

        let mut unknown = serde_json::to_value(&invocation).unwrap();
        unknown["unknownField"] = serde_json::Value::Bool(true);
        fs::write(&invocation_path, serde_json::to_vec(&unknown).unwrap()).unwrap();
        assert!(read_build_invocation(&invocation_path).is_err());

        let mut noncanonical = serde_json::to_value(&invocation).unwrap();
        noncanonical["context"]["toolchain"]
            .as_array_mut()
            .unwrap()
            .reverse();
        fs::write(&invocation_path, serde_json::to_vec(&noncanonical).unwrap()).unwrap();
        assert!(read_build_invocation(&invocation_path).is_err());

        fs::write(material_root.join("src/lib.rs"), b"pub fn changed() {}").unwrap();
        assert!(
            verify_build_context_with_materials(&root, std::slice::from_ref(&spec), &context)
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(material_root);
    }
}
