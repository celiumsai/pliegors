//! Deterministic inspection for `pliego.asset-manifest.json`.
//!
//! The crate deliberately separates manifest validity, file integrity, and
//! budget status. Phase 1 can record current budget debt without weakening the
//! structural or provenance contract.

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub const MANIFEST_VERSION: &str = "1.0.0";
pub const REPORT_VERSION: &str = "1.0.0";
pub const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TARGET_SET_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
pub enum InspectError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    InvalidInput {
        path: PathBuf,
        message: String,
    },
    InvalidTargetSet(String),
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            Self::Json { path, source } => {
                write!(f, "invalid JSON in {}: {source}", path.display())
            }
            Self::InvalidInput { path, message } => {
                write!(f, "invalid input {}: {message}", path.display())
            }
            Self::InvalidTargetSet(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for InspectError {}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetManifest {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    pub manifest_version: String,
    pub work: Work,
    pub coverage: Coverage,
    pub assets: Vec<Asset>,
    pub budget_scopes: Vec<BudgetScope>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub kind: WorkKind,
    pub id: String,
    pub slug: String,
    pub title: String,
    pub owner: String,
    #[serde(default)]
    pub signatura: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkKind {
    House,
    Pliego,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Coverage {
    pub asset_root: String,
    pub tracked_extensions: Vec<String>,
    pub excluded: Vec<Exclusion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub label: String,
    pub owner: Owner,
    pub source: Source,
    pub rights: Rights,
    pub visual_importance: VisualImportance,
    #[serde(default)]
    pub tags: Vec<String>,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub entity: String,
    pub role: OwnerRole,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerRole {
    House,
    Author,
    Client,
    ThirdParty,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub kind: SourceKind,
    pub locator: String,
    #[serde(default)]
    pub generator: Option<String>,
    #[serde(default)]
    pub created_for: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    Generated,
    Commissioned,
    Owned,
    Licensed,
    System,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rights {
    pub status: RightsStatus,
    pub holder: String,
    pub transfer: TransferStatus,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RightsStatus {
    Owned,
    Licensed,
    PublicDomain,
    System,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransferStatus {
    Included,
    Excluded,
    Conditional,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualImportance {
    Critical,
    Supporting,
    Decorative,
    Utility,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub path: String,
    pub media_type: String,
    pub format: String,
    pub sha256: String,
    pub bytes: u64,
    pub tiers: Vec<Tier>,
    pub delivery: Delivery,
    pub preload: bool,
    #[serde(default)]
    pub dimensions: Option<Dimensions>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub estimated_vram_bytes: Option<u64>,
    #[serde(default)]
    pub geometry: Option<Geometry>,
    #[serde(default)]
    pub fallback_for: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Geometry {
    pub triangles: u64,
    pub draw_calls: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Universal,
    Lite,
    Balanced,
    Signature,
}

impl Tier {
    pub const ALL: [Self; 4] = [Self::Universal, Self::Lite, Self::Balanced, Self::Signature];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Universal => "universal",
            Self::Lite => "lite",
            Self::Balanced => "balanced",
            Self::Signature => "signature",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Delivery {
    Initial,
    Deferred,
    OnDemand,
    Download,
}

impl Delivery {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Deferred => "deferred",
            Self::OnDemand => "on-demand",
            Self::Download => "download",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetScope {
    pub id: String,
    pub label: String,
    pub route: String,
    pub tier: Tier,
    pub phase: BudgetPhase,
    pub variants: Vec<String>,
    pub limits: AssetLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetPhase {
    Initial,
    Deferred,
    OnDemand,
}

impl BudgetPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Deferred => "deferred",
            Self::OnDemand => "on-demand",
        }
    }

    const fn matches(self, delivery: Delivery) -> bool {
        matches!(
            (self, delivery),
            (Self::Initial, Delivery::Initial)
                | (Self::Deferred, Delivery::Deferred)
                | (Self::OnDemand, Delivery::OnDemand)
        )
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_transfer_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_estimated_vram_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_triangles: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_draw_calls: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionReport {
    pub report_version: &'static str,
    pub manifest_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_sha256: Option<String>,
    pub work: Work,
    pub integrity_mode: IntegrityMode,
    pub valid: bool,
    pub budgets_pass: bool,
    pub asset_count: usize,
    pub variant_count: usize,
    pub total_bytes: u64,
    pub tier_totals: BTreeMap<String, TierTotals>,
    pub budget_results: Vec<BudgetResult>,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrityMode {
    Metadata,
    Files,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierTotals {
    pub variants: usize,
    pub transfer_bytes: u64,
    pub estimated_vram_bytes: u64,
    pub triangles: u64,
    pub draw_calls: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetResult {
    pub id: String,
    pub label: String,
    pub route: String,
    pub tier: Tier,
    pub phase: BudgetPhase,
    pub variant_count: usize,
    pub transfer_bytes: u64,
    pub estimated_vram_bytes: u64,
    pub triangles: u64,
    pub draw_calls: u64,
    pub limits: AssetLimits,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: String,
    pub message: String,
}

impl Issue {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetSet {
    pub baseline_version: String,
    pub measurement_plan: String,
    pub targets: Vec<TargetReference>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetReference {
    pub id: String,
    pub manifest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineReport {
    pub report_version: &'static str,
    pub baseline_version: String,
    pub measurement_plan: String,
    pub measurement_plan_sha256: String,
    pub valid: bool,
    pub budgets_pass: bool,
    pub target_count: usize,
    pub asset_count: usize,
    pub variant_count: usize,
    pub total_bytes: u64,
    pub failed_budget_count: usize,
    pub targets: Vec<InspectionReport>,
}

pub fn read_manifest(path: &Path) -> Result<AssetManifest, InspectError> {
    let bytes = read_bounded_input(path, MAX_MANIFEST_BYTES)?;
    serde_json::from_slice(&bytes).map_err(|source| InspectError::Json {
        path: path.to_path_buf(),
        source,
    })
}

pub fn inspect_path(
    manifest_path: &Path,
    asset_root: Option<&Path>,
) -> Result<InspectionReport, InspectError> {
    let bytes = read_bounded_input(manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest = serde_json::from_slice(&bytes).map_err(|source| InspectError::Json {
        path: manifest_path.to_path_buf(),
        source,
    })?;
    let mut report = inspect(&manifest, asset_root);
    report.manifest_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
    Ok(report)
}

pub fn inspect(manifest: &AssetManifest, asset_root: Option<&Path>) -> InspectionReport {
    let mut issues = Vec::new();
    let mut asset_ids = BTreeSet::new();
    let mut variant_ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut variants = BTreeMap::new();
    let mut total_bytes = 0_u64;
    let mut tier_totals = BTreeMap::new();

    for tier in Tier::ALL {
        tier_totals.insert(tier.as_str().to_owned(), TierTotals::default());
    }

    if manifest.manifest_version != MANIFEST_VERSION {
        issues.push(Issue::new(
            "manifest.version",
            format!(
                "manifestVersion must be {MANIFEST_VERSION}, got {}",
                manifest.manifest_version
            ),
        ));
    }
    validate_identifier("work.id", &manifest.work.id, &mut issues);
    validate_identifier("work.slug", &manifest.work.slug, &mut issues);
    if matches!(manifest.work.kind, WorkKind::Pliego)
        && manifest
            .work
            .signatura
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        issues.push(Issue::new(
            "work.signatura.missing",
            "a Pliego manifest requires a non-empty Signatura",
        ));
    }

    let extension_set: BTreeSet<String> = manifest
        .coverage
        .tracked_extensions
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    if extension_set.len() != manifest.coverage.tracked_extensions.len() {
        issues.push(Issue::new(
            "coverage.extension.duplicate",
            "trackedExtensions contains duplicates",
        ));
    }

    let mut excluded_paths = BTreeSet::new();
    for exclusion in &manifest.coverage.excluded {
        validate_relative_path("coverage.excluded.path", &exclusion.path, &mut issues);
        if exclusion.reason.trim().is_empty() {
            issues.push(Issue::new(
                "coverage.exclusion.reason",
                format!("{} has an empty exclusion reason", exclusion.path),
            ));
        }
        if !excluded_paths.insert(exclusion.path.clone()) {
            issues.push(Issue::new(
                "coverage.exclusion.duplicate",
                format!("{} is excluded more than once", exclusion.path),
            ));
        }
    }

    for asset in &manifest.assets {
        validate_identifier("asset.id", &asset.id, &mut issues);
        if !asset_ids.insert(asset.id.clone()) {
            issues.push(Issue::new(
                "asset.id.duplicate",
                format!("duplicate asset id {}", asset.id),
            ));
        }
        if asset.variants.is_empty() {
            issues.push(Issue::new(
                "asset.variants.empty",
                format!("asset {} has no variants", asset.id),
            ));
        }

        for variant in &asset.variants {
            validate_identifier("variant.id", &variant.id, &mut issues);
            validate_relative_path("variant.path", &variant.path, &mut issues);
            validate_sha256(&variant.id, &variant.sha256, &mut issues);
            if !variant_ids.insert(variant.id.clone()) {
                issues.push(Issue::new(
                    "variant.id.duplicate",
                    format!("duplicate variant id {}", variant.id),
                ));
            }
            if !paths.insert(variant.path.clone()) {
                issues.push(Issue::new(
                    "variant.path.duplicate",
                    format!("{} is declared by more than one variant", variant.path),
                ));
            }
            if variant.tiers.is_empty() {
                issues.push(Issue::new(
                    "variant.tiers.empty",
                    format!("variant {} has no tier", variant.id),
                ));
            }
            let unique_tiers: BTreeSet<Tier> = variant.tiers.iter().copied().collect();
            if unique_tiers.len() != variant.tiers.len() {
                issues.push(Issue::new(
                    "variant.tiers.duplicate",
                    format!("variant {} repeats a tier", variant.id),
                ));
            }
            if variant.fallback_for.iter().any(|id| id == &variant.id) {
                issues.push(Issue::new(
                    "variant.fallback.self",
                    format!("variant {} cannot fall back for itself", variant.id),
                ));
            }

            total_bytes = total_bytes.saturating_add(variant.bytes);
            for tier in &unique_tiers {
                let totals = tier_totals
                    .get_mut(tier.as_str())
                    .expect("all tiers are initialized");
                totals.variants += 1;
                totals.transfer_bytes = totals.transfer_bytes.saturating_add(variant.bytes);
                totals.estimated_vram_bytes = totals
                    .estimated_vram_bytes
                    .saturating_add(variant.estimated_vram_bytes.unwrap_or(0));
                if let Some(geometry) = variant.geometry {
                    totals.triangles = totals.triangles.saturating_add(geometry.triangles);
                    totals.draw_calls = totals.draw_calls.saturating_add(geometry.draw_calls);
                }
            }
            variants.insert(variant.id.clone(), variant);
        }
    }

    for variant in variants.values() {
        for fallback_id in &variant.fallback_for {
            if !variants.contains_key(fallback_id) {
                issues.push(Issue::new(
                    "variant.fallback.missing",
                    format!(
                        "variant {} references missing fallback target {fallback_id}",
                        variant.id
                    ),
                ));
            }
        }
    }
    for path in excluded_paths.intersection(&paths) {
        issues.push(Issue::new(
            "coverage.exclusion.declared",
            format!("{path} cannot be both declared and excluded"),
        ));
    }

    let mut budget_ids = BTreeSet::new();
    let mut budget_results = Vec::new();
    for budget in &manifest.budget_scopes {
        validate_identifier("budget.id", &budget.id, &mut issues);
        if !budget.route.starts_with('/') {
            issues.push(Issue::new(
                "budget.route.invalid",
                format!("budget {} route must start with /", budget.id),
            ));
        }
        if budget.limits.max_transfer_bytes.is_none()
            && budget.limits.max_estimated_vram_bytes.is_none()
            && budget.limits.max_triangles.is_none()
            && budget.limits.max_draw_calls.is_none()
        {
            issues.push(Issue::new(
                "budget.limits.empty",
                format!("budget {} has no enforceable asset limit", budget.id),
            ));
        }
        if !budget_ids.insert(budget.id.clone()) {
            issues.push(Issue::new(
                "budget.id.duplicate",
                format!("duplicate budget id {}", budget.id),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut transfer_bytes = 0_u64;
        let mut estimated_vram_bytes = 0_u64;
        let mut triangles = 0_u64;
        let mut draw_calls = 0_u64;

        for variant_id in &budget.variants {
            if !seen.insert(variant_id) {
                issues.push(Issue::new(
                    "budget.variant.duplicate",
                    format!("budget {} repeats variant {variant_id}", budget.id),
                ));
                continue;
            }
            let Some(variant) = variants.get(variant_id) else {
                issues.push(Issue::new(
                    "budget.variant.missing",
                    format!(
                        "budget {} references missing variant {variant_id}",
                        budget.id
                    ),
                ));
                continue;
            };
            if !variant.tiers.contains(&budget.tier) {
                issues.push(Issue::new(
                    "budget.variant.tier",
                    format!(
                        "budget {} uses {} outside tier {}",
                        budget.id,
                        variant.id,
                        budget.tier.as_str()
                    ),
                ));
            }
            if !budget.phase.matches(variant.delivery) {
                issues.push(Issue::new(
                    "budget.variant.phase",
                    format!(
                        "budget {} phase {} does not match {} delivery {}",
                        budget.id,
                        budget.phase.as_str(),
                        variant.id,
                        variant.delivery.as_str()
                    ),
                ));
            }
            transfer_bytes = transfer_bytes.saturating_add(variant.bytes);
            estimated_vram_bytes =
                estimated_vram_bytes.saturating_add(variant.estimated_vram_bytes.unwrap_or(0));
            if let Some(geometry) = variant.geometry {
                triangles = triangles.saturating_add(geometry.triangles);
                draw_calls = draw_calls.saturating_add(geometry.draw_calls);
            }
        }

        let passed = within_limit(transfer_bytes, budget.limits.max_transfer_bytes)
            && within_limit(estimated_vram_bytes, budget.limits.max_estimated_vram_bytes)
            && within_limit(triangles, budget.limits.max_triangles)
            && within_limit(draw_calls, budget.limits.max_draw_calls);
        budget_results.push(BudgetResult {
            id: budget.id.clone(),
            label: budget.label.clone(),
            route: budget.route.clone(),
            tier: budget.tier,
            phase: budget.phase,
            variant_count: seen.len(),
            transfer_bytes,
            estimated_vram_bytes,
            triangles,
            draw_calls,
            limits: budget.limits.clone(),
            passed,
        });
    }

    if let Some(root) = asset_root {
        verify_files(
            root,
            &extension_set,
            &paths,
            &excluded_paths,
            &variants,
            &mut issues,
        );
    }

    let budgets_pass = budget_results.iter().all(|budget| budget.passed);
    InspectionReport {
        report_version: REPORT_VERSION,
        manifest_version: manifest.manifest_version.clone(),
        manifest_sha256: None,
        work: manifest.work.clone(),
        integrity_mode: if asset_root.is_some() {
            IntegrityMode::Files
        } else {
            IntegrityMode::Metadata
        },
        valid: issues.is_empty(),
        budgets_pass,
        asset_count: manifest.assets.len(),
        variant_count: variants.len(),
        total_bytes,
        tier_totals,
        budget_results,
        issues,
    }
}

pub fn baseline(target_set_path: &Path) -> Result<BaselineReport, InspectError> {
    let bytes = read_bounded_input(target_set_path, MAX_TARGET_SET_BYTES)?;
    let target_set: TargetSet =
        serde_json::from_slice(&bytes).map_err(|source| InspectError::Json {
            path: target_set_path.to_path_buf(),
            source,
        })?;
    let base = target_set_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|source| InspectError::Io {
            path: target_set_path.to_path_buf(),
            source,
        })?;
    let measurement_plan_path =
        confined_baseline_reference(&base, &target_set.measurement_plan, "measurementPlan")?;
    let (_, measurement_plan_sha256) =
        hash_file(&measurement_plan_path).map_err(|source| InspectError::Io {
            path: measurement_plan_path,
            source,
        })?;
    let mut ids = BTreeSet::new();
    let mut targets = Vec::new();
    for target in &target_set.targets {
        if !ids.insert(target.id.clone()) {
            return Err(InspectError::InvalidTargetSet(format!(
                "duplicate target id {}",
                target.id
            )));
        }
        let manifest_path =
            confined_baseline_reference(&base, &target.manifest, "target manifest")?;
        let report = inspect_path(&manifest_path, None)?;
        if report.work.id != target.id {
            return Err(InspectError::InvalidTargetSet(format!(
                "target id {} does not match manifest work id {}",
                target.id, report.work.id
            )));
        }
        targets.push(report);
    }

    let asset_count = targets.iter().map(|report| report.asset_count).sum();
    let variant_count = targets.iter().map(|report| report.variant_count).sum();
    let total_bytes = targets.iter().map(|report| report.total_bytes).sum();
    let failed_budget_count = targets
        .iter()
        .flat_map(|report| &report.budget_results)
        .filter(|budget| !budget.passed)
        .count();
    let valid = targets.iter().all(|report| report.valid);
    let budgets_pass = targets.iter().all(|report| report.budgets_pass);
    Ok(BaselineReport {
        report_version: REPORT_VERSION,
        baseline_version: target_set.baseline_version,
        measurement_plan: target_set.measurement_plan,
        measurement_plan_sha256,
        valid,
        budgets_pass,
        target_count: targets.len(),
        asset_count,
        variant_count,
        total_bytes,
        failed_budget_count,
        targets,
    })
}

pub fn json_pretty<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut output = serde_json::to_string_pretty(value)?;
    output.push('\n');
    Ok(output)
}

pub fn human_report(report: &InspectionReport) -> String {
    let mut output = String::new();
    output.push_str("PLIEGO INSPECT 1.0\n");
    output.push_str(&format!("{} ({})\n", report.work.title, report.work.id));
    output.push_str(&format!(
        "assets {} | variants {} | bytes {} | integrity {:?}\n",
        report.asset_count, report.variant_count, report.total_bytes, report.integrity_mode
    ));
    if let Some(hash) = &report.manifest_sha256 {
        output.push_str(&format!("manifest sha256 {hash}\n"));
    }
    output.push_str("\nTIER        VARIANTS       BYTES        VRAM   TRIANGLES  DRAWS\n");
    for tier in Tier::ALL {
        let totals = report
            .tier_totals
            .get(tier.as_str())
            .expect("all tiers are initialized");
        output.push_str(&format!(
            "{:<12}{:>8}{:>12}{:>12}{:>12}{:>7}\n",
            tier.as_str(),
            totals.variants,
            totals.transfer_bytes,
            totals.estimated_vram_bytes,
            totals.triangles,
            totals.draw_calls
        ));
    }
    output.push_str("\nBUDGETS\n");
    if report.budget_results.is_empty() {
        output.push_str("none declared\n");
    } else {
        for budget in &report.budget_results {
            output.push_str(&format!(
                "{} | {} | {} | {} bytes | {}\n",
                if budget.passed { "PASS" } else { "OVER" },
                budget.id,
                budget.tier.as_str(),
                budget.transfer_bytes,
                budget.route
            ));
        }
    }
    if !report.issues.is_empty() {
        output.push_str("\nISSUES\n");
        for issue in &report.issues {
            output.push_str(&format!("{}: {}\n", issue.code, issue.message));
        }
    }
    output.push_str(&format!(
        "\nRESULT {} | BUDGETS {}\n",
        if report.valid { "VALID" } else { "INVALID" },
        if report.budgets_pass { "PASS" } else { "OVER" }
    ));
    output
}

pub fn human_baseline(report: &BaselineReport) -> String {
    let mut output = String::new();
    output.push_str("PLIEGO BASELINE 1.0\n");
    output.push_str(&format!(
        "targets {} | assets {} | variants {} | bytes {}\n",
        report.target_count, report.asset_count, report.variant_count, report.total_bytes
    ));
    for target in &report.targets {
        output.push_str(&format!(
            "{} | {} assets | {} variants | {} bytes | {}\n",
            target.work.id,
            target.asset_count,
            target.variant_count,
            target.total_bytes,
            if target.budgets_pass {
                "budgets pass"
            } else {
                "budget debt"
            }
        ));
    }
    output.push_str(&format!(
        "RESULT {} | FAILED BUDGETS {}\n",
        if report.valid { "VALID" } else { "INVALID" },
        report.failed_budget_count
    ));
    output
}

fn validate_identifier(field: &str, value: &str, issues: &mut Vec<Issue>) {
    let valid = !value.is_empty()
        && value.len() <= 120
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'-' => index > 0 && index + 1 < value.len(),
            _ => false,
        })
        && !value.contains("..")
        && !value.contains(".-")
        && !value.contains("-.")
        && !value.contains("--");
    if !valid {
        issues.push(Issue::new(
            "identifier.invalid",
            format!("{field} has invalid identifier {value:?}"),
        ));
    }
}

fn validate_relative_path(field: &str, value: &str, issues: &mut Vec<Issue>) {
    let path = Path::new(value);
    let invalid_component = path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if value.is_empty() || value.contains('\\') || path.is_absolute() || invalid_component {
        issues.push(Issue::new(
            "path.invalid",
            format!("{field} must be a relative POSIX path, got {value:?}"),
        ));
    }
}

fn validate_sha256(variant_id: &str, value: &str, issues: &mut Vec<Issue>) {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        issues.push(Issue::new(
            "variant.sha256.invalid",
            format!("variant {variant_id} has invalid lowercase SHA-256"),
        ));
    }
}

fn within_limit(value: u64, limit: Option<u64>) -> bool {
    limit.is_none_or(|maximum| value <= maximum)
}

fn verify_files(
    root: &Path,
    tracked_extensions: &BTreeSet<String>,
    declared_paths: &BTreeSet<String>,
    excluded_paths: &BTreeSet<String>,
    variants: &BTreeMap<String, &Variant>,
    issues: &mut Vec<Issue>,
) {
    let root = match fs::symlink_metadata(root).and_then(|metadata| {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "asset root must be a real directory, not a link",
            ))
        } else {
            root.canonicalize()
        }
    }) {
        Ok(root) => root,
        Err(source) => {
            issues.push(Issue::new(
                "coverage.root",
                format!("cannot secure asset root: {source}"),
            ));
            return;
        }
    };
    for variant in variants.values() {
        let path = root.join(path_from_manifest(&variant.path));
        match hash_confined_file(&root, &path) {
            Ok((bytes, hash)) => {
                if bytes != variant.bytes {
                    issues.push(Issue::new(
                        "file.bytes.mismatch",
                        format!(
                            "{} declares {} bytes but disk has {bytes}",
                            variant.path, variant.bytes
                        ),
                    ));
                }
                if hash != variant.sha256 {
                    issues.push(Issue::new(
                        "file.sha256.mismatch",
                        format!("{} does not match its declared SHA-256", variant.path),
                    ));
                }
            }
            Err(source) => issues.push(Issue::new(
                "file.missing",
                format!("cannot read {}: {source}", path.display()),
            )),
        }
    }

    let mut disk_paths = BTreeSet::new();
    if let Err(source) = collect_tracked_files(&root, &root, tracked_extensions, &mut disk_paths) {
        issues.push(Issue::new(
            "coverage.scan",
            format!("cannot scan {}: {source}", root.display()),
        ));
        return;
    }
    for path in &disk_paths {
        if !declared_paths.contains(path) && !excluded_paths.contains(path) {
            issues.push(Issue::new(
                "coverage.untracked",
                format!("tracked file {path} is absent from the manifest"),
            ));
        }
    }
    for path in declared_paths {
        if !disk_paths.contains(path) {
            issues.push(Issue::new(
                "coverage.declared-missing",
                format!("declared tracked file {path} is absent from disk"),
            ));
        }
    }
}

fn collect_tracked_files(
    root: &Path,
    directory: &Path,
    tracked_extensions: &BTreeSet<String>,
    paths: &mut BTreeSet<String>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "symbolic links are not valid asset inputs: {}",
                    path.display()
                ),
            ));
        }
        if file_type.is_dir() {
            collect_tracked_files(root, &path, tracked_extensions, paths)?;
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{}", value.to_ascii_lowercase()));
        if extension
            .as_ref()
            .is_some_and(|value| tracked_extensions.contains(value))
        {
            let relative = path
                .strip_prefix(root)
                .expect("walked paths remain below root")
                .to_string_lossy()
                .replace('\\', "/");
            paths.insert(relative);
        }
    }
    Ok(())
}

fn hash_confined_file(root: &Path, path: &Path) -> io::Result<(u64, String)> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "asset variant must be a real file",
        ));
    }
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "asset variant resolves outside the asset root",
        ));
    }
    hash_file(&canonical)
}

fn read_bounded_input(path: &Path, limit: u64) -> Result<Vec<u8>, InspectError> {
    let file = open_regular_nofollow(path).map_err(|source| InspectError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| InspectError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        return Err(InspectError::InvalidInput {
            path: path.to_path_buf(),
            message: format!("{} bytes exceeds the {limit}-byte limit", metadata.len()),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| InspectError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(InspectError::InvalidInput {
            path: path.to_path_buf(),
            message: format!("input grew beyond the {limit}-byte limit while reading"),
        });
    }
    Ok(bytes)
}

fn hash_file(path: &Path) -> io::Result<(u64, String)> {
    let mut file = open_regular_nofollow(path)?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        hash.update(&buffer[..read]);
    }
    Ok((bytes, format!("{:x}", hash.finalize())))
}

fn open_regular_nofollow(path: &Path) -> io::Result<fs::File> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let directory = Dir::open_ambient_dir(parent, ambient_authority())?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular file, not a link or directory",
        ));
    }
    Ok(file.into_std())
}

fn path_from_manifest(value: &str) -> PathBuf {
    value.split('/').collect()
}

fn confined_baseline_reference(
    base: &Path,
    value: &str,
    field: &str,
) -> Result<PathBuf, InspectError> {
    let relative = path_from_manifest(value);
    if value.is_empty()
        || value.contains(['\\', '\0'])
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(InspectError::InvalidTargetSet(format!(
            "{field} must be a normalized relative POSIX path, got {value:?}"
        )));
    }
    let candidate = base.join(relative);
    let canonical = candidate
        .canonicalize()
        .map_err(|source| InspectError::Io {
            path: candidate.clone(),
            source,
        })?;
    if !canonical.starts_with(base) {
        return Err(InspectError::InvalidTargetSet(format!(
            "{field} resolves outside the target-set directory: {value:?}"
        )));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory(label: &str) -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pliego-inspect-{label}-{}-{id}",
            std::process::id()
        ));
        if path.exists() {
            fs::remove_dir_all(&path).expect("remove stale test directory");
        }
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    fn manifest_json(hash: &str, bytes: u64) -> serde_json::Value {
        json!({
            "manifestVersion": "1.0.0",
            "work": {
                "kind": "pliego",
                "id": "pliego-999-test",
                "slug": "test",
                "title": "Test Pliego",
                "owner": "PLIEGO",
                "signatura": "PLIEGO / 999 / TEST"
            },
            "coverage": {
                "assetRoot": "public",
                "trackedExtensions": [".bin"],
                "excluded": []
            },
            "assets": [{
                "id": "test-asset",
                "label": "Test asset",
                "owner": { "entity": "PLIEGO", "role": "house" },
                "source": { "kind": "owned", "locator": "fixtures/test.bin" },
                "rights": {
                    "status": "owned",
                    "holder": "PLIEGO",
                    "transfer": "included"
                },
                "visualImportance": "critical",
                "variants": [{
                    "id": "test-bin",
                    "path": "test.bin",
                    "mediaType": "application/octet-stream",
                    "format": "bin",
                    "sha256": hash,
                    "bytes": bytes,
                    "tiers": ["universal"],
                    "delivery": "initial",
                    "preload": true
                }]
            }],
            "budgetScopes": [{
                "id": "home-universal-initial",
                "label": "Home initial",
                "route": "/",
                "tier": "universal",
                "phase": "initial",
                "variants": ["test-bin"],
                "limits": { "maxTransferBytes": 3 }
            }]
        })
    }

    #[test]
    fn valid_manifest_is_deterministic() {
        let digest = format!("{:x}", Sha256::digest(b"abc"));
        let manifest: AssetManifest =
            serde_json::from_value(manifest_json(&digest, 3)).expect("valid manifest");
        let first = inspect(&manifest, None);
        let second = inspect(&manifest, None);
        assert!(first.valid);
        assert!(first.budgets_pass);
        assert_eq!(json_pretty(&first).unwrap(), json_pretty(&second).unwrap());
    }

    #[test]
    fn file_verification_detects_hash_mismatch() {
        let root = temporary_directory("hash");
        fs::write(root.join("test.bin"), b"abd").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"abc"));
        let manifest: AssetManifest =
            serde_json::from_value(manifest_json(&expected, 3)).expect("valid manifest");
        let report = inspect(&manifest, Some(&root));
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "file.sha256.mismatch")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn coverage_detects_untracked_files() {
        let root = temporary_directory("coverage");
        fs::write(root.join("test.bin"), b"abc").unwrap();
        fs::write(root.join("extra.bin"), b"extra").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"abc"));
        let manifest: AssetManifest =
            serde_json::from_value(manifest_json(&digest, 3)).expect("valid manifest");
        let report = inspect(&manifest, Some(&root));
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "coverage.untracked")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn budget_debt_is_separate_from_manifest_validity() {
        let digest = format!("{:x}", Sha256::digest(b"abcd"));
        let manifest: AssetManifest =
            serde_json::from_value(manifest_json(&digest, 4)).expect("valid manifest");
        let report = inspect(&manifest, None);
        assert!(report.valid);
        assert!(!report.budgets_pass);
    }

    #[test]
    fn duplicate_variant_path_is_invalid() {
        let digest = format!("{:x}", Sha256::digest(b"abc"));
        let mut value = manifest_json(&digest, 3);
        let mut duplicate = value["assets"][0]["variants"][0].clone();
        duplicate["id"] = json!("test-bin-copy");
        value["assets"][0]["variants"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let manifest: AssetManifest = serde_json::from_value(value).expect("parse manifest");
        let report = inspect(&manifest, None);
        assert!(!report.valid);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "variant.path.duplicate")
        );
    }

    #[test]
    fn bounded_inputs_fail_before_reading_oversized_files() {
        let root = temporary_directory("bounded");
        let input = root.join("manifest.json");
        fs::write(&input, b"1234").unwrap();
        assert!(matches!(
            read_bounded_input(&input, 3),
            Err(InspectError::InvalidInput { .. })
        ));

        let outside = root.with_extension("outside.json");
        fs::write(&outside, b"{}").unwrap();
        let linked = root.join("linked.json");
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(&outside, &linked);
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(&outside, &linked);
        if result.is_ok() {
            assert!(read_bounded_input(&linked, MAX_MANIFEST_BYTES).is_err());
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn asset_integrity_never_follows_symbolic_links() {
        let root = temporary_directory("linked");
        let outside = root.with_extension("outside.bin");
        fs::write(&outside, b"abc").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, root.join("test.bin"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("test.bin"));
        if linked.is_ok() {
            let digest = format!("{:x}", Sha256::digest(b"abc"));
            let manifest: AssetManifest =
                serde_json::from_value(manifest_json(&digest, 3)).expect("valid manifest");
            let report = inspect(&manifest, Some(&root));
            assert!(!report.valid);
            assert!(
                report.issues.iter().any(|issue| {
                    matches!(issue.code.as_str(), "file.missing" | "coverage.scan")
                })
            );
        }
        fs::remove_dir_all(root).unwrap();
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn baseline_references_cannot_escape_the_target_set_directory() {
        let root = temporary_directory("baseline-confined");
        let baseline_dir = root.join("baseline");
        fs::create_dir(&baseline_dir).unwrap();
        let outside_plan = root.join("outside-plan.md");
        fs::write(&outside_plan, b"outside").unwrap();

        let target_set = baseline_dir.join("targets.json");
        fs::write(
            &target_set,
            serde_json::to_vec(&json!({
                "baselineVersion": "1",
                "measurementPlan": "../outside-plan.md",
                "targets": []
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            baseline(&target_set),
            Err(InspectError::InvalidTargetSet(_))
        ));

        fs::write(baseline_dir.join("plan.md"), b"inside").unwrap();
        let outside_manifest = root.join("outside-manifest.json");
        fs::write(&outside_manifest, b"{}").unwrap();
        fs::write(
            &target_set,
            serde_json::to_vec(&json!({
                "baselineVersion": "1",
                "measurementPlan": "plan.md",
                "targets": [{
                    "id": "pliego-999-test",
                    "manifest": "../outside-manifest.json"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            baseline(&target_set),
            Err(InspectError::InvalidTargetSet(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
