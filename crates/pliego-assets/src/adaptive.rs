//! Typed, toolchain-neutral plans for adaptive images, video, fonts, and 3D.

use crate::{
    AssetError, hash_open_file, open_regular_nofollow, publish_open_file_no_clobber,
    reject_symlink, sha256, sha256_file, valid_identifier,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

pub const ADAPTIVE_RECIPE_VERSION: &str = "1.0.0";
pub const ADAPTIVE_PLAN_VERSION: &str = "1.0.0";
pub const ADAPTIVE_MANIFEST_VERSION: &str = "1.0.0";
pub const ADAPTIVE_RECIPE_SCHEMA: &str =
    "https://pliegors.dev/schemas/pliego.adaptive-asset-recipe.schema.json";
pub const ADAPTIVE_PLAN_SCHEMA: &str =
    "https://pliegors.dev/schemas/pliego.adaptive-asset-plan.schema.json";
pub const ADAPTIVE_MANIFEST_SCHEMA: &str =
    "https://pliegors.dev/schemas/pliego.adaptive-asset-manifest.schema.json";
const MAX_ASSETS: usize = 512;
const MAX_JOBS: usize = 4096;
const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_RECIPE_SOURCE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_RECIPE_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Universal,
    Lite,
    Balanced,
    Signature,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Delivery {
    Initial,
    Deferred,
    OnDemand,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FetchPriority {
    High,
    Auto,
    Low,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MotionPolicy {
    Static,
    SuppressWhenReduced,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadPolicy {
    pub tiers: BTreeSet<Tier>,
    pub delivery: Delivery,
    #[serde(default)]
    pub preload: bool,
    pub fetch_priority: FetchPriority,
    pub motion: MotionPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Toolchain {
    Ffmpeg,
    Fonttools,
    Blender,
    GltfTransform,
    Ktx2Encoder,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolchainPin {
    pub name: Toolchain,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceClass {
    ModestMobile,
    Tablet,
    Desktop,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TierBudget {
    pub tier: Tier,
    pub max_initial_bytes: u64,
    pub max_deferred_bytes: u64,
    pub max_on_demand_bytes: u64,
    pub max_decoded_bytes: u64,
    pub max_triangles: u64,
    pub max_draw_calls: u64,
    pub max_preloads: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceBudgetProfile {
    pub id: String,
    pub device: DeviceClass,
    pub tiers: Vec<TierBudget>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Avif,
    Webp,
    Jpeg,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VideoFormat {
    Mp4H264,
    Mp4Av1,
    WebmVp9,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MeshCompression {
    None,
    Meshopt,
    Draco,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextureCompression {
    Ktx2Etc1s,
    Ktx2Uastc,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageOutput {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub quality: u8,
    pub policy: LoadPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoOutput {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub format: VideoFormat,
    pub bitrate_kbps: u32,
    pub max_fps: u16,
    #[serde(default)]
    pub audio: bool,
    pub policy: LoadPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontOutput {
    pub id: String,
    pub glyphs: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unicode_range: Option<String>,
    pub policy: LoadPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneOutput {
    pub id: String,
    pub max_triangles: u64,
    pub max_draw_calls: u64,
    pub mesh_compression: MeshCompression,
    pub texture_compression: TextureCompression,
    pub max_texture_dimension: u32,
    #[serde(default)]
    pub requires_blender: bool,
    pub policy: LoadPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TransformRecipe {
    Image { outputs: Vec<ImageOutput> },
    Video { outputs: Vec<VideoOutput> },
    Font { outputs: Vec<FontOutput> },
    Scene3d { outputs: Vec<SceneOutput> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRecipe {
    pub id: String,
    pub input: String,
    pub transform: TransformRecipe,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptiveRecipe {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub recipe_version: String,
    pub toolchains: Vec<ToolchainPin>,
    pub budget_profiles: Vec<DeviceBudgetProfile>,
    pub assets: Vec<AssetRecipe>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum JobOperation {
    Image {
        width: u32,
        height: u32,
        format: ImageFormat,
        quality: u8,
    },
    Video {
        width: u32,
        height: u32,
        format: VideoFormat,
        bitrate_kbps: u32,
        max_fps: u16,
        audio: bool,
    },
    FontSubset {
        glyphs: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unicode_range: Option<String>,
    },
    Scene3d {
        max_triangles: u64,
        max_draw_calls: u64,
        mesh_compression: MeshCompression,
        texture_compression: TextureCompression,
        max_texture_dimension: u32,
        requires_blender: bool,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSource {
    pub id: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetJob {
    pub id: String,
    pub source_id: String,
    pub variant_id: String,
    pub staging_path: String,
    pub format: ArtifactFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_group: Option<String>,
    pub required_toolchains: BTreeSet<Toolchain>,
    pub policy: LoadPolicy,
    pub operation: JobOperation,
    pub estimated_decoded_bytes: u64,
    pub estimated_triangles: u64,
    pub estimated_draw_calls: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactFormat {
    Avif,
    Webp,
    Jpeg,
    Mp4,
    Webm,
    Woff2,
    Glb,
}

impl ArtifactFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Avif => "avif",
            Self::Webp => "webp",
            Self::Jpeg => "jpg",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Woff2 => "woff2",
            Self::Glb => "glb",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Avif => "image/avif",
            Self::Webp => "image/webp",
            Self::Jpeg => "image/jpeg",
            Self::Mp4 => "video/mp4",
            Self::Webm => "video/webm",
            Self::Woff2 => "font/woff2",
            Self::Glb => "model/gltf-binary",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptivePlan {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub plan_version: String,
    pub recipe_sha256: String,
    pub toolchains: Vec<ToolchainPin>,
    pub budget_profiles: Vec<DeviceBudgetProfile>,
    pub sources: Vec<PlanSource>,
    pub jobs: Vec<AssetJob>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedVariant {
    pub id: String,
    pub source_id: String,
    pub path: String,
    pub format: ArtifactFormat,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_group: Option<String>,
    pub required_toolchains: BTreeSet<Toolchain>,
    pub operation: JobOperation,
    pub sha256: String,
    pub bytes: u64,
    pub policy: LoadPolicy,
    pub estimated_decoded_bytes: u64,
    pub estimated_triangles: u64,
    pub estimated_draw_calls: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetTotals {
    pub initial_bytes: u64,
    pub deferred_bytes: u64,
    pub on_demand_bytes: u64,
    pub decoded_bytes: u64,
    pub triangles: u64,
    pub draw_calls: u64,
    pub preloads: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetResult {
    pub profile_id: String,
    pub tier: Tier,
    pub pass: bool,
    pub totals: BudgetTotals,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdaptiveManifest {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub manifest_version: String,
    pub recipe_sha256: String,
    pub toolchains: Vec<ToolchainPin>,
    pub sources: Vec<PlanSource>,
    pub variants: Vec<PublishedVariant>,
    pub budget_results: Vec<BudgetResult>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeConstraints {
    pub tier: Tier,
    pub reduced_motion: bool,
    pub save_data: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadStrategy {
    Eager,
    Lazy,
    Interaction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryDirective {
    pub variant_id: String,
    pub path: String,
    pub media_type: String,
    pub strategy: LoadStrategy,
    pub preload: bool,
    pub fetch_priority: FetchPriority,
}

impl AdaptiveRecipe {
    pub fn from_json(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(AssetError::Json)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, AssetError> {
        pretty_json(self)
    }
}

impl AdaptivePlan {
    pub fn from_json(bytes: &[u8]) -> Result<Self, AssetError> {
        serde_json::from_slice(bytes).map_err(AssetError::Json)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, AssetError> {
        pretty_json(self)
    }
}

impl AdaptiveManifest {
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, AssetError> {
        pretty_json(self)
    }

    /// Resolves the deterministic network policy for a concrete client. Motion
    /// assets disappear under reduced motion, while Save-Data keeps only static
    /// initial assets. The caller still owns codec capability negotiation.
    pub fn delivery(&self, constraints: RuntimeConstraints) -> Vec<DeliveryDirective> {
        self.variants
            .iter()
            .filter(|variant| variant.policy.tiers.contains(&constraints.tier))
            .filter(|variant| {
                !(constraints.reduced_motion
                    && variant.policy.motion == MotionPolicy::SuppressWhenReduced)
            })
            .filter(|variant| {
                !constraints.save_data
                    || (variant.policy.delivery == Delivery::Initial
                        && variant.policy.motion == MotionPolicy::Static)
            })
            .map(|variant| DeliveryDirective {
                variant_id: variant.id.clone(),
                path: variant.path.clone(),
                media_type: variant.media_type.clone(),
                strategy: match variant.policy.delivery {
                    Delivery::Initial => LoadStrategy::Eager,
                    Delivery::Deferred => LoadStrategy::Lazy,
                    Delivery::OnDemand => LoadStrategy::Interaction,
                },
                preload: variant.policy.preload,
                fetch_priority: variant.policy.fetch_priority,
            })
            .collect()
    }
}

pub fn plan_adaptive_assets(
    recipe: &AdaptiveRecipe,
    source_root: &Path,
) -> Result<AdaptivePlan, AssetError> {
    validate_recipe_contract(recipe)?;
    let canonical_root = source_root
        .canonicalize()
        .map_err(|source| AssetError::Io {
            path: source_root.to_path_buf(),
            source,
        })?;
    if !canonical_root.is_dir() {
        return Err(invalid("source root must be a directory"));
    }
    let recipe_bytes = serde_json::to_vec(recipe).map_err(AssetError::Json)?;
    let recipe_sha256 = sha256(&recipe_bytes);
    let mut sources = Vec::with_capacity(recipe.assets.len());
    let mut jobs = Vec::new();
    let mut total_source_bytes = 0_u64;

    for asset in &recipe.assets {
        let relative = validate_relative_path(&asset.input)?;
        reject_symlink_components(&canonical_root, &relative)?;
        let input = canonical_root.join(&relative);
        reject_symlink(&input)?;
        let canonical_input = input.canonicalize().map_err(|source| AssetError::Io {
            path: input.clone(),
            source,
        })?;
        if !canonical_input.starts_with(&canonical_root) {
            return Err(invalid(format!(
                "asset {} resolves outside the source root",
                asset.id
            )));
        }
        let mut source_file = open_regular_nofollow(&canonical_input)?;
        let metadata = source_file.metadata().map_err(|source| AssetError::Io {
            path: canonical_input.clone(),
            source,
        })?;
        if metadata.len() == 0 || metadata.len() > MAX_SOURCE_BYTES {
            return Err(invalid(format!(
                "asset {} source bytes must be in 1..={MAX_SOURCE_BYTES}",
                asset.id
            )));
        }
        total_source_bytes = total_source_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| invalid("source byte total overflow"))?;
        if total_source_bytes > MAX_RECIPE_SOURCE_BYTES {
            return Err(invalid(format!(
                "recipe sources exceed {MAX_RECIPE_SOURCE_BYTES} bytes"
            )));
        }
        sources.push(PlanSource {
            id: asset.id.clone(),
            path: slash_path(&relative),
            sha256: hash_open_file(&mut source_file, &canonical_input)?,
            bytes: metadata.len(),
        });
        append_jobs(asset, &mut jobs)?;
        if jobs.len() > MAX_JOBS {
            return Err(invalid(format!("recipe exceeds {MAX_JOBS} output jobs")));
        }
    }
    sources.sort_by(|left, right| left.id.cmp(&right.id));
    jobs.sort_by(|left, right| left.id.cmp(&right.id));
    let plan = AdaptivePlan {
        schema: ADAPTIVE_PLAN_SCHEMA.to_owned(),
        plan_version: ADAPTIVE_PLAN_VERSION.to_owned(),
        recipe_sha256,
        toolchains: sorted_toolchains(&recipe.toolchains),
        budget_profiles: sorted_profiles(&recipe.budget_profiles),
        sources,
        jobs,
    };
    validate_plan(&plan)?;
    Ok(plan)
}

pub fn finalize_adaptive_assets(
    plan: &AdaptivePlan,
    output_root: &Path,
) -> Result<AdaptiveManifest, AssetError> {
    validate_plan(plan)?;
    fs::create_dir_all(output_root).map_err(|source| AssetError::Io {
        path: output_root.to_path_buf(),
        source,
    })?;
    let canonical_root = output_root
        .canonicalize()
        .map_err(|source| AssetError::Io {
            path: output_root.to_path_buf(),
            source,
        })?;
    let mut staged_inputs = Vec::with_capacity(plan.jobs.len());
    let mut total_artifact_bytes = 0_u64;
    for job in &plan.jobs {
        let relative = validate_relative_path(&job.staging_path)?;
        reject_symlink_components(&canonical_root, &relative)?;
        let staged = canonical_root.join(&relative);
        reject_symlink(&staged)?;
        let canonical = staged.canonicalize().map_err(|source| AssetError::Io {
            path: staged.clone(),
            source,
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(invalid(format!("job {} escaped the output root", job.id)));
        }
        let file = open_regular_nofollow(&canonical)?;
        let metadata = file.metadata().map_err(|source| AssetError::Io {
            path: canonical.clone(),
            source,
        })?;
        if metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
            return Err(invalid(format!(
                "job {} artifact bytes must be in 1..={MAX_ARTIFACT_BYTES}",
                job.id
            )));
        }
        total_artifact_bytes = checked_artifact_total(total_artifact_bytes, metadata.len())?;
        staged_inputs.push((job, file, canonical, metadata.len()));
    }

    let mut candidates = Vec::with_capacity(staged_inputs.len());
    for (job, mut file, canonical, artifact_bytes) in staged_inputs {
        validate_artifact(&mut file, &canonical, job)?;
        let digest = hash_open_file(&mut file, &canonical)?;
        let destination = format!(
            "assets/{}/{}.{}.{}",
            job.source_id,
            job.variant_id,
            &digest[..16],
            job.format.extension()
        );
        candidates.push((
            file,
            canonical,
            PublishedVariant {
                id: job.id.clone(),
                source_id: job.source_id.clone(),
                path: destination,
                format: job.format,
                media_type: job.format.media_type().to_owned(),
                selection_group: job.selection_group.clone(),
                required_toolchains: job.required_toolchains.clone(),
                operation: job.operation.clone(),
                sha256: digest,
                bytes: artifact_bytes,
                policy: job.policy.clone(),
                estimated_decoded_bytes: job.estimated_decoded_bytes,
                estimated_triangles: job.estimated_triangles,
                estimated_draw_calls: job.estimated_draw_calls,
            },
        ));
    }

    let variants: Vec<PublishedVariant> = candidates
        .iter()
        .map(|(_, _, variant)| variant.clone())
        .collect();
    let budget_results = evaluate_budgets(&plan.budget_profiles, &variants)?;
    let failures: Vec<String> = budget_results
        .iter()
        .filter(|result| !result.pass)
        .map(|result| format!("{}:{:?}", result.profile_id, result.tier))
        .collect();
    if !failures.is_empty() {
        return Err(invalid(format!(
            "adaptive asset budgets exceeded for {}",
            failures.join(", ")
        )));
    }

    for (_, _, variant) in &candidates {
        let relative = validate_relative_path(&variant.path)?;
        let destination = canonical_root.join(relative);
        let parent = destination
            .parent()
            .ok_or_else(|| invalid("published artifact has no parent"))?;
        secure_create_directory(parent, &canonical_root)?;
        if destination.exists() {
            reject_symlink(&destination)?;
            if sha256_file(&destination)? != variant.sha256 {
                return Err(invalid(format!(
                    "content-address collision at {}",
                    destination.display()
                )));
            }
        }
    }
    for (file, staged, variant) in &mut candidates {
        let destination = canonical_root.join(validate_relative_path(&variant.path)?);
        publish_open_file_no_clobber(file, staged, &destination, &variant.sha256)?;
    }

    Ok(AdaptiveManifest {
        schema: ADAPTIVE_MANIFEST_SCHEMA.to_owned(),
        manifest_version: ADAPTIVE_MANIFEST_VERSION.to_owned(),
        recipe_sha256: plan.recipe_sha256.clone(),
        toolchains: plan.toolchains.clone(),
        sources: plan.sources.clone(),
        variants,
        budget_results,
    })
}

fn checked_artifact_total(current: u64, artifact_bytes: u64) -> Result<u64, AssetError> {
    let total = current
        .checked_add(artifact_bytes)
        .ok_or_else(|| invalid("adaptive artifact byte total overflow"))?;
    if total > MAX_RECIPE_ARTIFACT_BYTES {
        return Err(invalid(format!(
            "adaptive artifacts exceed {MAX_RECIPE_ARTIFACT_BYTES} aggregate bytes"
        )));
    }
    Ok(total)
}

pub fn evaluate_budgets(
    profiles: &[DeviceBudgetProfile],
    variants: &[PublishedVariant],
) -> Result<Vec<BudgetResult>, AssetError> {
    let mut results = Vec::new();
    for profile in profiles {
        for budget in &profile.tiers {
            let mut totals = BudgetTotals::default();
            let applicable: Vec<&PublishedVariant> = variants
                .iter()
                .filter(|variant| variant.policy.tiers.contains(&budget.tier))
                .collect();
            let mut groups: BTreeMap<&str, Vec<&PublishedVariant>> = BTreeMap::new();
            for variant in applicable {
                if let Some(group) = &variant.selection_group {
                    groups.entry(group).or_default().push(variant);
                } else {
                    accumulate_variant(&mut totals, variant)?;
                }
            }
            for alternatives in groups.values() {
                let mut worst = BudgetTotals::default();
                for variant in alternatives {
                    match variant.policy.delivery {
                        Delivery::Initial => {
                            worst.initial_bytes = worst.initial_bytes.max(variant.bytes);
                        }
                        Delivery::Deferred => {
                            worst.deferred_bytes = worst.deferred_bytes.max(variant.bytes);
                        }
                        Delivery::OnDemand => {
                            worst.on_demand_bytes = worst.on_demand_bytes.max(variant.bytes);
                        }
                    }
                    worst.decoded_bytes = worst.decoded_bytes.max(variant.estimated_decoded_bytes);
                    worst.triangles = worst.triangles.max(variant.estimated_triangles);
                    worst.draw_calls = worst.draw_calls.max(variant.estimated_draw_calls);
                    worst.preloads = worst.preloads.max(u32::from(variant.policy.preload));
                }
                add_totals(&mut totals, &worst)?;
            }
            let pass = totals.initial_bytes <= budget.max_initial_bytes
                && totals.deferred_bytes <= budget.max_deferred_bytes
                && totals.on_demand_bytes <= budget.max_on_demand_bytes
                && totals.decoded_bytes <= budget.max_decoded_bytes
                && totals.triangles <= budget.max_triangles
                && totals.draw_calls <= budget.max_draw_calls
                && totals.preloads <= budget.max_preloads;
            results.push(BudgetResult {
                profile_id: profile.id.clone(),
                tier: budget.tier,
                pass,
                totals,
            });
        }
    }
    Ok(results)
}

fn validate_recipe_contract(recipe: &AdaptiveRecipe) -> Result<(), AssetError> {
    if recipe
        .schema
        .as_ref()
        .is_some_and(|schema| schema != ADAPTIVE_RECIPE_SCHEMA)
    {
        return Err(invalid("adaptive recipe references an unsupported schema"));
    }
    if recipe.recipe_version != ADAPTIVE_RECIPE_VERSION {
        return Err(invalid(format!(
            "unsupported adaptive recipe version {}",
            recipe.recipe_version
        )));
    }
    if recipe.assets.is_empty() || recipe.assets.len() > MAX_ASSETS {
        return Err(invalid(format!(
            "recipe assets must be in 1..={MAX_ASSETS}"
        )));
    }
    validate_toolchains(&recipe.toolchains)?;
    validate_profiles(&recipe.budget_profiles)?;
    let pins: BTreeSet<Toolchain> = recipe.toolchains.iter().map(|pin| pin.name).collect();
    let mut asset_ids = BTreeSet::new();
    for asset in &recipe.assets {
        if !valid_identifier(&asset.id) || !asset_ids.insert(asset.id.as_str()) {
            return Err(invalid(format!(
                "invalid or duplicate asset id {}",
                asset.id
            )));
        }
        validate_relative_path(&asset.input)?;
        validate_transform(&asset.transform, &pins)?;
    }
    Ok(())
}

fn validate_plan(plan: &AdaptivePlan) -> Result<(), AssetError> {
    if plan.schema != ADAPTIVE_PLAN_SCHEMA || plan.plan_version != ADAPTIVE_PLAN_VERSION {
        return Err(invalid("unsupported adaptive plan version"));
    }
    if !valid_sha256(&plan.recipe_sha256) {
        return Err(invalid(
            "plan recipeSha256 must be 64 hexadecimal characters",
        ));
    }
    validate_toolchains(&plan.toolchains)?;
    validate_profiles(&plan.budget_profiles)?;
    if plan.sources.is_empty() || plan.sources.len() > MAX_ASSETS || plan.jobs.len() > MAX_JOBS {
        return Err(invalid("plan size is outside supported bounds"));
    }
    let pins: BTreeSet<Toolchain> = plan.toolchains.iter().map(|pin| pin.name).collect();
    for source in &plan.sources {
        if !valid_identifier(&source.id)
            || !valid_sha256(&source.sha256)
            || source.bytes == 0
            || source.bytes > MAX_SOURCE_BYTES
        {
            return Err(invalid(format!("invalid plan source {}", source.id)));
        }
        validate_relative_path(&source.path)?;
    }
    let sources: BTreeSet<&str> = plan
        .sources
        .iter()
        .map(|source| source.id.as_str())
        .collect();
    if sources.len() != plan.sources.len() {
        return Err(invalid("plan contains duplicate sources"));
    }
    let mut jobs = BTreeSet::new();
    for job in &plan.jobs {
        if !valid_dotted_identifier(&job.id)
            || !valid_identifier(&job.source_id)
            || !valid_identifier(&job.variant_id)
            || !jobs.insert(job.id.as_str())
            || !sources.contains(job.source_id.as_str())
        {
            return Err(invalid(format!("invalid job identity {}", job.id)));
        }
        if job.id != format!("{}.{}", job.source_id, job.variant_id) {
            return Err(invalid(format!(
                "job {} identity does not match its source",
                job.id
            )));
        }
        validate_relative_path(&job.staging_path)?;
        let expected_stage = format!(".pliego-assets/stage/{}.{}", job.id, job.format.extension());
        if job.staging_path != expected_stage {
            return Err(invalid(format!("job {} has a forged staging path", job.id)));
        }
        if !job.required_toolchains.is_subset(&pins) {
            return Err(invalid(format!(
                "job {} requires an unpinned toolchain",
                job.id
            )));
        }
        validate_policy(&job.policy, operation_family(&job.operation))?;
        validate_job_contract(job)?;
    }
    Ok(())
}

fn validate_toolchains(toolchains: &[ToolchainPin]) -> Result<(), AssetError> {
    let mut seen = BTreeSet::new();
    for pin in toolchains {
        if !seen.insert(pin.name)
            || pin.version.trim().is_empty()
            || pin.version.len() > 80
            || !pin
                .version
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\'' | b'"' | b'`'))
        {
            return Err(invalid(
                "toolchain pins must be unique and have a bounded version",
            ));
        }
        if let Some(integrity) = &pin.integrity {
            let valid = integrity
                .strip_prefix("sha256-")
                .map(|digest| {
                    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .unwrap_or(false);
            if !valid {
                return Err(invalid(
                    "toolchain integrity must use sha256- plus 64 hex characters",
                ));
            }
        }
    }
    Ok(())
}

fn validate_profiles(profiles: &[DeviceBudgetProfile]) -> Result<(), AssetError> {
    if profiles.is_empty() {
        return Err(invalid("at least one device budget profile is required"));
    }
    let mut ids = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for profile in profiles {
        if !valid_identifier(&profile.id)
            || !ids.insert(profile.id.as_str())
            || profile.tiers.is_empty()
        {
            return Err(invalid("budget profile ids must be valid and unique"));
        }
        let mut tiers = BTreeSet::new();
        for budget in &profile.tiers {
            if !tiers.insert(budget.tier) {
                return Err(invalid(format!(
                    "profile {} duplicates a tier budget",
                    profile.id
                )));
            }
            covered.insert(budget.tier);
        }
    }
    let required = BTreeSet::from([Tier::Universal, Tier::Lite, Tier::Balanced, Tier::Signature]);
    if covered != required {
        return Err(invalid(
            "budget profiles must collectively cover all four tiers",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Family {
    Image,
    Video,
    Font,
    Scene,
}

fn validate_transform(
    transform: &TransformRecipe,
    pins: &BTreeSet<Toolchain>,
) -> Result<(), AssetError> {
    let mut ids = BTreeSet::new();
    match transform {
        TransformRecipe::Image { outputs } => {
            require_outputs(outputs.len())?;
            require_pin(pins, Toolchain::Ffmpeg, "image")?;
            for output in outputs {
                validate_output_id(&mut ids, &output.id)?;
                if output.width == 0 || output.height == 0 || !(1..=100).contains(&output.quality) {
                    return Err(invalid(
                        "image dimensions and quality must be positive and bounded",
                    ));
                }
                validate_policy(&output.policy, Family::Image)?;
            }
        }
        TransformRecipe::Video { outputs } => {
            require_outputs(outputs.len())?;
            require_pin(pins, Toolchain::Ffmpeg, "video")?;
            for output in outputs {
                validate_output_id(&mut ids, &output.id)?;
                if output.width == 0
                    || output.height == 0
                    || output.bitrate_kbps == 0
                    || output.max_fps == 0
                    || output.max_fps > 240
                {
                    return Err(invalid(
                        "video dimensions, bitrate, and fps are outside supported bounds",
                    ));
                }
                validate_policy(&output.policy, Family::Video)?;
            }
        }
        TransformRecipe::Font { outputs } => {
            require_outputs(outputs.len())?;
            require_pin(pins, Toolchain::Fonttools, "font")?;
            for output in outputs {
                validate_output_id(&mut ids, &output.id)?;
                if output.glyphs.is_empty() || output.glyphs.chars().count() > 8192 {
                    return Err(invalid("font glyph set must contain 1..=8192 characters"));
                }
                if output
                    .unicode_range
                    .as_ref()
                    .is_some_and(|value| value.len() > 512 || !value.starts_with("U+"))
                {
                    return Err(invalid("font unicodeRange must be a bounded CSS U+ range"));
                }
                validate_policy(&output.policy, Family::Font)?;
            }
        }
        TransformRecipe::Scene3d { outputs } => {
            require_outputs(outputs.len())?;
            require_pin(pins, Toolchain::GltfTransform, "3D")?;
            require_pin(pins, Toolchain::Ktx2Encoder, "3D")?;
            for output in outputs {
                validate_output_id(&mut ids, &output.id)?;
                if output.requires_blender {
                    require_pin(pins, Toolchain::Blender, "3D")?;
                }
                if output.max_triangles == 0
                    || output.max_draw_calls == 0
                    || output.max_texture_dimension == 0
                    || output.max_texture_dimension > 16_384
                {
                    return Err(invalid(
                        "3D geometry and texture limits must be positive and bounded",
                    ));
                }
                validate_policy(&output.policy, Family::Scene)?;
            }
        }
    }
    Ok(())
}

fn validate_policy(policy: &LoadPolicy, family: Family) -> Result<(), AssetError> {
    if policy.tiers.is_empty() {
        return Err(invalid("every output must target at least one tier"));
    }
    if policy.preload && policy.delivery != Delivery::Initial {
        return Err(invalid("only initial assets may preload"));
    }
    if policy.fetch_priority == FetchPriority::High && policy.delivery != Delivery::Initial {
        return Err(invalid(
            "high fetch priority is reserved for initial assets",
        ));
    }
    if matches!(family, Family::Video | Family::Scene) {
        if policy.tiers.contains(&Tier::Universal) {
            return Err(invalid("Universal tier cannot download video or 3D assets"));
        }
        if policy.motion != MotionPolicy::SuppressWhenReduced {
            return Err(invalid(
                "video and 3D outputs must be suppressed for reduced motion",
            ));
        }
    } else if policy.motion != MotionPolicy::Static {
        return Err(invalid(
            "image and font outputs must use the static motion policy",
        ));
    }
    Ok(())
}

fn validate_job_contract(job: &AssetJob) -> Result<(), AssetError> {
    let (format, tools, decoded, triangles, draw_calls) = match &job.operation {
        JobOperation::Image {
            width,
            height,
            format,
            quality,
        } => {
            if *width == 0 || *height == 0 || !(1..=100).contains(quality) {
                return Err(invalid(format!(
                    "job {} has invalid image parameters",
                    job.id
                )));
            }
            (
                image_artifact(*format),
                BTreeSet::from([Toolchain::Ffmpeg]),
                rgba_bytes(*width, *height)?,
                0,
                0,
            )
        }
        JobOperation::Video {
            width,
            height,
            format,
            bitrate_kbps,
            max_fps,
            ..
        } => {
            if *width == 0 || *height == 0 || *bitrate_kbps == 0 || *max_fps == 0 || *max_fps > 240
            {
                return Err(invalid(format!(
                    "job {} has invalid video parameters",
                    job.id
                )));
            }
            (
                video_artifact(*format),
                BTreeSet::from([Toolchain::Ffmpeg]),
                rgba_bytes(*width, *height)?
                    .checked_mul(3)
                    .ok_or_else(|| invalid("video decoded memory estimate overflow"))?,
                0,
                0,
            )
        }
        JobOperation::FontSubset {
            glyphs,
            unicode_range,
        } => {
            if glyphs.is_empty()
                || glyphs.chars().count() > 8192
                || unicode_range
                    .as_ref()
                    .is_some_and(|value| value.len() > 512 || !value.starts_with("U+"))
            {
                return Err(invalid(format!(
                    "job {} has invalid font parameters",
                    job.id
                )));
            }
            (
                ArtifactFormat::Woff2,
                BTreeSet::from([Toolchain::Fonttools]),
                0,
                0,
                0,
            )
        }
        JobOperation::Scene3d {
            max_triangles,
            max_draw_calls,
            max_texture_dimension,
            requires_blender,
            ..
        } => {
            if *max_triangles == 0
                || *max_draw_calls == 0
                || *max_texture_dimension == 0
                || *max_texture_dimension > 16_384
            {
                return Err(invalid(format!("job {} has invalid 3D parameters", job.id)));
            }
            let mut tools = BTreeSet::from([Toolchain::GltfTransform, Toolchain::Ktx2Encoder]);
            if *requires_blender {
                tools.insert(Toolchain::Blender);
            }
            (
                ArtifactFormat::Glb,
                tools,
                rgba_bytes(*max_texture_dimension, *max_texture_dimension)?,
                *max_triangles,
                *max_draw_calls,
            )
        }
    };
    let expected_selection_group = if matches!(&job.operation, JobOperation::FontSubset { .. }) {
        None
    } else {
        Some(job.source_id.clone())
    };
    if job.format != format
        || job.required_toolchains != tools
        || job.estimated_decoded_bytes != decoded
        || job.estimated_triangles != triangles
        || job.estimated_draw_calls != draw_calls
        || job.selection_group != expected_selection_group
    {
        return Err(invalid(format!(
            "job {} format, toolchains, or estimates do not match its operation",
            job.id
        )));
    }
    Ok(())
}

fn append_jobs(asset: &AssetRecipe, jobs: &mut Vec<AssetJob>) -> Result<(), AssetError> {
    match &asset.transform {
        TransformRecipe::Image { outputs } => {
            for output in outputs {
                let decoded = rgba_bytes(output.width, output.height)?;
                push_job(
                    jobs,
                    asset,
                    &output.id,
                    image_artifact(output.format),
                    BTreeSet::from([Toolchain::Ffmpeg]),
                    output.policy.clone(),
                    JobOperation::Image {
                        width: output.width,
                        height: output.height,
                        format: output.format,
                        quality: output.quality,
                    },
                    decoded,
                    0,
                    0,
                );
            }
        }
        TransformRecipe::Video { outputs } => {
            for output in outputs {
                let decoded = rgba_bytes(output.width, output.height)?
                    .checked_mul(3)
                    .ok_or_else(|| invalid("video decoded memory estimate overflow"))?;
                push_job(
                    jobs,
                    asset,
                    &output.id,
                    video_artifact(output.format),
                    BTreeSet::from([Toolchain::Ffmpeg]),
                    output.policy.clone(),
                    JobOperation::Video {
                        width: output.width,
                        height: output.height,
                        format: output.format,
                        bitrate_kbps: output.bitrate_kbps,
                        max_fps: output.max_fps,
                        audio: output.audio,
                    },
                    decoded,
                    0,
                    0,
                );
            }
        }
        TransformRecipe::Font { outputs } => {
            for output in outputs {
                push_job(
                    jobs,
                    asset,
                    &output.id,
                    ArtifactFormat::Woff2,
                    BTreeSet::from([Toolchain::Fonttools]),
                    output.policy.clone(),
                    JobOperation::FontSubset {
                        glyphs: output.glyphs.clone(),
                        unicode_range: output.unicode_range.clone(),
                    },
                    0,
                    0,
                    0,
                );
            }
        }
        TransformRecipe::Scene3d { outputs } => {
            for output in outputs {
                let mut tools = BTreeSet::from([Toolchain::GltfTransform, Toolchain::Ktx2Encoder]);
                if output.requires_blender {
                    tools.insert(Toolchain::Blender);
                }
                let decoded =
                    rgba_bytes(output.max_texture_dimension, output.max_texture_dimension)?;
                push_job(
                    jobs,
                    asset,
                    &output.id,
                    ArtifactFormat::Glb,
                    tools,
                    output.policy.clone(),
                    JobOperation::Scene3d {
                        max_triangles: output.max_triangles,
                        max_draw_calls: output.max_draw_calls,
                        mesh_compression: output.mesh_compression,
                        texture_compression: output.texture_compression,
                        max_texture_dimension: output.max_texture_dimension,
                        requires_blender: output.requires_blender,
                    },
                    decoded,
                    output.max_triangles,
                    output.max_draw_calls,
                );
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_job(
    jobs: &mut Vec<AssetJob>,
    asset: &AssetRecipe,
    variant_id: &str,
    format: ArtifactFormat,
    required_toolchains: BTreeSet<Toolchain>,
    policy: LoadPolicy,
    operation: JobOperation,
    estimated_decoded_bytes: u64,
    estimated_triangles: u64,
    estimated_draw_calls: u64,
) {
    let id = format!("{}.{}", asset.id, variant_id);
    let selection_group = if matches!(&operation, JobOperation::FontSubset { .. }) {
        None
    } else {
        Some(asset.id.clone())
    };
    jobs.push(AssetJob {
        staging_path: format!(".pliego-assets/stage/{id}.{}", format.extension()),
        id,
        source_id: asset.id.clone(),
        variant_id: variant_id.to_owned(),
        format,
        selection_group,
        required_toolchains,
        policy,
        operation,
        estimated_decoded_bytes,
        estimated_triangles,
        estimated_draw_calls,
    });
}

fn validate_magic(
    file: &mut fs::File,
    path: &Path,
    format: ArtifactFormat,
) -> Result<(), AssetError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut head = [0_u8; 32];
    let read = file.read(&mut head).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bytes = &head[..read];
    let valid = match format {
        ArtifactFormat::Avif => {
            bytes.len() >= 12
                && &bytes[4..8] == b"ftyp"
                && bytes[8..]
                    .windows(4)
                    .any(|brand| matches!(brand, b"avif" | b"avis"))
        }
        ArtifactFormat::Webp => {
            bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
        }
        ArtifactFormat::Jpeg => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        ArtifactFormat::Mp4 => bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
        ArtifactFormat::Webm => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        ArtifactFormat::Woff2 => bytes.starts_with(b"wOF2"),
        ArtifactFormat::Glb => {
            bytes.len() >= 12 && &bytes[..4] == b"glTF" && bytes[4..8] == 2_u32.to_le_bytes()
        }
    };
    if !valid {
        return Err(invalid(format!(
            "{} does not match declared {:?} format",
            path.display(),
            format
        )));
    }
    Ok(())
}

fn validate_artifact(file: &mut fs::File, path: &Path, job: &AssetJob) -> Result<(), AssetError> {
    validate_magic(file, path, job.format)?;
    match &job.operation {
        JobOperation::Video { format, .. } => {
            let matches_codec = match format {
                VideoFormat::Mp4H264 => {
                    file_contains(file, path, b"avc1")? || file_contains(file, path, b"avc3")?
                }
                VideoFormat::Mp4Av1 => file_contains(file, path, b"av01")?,
                VideoFormat::WebmVp9 => file_contains(file, path, b"V_VP9")?,
            };
            if !matches_codec {
                return Err(invalid(format!(
                    "{} does not advertise its declared {:?} codec",
                    path.display(),
                    format
                )));
            }
        }
        JobOperation::Scene3d {
            max_triangles,
            max_draw_calls,
            mesh_compression,
            texture_compression,
            ..
        } => validate_glb(
            file,
            path,
            *max_triangles,
            *max_draw_calls,
            *mesh_compression,
            *texture_compression,
        )?,
        JobOperation::Image { .. } | JobOperation::FontSubset { .. } => {}
    }
    Ok(())
}

fn validate_glb(
    file: &mut fs::File,
    path: &Path,
    max_triangles: u64,
    max_draw_calls: u64,
    mesh_compression: MeshCompression,
    _texture_compression: TextureCompression,
) -> Result<(), AssetError> {
    const MAX_GLTF_JSON_BYTES: u32 = 16 * 1024 * 1024;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut header = [0_u8; 20];
    file.read_exact(&mut header)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let declared_length = u64::from(u32::from_le_bytes(header[8..12].try_into().unwrap()));
    let json_length = u32::from_le_bytes(header[12..16].try_into().unwrap());
    if declared_length != metadata.len()
        || json_length == 0
        || json_length > MAX_GLTF_JSON_BYTES
        || &header[16..20] != b"JSON"
    {
        return Err(invalid(format!(
            "{} has an invalid GLB header",
            path.display()
        )));
    }
    let mut json = vec![0_u8; json_length as usize];
    file.read_exact(&mut json)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    while json.last().is_some_and(|byte| matches!(byte, b' ' | 0)) {
        json.pop();
    }
    let root: serde_json::Value = serde_json::from_slice(&json).map_err(|source| {
        invalid(format!(
            "{} contains invalid glTF JSON: {source}",
            path.display()
        ))
    })?;
    if root
        .pointer("/asset/version")
        .and_then(|value| value.as_str())
        != Some("2.0")
    {
        return Err(invalid(format!("{} is not glTF 2.0", path.display())));
    }
    for collection in ["buffers", "images"] {
        if root
            .get(collection)
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .any(|entry| entry.get("uri").is_some())
        {
            return Err(invalid(format!(
                "{} must be self-contained; external and data URIs are forbidden",
                path.display()
            )));
        }
    }
    let extensions: BTreeSet<&str> = root
        .get("extensionsUsed")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect();
    let mesh_extension = match mesh_compression {
        MeshCompression::None => None,
        MeshCompression::Meshopt => Some("EXT_meshopt_compression"),
        MeshCompression::Draco => Some("KHR_draco_mesh_compression"),
    };
    if mesh_extension.is_some_and(|extension| !extensions.contains(extension))
        || !extensions.contains("KHR_texture_basisu")
    {
        return Err(invalid(format!(
            "{} does not declare the planned mesh/KTX2 extensions",
            path.display()
        )));
    }

    let accessors = root
        .get("accessors")
        .and_then(|value| value.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut triangles = 0_u64;
    let mut draw_calls = 0_u64;
    for primitive in root
        .get("meshes")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|mesh| mesh.get("primitives").and_then(|value| value.as_array()))
        .flatten()
    {
        draw_calls = draw_calls
            .checked_add(1)
            .ok_or_else(|| invalid("GLB draw call count overflow"))?;
        let accessor_index = primitive
            .get("indices")
            .and_then(|value| value.as_u64())
            .or_else(|| {
                primitive
                    .pointer("/attributes/POSITION")
                    .and_then(|value| value.as_u64())
            });
        let count = accessor_index
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| accessors.get(index))
            .and_then(|accessor| accessor.get("count"))
            .and_then(|value| value.as_u64())
            .ok_or_else(|| invalid("GLB primitive has no bounded index/position accessor"))?;
        let primitive_triangles = match primitive
            .get("mode")
            .and_then(|value| value.as_u64())
            .unwrap_or(4)
        {
            4 => count / 3,
            5 | 6 => count.saturating_sub(2),
            other => return Err(invalid(format!("unsupported GLB primitive mode {other}"))),
        };
        triangles = triangles
            .checked_add(primitive_triangles)
            .ok_or_else(|| invalid("GLB triangle count overflow"))?;
    }
    if draw_calls == 0 || triangles > max_triangles || draw_calls > max_draw_calls {
        return Err(invalid(format!(
            "{} exceeds planned geometry: {triangles} triangles / {draw_calls} draw calls",
            path.display()
        )));
    }
    Ok(())
}

fn file_contains(file: &mut fs::File, path: &Path, needle: &[u8]) -> Result<bool, AssetError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut previous = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            return Ok(false);
        }
        previous.extend_from_slice(&buffer[..read]);
        if previous
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return Ok(true);
        }
        let keep = needle.len().saturating_sub(1).min(previous.len());
        previous.drain(..previous.len() - keep);
    }
}

fn validate_relative_path(value: &str) -> Result<PathBuf, AssetError> {
    if value.is_empty() || value.len() > 500 || value.contains('\\') || value.contains('\0') {
        return Err(invalid(
            "asset paths must be bounded, slash-separated relative paths",
        ));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(invalid("absolute asset paths are forbidden"));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => {
                let segment = segment
                    .to_str()
                    .ok_or_else(|| invalid("asset paths must be UTF-8"))?;
                if segment.is_empty() || segment == "." || windows_reserved(segment) {
                    return Err(invalid(format!("unsafe asset path segment {segment}")));
                }
                normalized.push(segment);
            }
            _ => {
                return Err(invalid(
                    "asset paths cannot contain roots, prefixes, or dot segments",
                ));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(invalid("asset path cannot be empty"));
    }
    Ok(normalized)
}

fn windows_reserved(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        || segment.ends_with(['.', ' '])
        || segment.contains(':')
}

fn reject_symlink_components(root: &Path, relative: &Path) -> Result<(), AssetError> {
    let mut current = root.to_path_buf();
    for segment in relative.components() {
        let Component::Normal(segment) = segment else {
            return Err(invalid("unsafe path component"));
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid(format!(
                    "symbolic links are forbidden in asset paths: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(AssetError::Io {
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn secure_create_directory(path: &Path, root: &Path) -> Result<(), AssetError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| invalid("publish directory escaped output root"))?;
    let mut current = root.to_path_buf();
    for segment in relative.components() {
        let Component::Normal(segment) = segment else {
            return Err(invalid("unsafe publish directory component"));
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid(format!(
                    "publish directory must not be a link or file: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|source| AssetError::Io {
                    path: current.clone(),
                    source,
                })?;
            }
            Err(source) => {
                return Err(AssetError::Io {
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn image_artifact(format: ImageFormat) -> ArtifactFormat {
    match format {
        ImageFormat::Avif => ArtifactFormat::Avif,
        ImageFormat::Webp => ArtifactFormat::Webp,
        ImageFormat::Jpeg => ArtifactFormat::Jpeg,
    }
}

fn video_artifact(format: VideoFormat) -> ArtifactFormat {
    match format {
        VideoFormat::Mp4H264 | VideoFormat::Mp4Av1 => ArtifactFormat::Mp4,
        VideoFormat::WebmVp9 => ArtifactFormat::Webm,
    }
}

fn operation_family(operation: &JobOperation) -> Family {
    match operation {
        JobOperation::Image { .. } => Family::Image,
        JobOperation::Video { .. } => Family::Video,
        JobOperation::FontSubset { .. } => Family::Font,
        JobOperation::Scene3d { .. } => Family::Scene,
    }
}

fn rgba_bytes(width: u32, height: u32) -> Result<u64, AssetError> {
    u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| invalid("decoded size overflow"))
}

fn sorted_toolchains(toolchains: &[ToolchainPin]) -> Vec<ToolchainPin> {
    let mut values = toolchains.to_vec();
    values.sort_by_key(|pin| pin.name);
    values
}

fn sorted_profiles(profiles: &[DeviceBudgetProfile]) -> Vec<DeviceBudgetProfile> {
    let mut values = profiles.to_vec();
    for profile in &mut values {
        profile.tiers.sort_by_key(|budget| budget.tier);
    }
    values.sort_by(|left, right| left.id.cmp(&right.id));
    values
}

fn pretty_json<T: Serialize>(value: &T) -> Result<Vec<u8>, AssetError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(AssetError::Json)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn slash_path(path: &Path) -> String {
    path.iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
fn invalid(message: impl Into<String>) -> AssetError {
    AssetError::InvalidRecipe(message.into())
}
fn require_outputs(count: usize) -> Result<(), AssetError> {
    if count == 0 || count > 128 {
        Err(invalid("each asset must define 1..=128 outputs"))
    } else {
        Ok(())
    }
}
fn require_pin(pins: &BTreeSet<Toolchain>, pin: Toolchain, family: &str) -> Result<(), AssetError> {
    if pins.contains(&pin) {
        Ok(())
    } else {
        Err(invalid(format!(
            "{family} outputs require a pinned {pin:?} toolchain"
        )))
    }
}
fn validate_output_id<'a>(ids: &mut BTreeSet<&'a str>, id: &'a str) -> Result<(), AssetError> {
    if valid_identifier(id) && ids.insert(id) {
        Ok(())
    } else {
        Err(invalid(format!("invalid or duplicate output id {id}")))
    }
}
fn valid_dotted_identifier(value: &str) -> bool {
    value.split('.').count() == 2 && value.split('.').all(valid_identifier)
}
fn add(total: &mut u64, value: u64) -> Result<(), AssetError> {
    *total = total
        .checked_add(value)
        .ok_or_else(|| invalid("budget total overflow"))?;
    Ok(())
}

fn accumulate_variant(
    totals: &mut BudgetTotals,
    variant: &PublishedVariant,
) -> Result<(), AssetError> {
    match variant.policy.delivery {
        Delivery::Initial => add(&mut totals.initial_bytes, variant.bytes)?,
        Delivery::Deferred => add(&mut totals.deferred_bytes, variant.bytes)?,
        Delivery::OnDemand => add(&mut totals.on_demand_bytes, variant.bytes)?,
    }
    add(&mut totals.decoded_bytes, variant.estimated_decoded_bytes)?;
    add(&mut totals.triangles, variant.estimated_triangles)?;
    add(&mut totals.draw_calls, variant.estimated_draw_calls)?;
    totals.preloads = totals
        .preloads
        .checked_add(u32::from(variant.policy.preload))
        .ok_or_else(|| invalid("preload count overflow"))?;
    Ok(())
}

fn add_totals(total: &mut BudgetTotals, value: &BudgetTotals) -> Result<(), AssetError> {
    add(&mut total.initial_bytes, value.initial_bytes)?;
    add(&mut total.deferred_bytes, value.deferred_bytes)?;
    add(&mut total.on_demand_bytes, value.on_demand_bytes)?;
    add(&mut total.decoded_bytes, value.decoded_bytes)?;
    add(&mut total.triangles, value.triangles)?;
    add(&mut total.draw_calls, value.draw_calls)?;
    total.preloads = total
        .preloads
        .checked_add(value.preloads)
        .ok_or_else(|| invalid("preload count overflow"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn mixed_media_plan_is_deterministic_and_toolchain_neutral() {
        let root = temp();
        fs::write(root.join("hero.png"), b"source").unwrap();
        let recipe = image_recipe();
        let first = plan_adaptive_assets(&recipe, &root).unwrap();
        let second = plan_adaptive_assets(&recipe, &root).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.jobs[0].staging_path,
            ".pliego-assets/stage/hero.small.avif"
        );
        assert_eq!(
            first.jobs[0].required_toolchains,
            BTreeSet::from([Toolchain::Ffmpeg])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_media_family_emits_a_typed_external_job() {
        let root = temp();
        for source in ["hero.png", "film.mov", "display.ttf", "scene.blend"] {
            fs::write(root.join(source), format!("source:{source}")).unwrap();
        }
        let mut recipe = image_recipe();
        recipe.toolchains = vec![
            pin(Toolchain::Ffmpeg),
            pin(Toolchain::Fonttools),
            pin(Toolchain::Blender),
            pin(Toolchain::GltfTransform),
            pin(Toolchain::Ktx2Encoder),
        ];
        let motion = LoadPolicy {
            tiers: BTreeSet::from([Tier::Balanced, Tier::Signature]),
            delivery: Delivery::Deferred,
            preload: false,
            fetch_priority: FetchPriority::Low,
            motion: MotionPolicy::SuppressWhenReduced,
        };
        recipe.assets.extend([
            AssetRecipe {
                id: "film".to_owned(),
                input: "film.mov".to_owned(),
                transform: TransformRecipe::Video {
                    outputs: vec![VideoOutput {
                        id: "mobile".to_owned(),
                        width: 640,
                        height: 360,
                        format: VideoFormat::Mp4H264,
                        bitrate_kbps: 800,
                        max_fps: 30,
                        audio: false,
                        policy: motion.clone(),
                    }],
                },
            },
            AssetRecipe {
                id: "display-font".to_owned(),
                input: "display.ttf".to_owned(),
                transform: TransformRecipe::Font {
                    outputs: vec![FontOutput {
                        id: "latin".to_owned(),
                        glyphs: "PliegoRS".to_owned(),
                        unicode_range: Some("U+0000-00FF".to_owned()),
                        policy: LoadPolicy {
                            tiers: BTreeSet::from([Tier::Universal]),
                            delivery: Delivery::Initial,
                            preload: true,
                            fetch_priority: FetchPriority::High,
                            motion: MotionPolicy::Static,
                        },
                    }],
                },
            },
            AssetRecipe {
                id: "gallery-scene".to_owned(),
                input: "scene.blend".to_owned(),
                transform: TransformRecipe::Scene3d {
                    outputs: vec![SceneOutput {
                        id: "lod-lite".to_owned(),
                        max_triangles: 20_000,
                        max_draw_calls: 24,
                        mesh_compression: MeshCompression::Meshopt,
                        texture_compression: TextureCompression::Ktx2Etc1s,
                        max_texture_dimension: 1024,
                        requires_blender: true,
                        policy: motion,
                    }],
                },
            },
        ]);
        let plan = plan_adaptive_assets(&recipe, &root).unwrap();
        assert_eq!(plan.jobs.len(), 4);
        assert!(
            plan.jobs
                .iter()
                .any(|job| job.format == ArtifactFormat::Mp4)
        );
        assert!(
            plan.jobs
                .iter()
                .any(|job| job.format == ArtifactFormat::Woff2)
        );
        let scene = plan
            .jobs
            .iter()
            .find(|job| job.format == ArtifactFormat::Glb)
            .unwrap();
        assert_eq!(
            scene.required_toolchains,
            BTreeSet::from([
                Toolchain::Blender,
                Toolchain::GltfTransform,
                Toolchain::Ktx2Encoder
            ])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finalizer_rejects_disguised_artifacts_before_publish() {
        let source = temp();
        fs::write(source.join("hero.png"), b"source").unwrap();
        let plan = plan_adaptive_assets(&image_recipe(), &source).unwrap();
        let output = temp();
        let staged = output.join(&plan.jobs[0].staging_path);
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::write(&staged, b"not really avif").unwrap();
        let error = finalize_adaptive_assets(&plan, &output)
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not match declared"));
        assert!(!output.join("assets").exists());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn finalizer_content_addresses_valid_artifacts_and_enforces_budgets() {
        let source = temp();
        fs::write(source.join("hero.png"), b"source").unwrap();
        let mut recipe = image_recipe();
        recipe.budget_profiles[0].tiers[0].max_initial_bytes = 64;
        let plan = plan_adaptive_assets(&recipe, &source).unwrap();
        let output = temp();
        let staged = output.join(&plan.jobs[0].staging_path);
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        let mut avif = vec![0, 0, 0, 20];
        avif.extend_from_slice(b"ftypavif");
        avif.extend_from_slice(b"payload");
        fs::write(&staged, avif).unwrap();
        let manifest = finalize_adaptive_assets(&plan, &output).unwrap();
        assert_eq!(manifest.variants.len(), 1);
        assert!(
            manifest.variants[0]
                .path
                .contains(&manifest.variants[0].sha256[..16])
        );
        assert!(output.join(&manifest.variants[0].path).is_file());
        assert!(manifest.budget_results.iter().all(|result| result.pass));
        let delivery = manifest.delivery(RuntimeConstraints {
            tier: Tier::Universal,
            reduced_motion: false,
            save_data: true,
        });
        assert_eq!(delivery.len(), 1);
        assert_eq!(delivery[0].strategy, LoadStrategy::Eager);
        assert!(delivery[0].preload);
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn budget_failure_is_transactional() {
        let source = temp();
        fs::write(source.join("hero.png"), b"source").unwrap();
        let mut recipe = image_recipe();
        recipe.budget_profiles[0].tiers[0].max_initial_bytes = 1;
        let plan = plan_adaptive_assets(&recipe, &source).unwrap();
        let output = temp();
        let staged = output.join(&plan.jobs[0].staging_path);
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::write(&staged, b"\0\0\0\x14ftypavifpayload").unwrap();
        assert!(finalize_adaptive_assets(&plan, &output).is_err());
        assert!(staged.is_file());
        assert!(!output.join("assets").exists());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn codec_and_size_alternatives_use_worst_case_instead_of_sum() {
        let source = temp();
        fs::write(source.join("hero.png"), b"source").unwrap();
        let mut recipe = image_recipe();
        let TransformRecipe::Image { outputs } = &mut recipe.assets[0].transform else {
            unreachable!();
        };
        let mut larger = outputs[0].clone();
        larger.id = "large".to_owned();
        larger.width = 4;
        larger.height = 4;
        larger.format = ImageFormat::Webp;
        outputs.push(larger);
        recipe.budget_profiles[0].tiers[0].max_decoded_bytes = 64;
        recipe.budget_profiles[0].tiers[0].max_initial_bytes = 64;
        let plan = plan_adaptive_assets(&recipe, &source).unwrap();
        let output = temp();
        for job in &plan.jobs {
            let staged = output.join(&job.staging_path);
            fs::create_dir_all(staged.parent().unwrap()).unwrap();
            let bytes: &[u8] = match job.format {
                ArtifactFormat::Avif => b"\0\0\0\x14ftypavifpayload",
                ArtifactFormat::Webp => b"RIFF0000WEBPpayload",
                _ => unreachable!(),
            };
            fs::write(staged, bytes).unwrap();
        }
        let manifest = finalize_adaptive_assets(&plan, &output).unwrap();
        let universal = manifest
            .budget_results
            .iter()
            .find(|result| result.tier == Tier::Universal)
            .unwrap();
        assert_eq!(universal.totals.decoded_bytes, 64);
        assert_eq!(universal.totals.preloads, 1);
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn path_traversal_reserved_names_and_unknown_fields_fail_closed() {
        let root = temp();
        fs::write(root.join("hero.png"), b"source").unwrap();
        for path in [
            "../secret",
            "/absolute",
            "C:/windows",
            "folder\\file",
            "NUL.txt",
        ] {
            let mut recipe = image_recipe();
            recipe.assets[0].input = path.to_owned();
            assert!(
                plan_adaptive_assets(&recipe, &root).is_err(),
                "accepted {path}"
            );
        }
        let json = br#"{"recipeVersion":"1.0.0","toolchains":[],"budgetProfiles":[],"assets":[],"surprise":true}"#;
        assert!(AdaptiveRecipe::from_json(json).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn aggregate_artifact_limit_rejects_multi_file_resource_exhaustion() {
        assert_eq!(
            checked_artifact_total(MAX_RECIPE_ARTIFACT_BYTES - 1, 1).unwrap(),
            MAX_RECIPE_ARTIFACT_BYTES
        );
        let error = checked_artifact_total(MAX_RECIPE_ARTIFACT_BYTES, 1)
            .expect_err("aggregate bytes above the global ceiling must fail");
        assert!(error.to_string().contains("aggregate bytes"));
        assert!(checked_artifact_total(u64::MAX, 1).is_err());
    }

    #[test]
    fn universal_motion_and_unsafe_preload_policies_are_rejected() {
        let root = temp();
        fs::write(root.join("clip.mp4"), b"source").unwrap();
        let mut recipe = image_recipe();
        recipe.assets[0].input = "clip.mp4".to_owned();
        recipe.assets[0].transform = TransformRecipe::Video {
            outputs: vec![VideoOutput {
                id: "mobile".to_owned(),
                width: 640,
                height: 360,
                format: VideoFormat::Mp4H264,
                bitrate_kbps: 800,
                max_fps: 30,
                audio: false,
                policy: LoadPolicy {
                    tiers: BTreeSet::from([Tier::Universal]),
                    delivery: Delivery::Deferred,
                    preload: true,
                    fetch_priority: FetchPriority::High,
                    motion: MotionPolicy::Static,
                },
            }],
        };
        assert!(plan_adaptive_assets(&recipe, &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forged_plan_format_paths_and_estimates_fail_closed() {
        let root = temp();
        fs::write(root.join("hero.png"), b"source").unwrap();
        let plan = plan_adaptive_assets(&image_recipe(), &root).unwrap();

        let mut forged = plan.clone();
        forged.jobs[0].format = ArtifactFormat::Glb;
        assert!(validate_plan(&forged).is_err());

        let mut forged = plan.clone();
        forged.jobs[0].staging_path = "../escape.avif".to_owned();
        assert!(validate_plan(&forged).is_err());

        let mut forged = plan;
        forged.jobs[0].estimated_decoded_bytes = 0;
        assert!(validate_plan(&forged).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn glb_validation_rejects_external_uris_and_observes_geometry() {
        let root = temp();
        let valid = root.join("valid.glb");
        fs::write(
            &valid,
            glb(br#"{"asset":{"version":"2.0"},"extensionsUsed":["EXT_meshopt_compression","KHR_texture_basisu"],"accessors":[{"count":6}],"meshes":[{"primitives":[{"indices":0}]}]}"#),
        )
        .unwrap();
        let mut valid_file = open_regular_nofollow(&valid).unwrap();
        validate_glb(
            &mut valid_file,
            &valid,
            2,
            1,
            MeshCompression::Meshopt,
            TextureCompression::Ktx2Etc1s,
        )
        .unwrap();

        let external = root.join("external.glb");
        fs::write(
            &external,
            glb(br#"{"asset":{"version":"2.0"},"extensionsUsed":["EXT_meshopt_compression","KHR_texture_basisu"],"images":[{"uri":"https://attacker.invalid/texture.ktx2"}],"accessors":[{"count":6}],"meshes":[{"primitives":[{"indices":0}]}]}"#),
        )
        .unwrap();
        let mut external_file = open_regular_nofollow(&external).unwrap();
        assert!(
            validate_glb(
                &mut external_file,
                &external,
                2,
                1,
                MeshCompression::Meshopt,
                TextureCompression::Ktx2Etc1s,
            )
            .unwrap_err()
            .to_string()
            .contains("self-contained")
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn image_recipe() -> AdaptiveRecipe {
        AdaptiveRecipe {
            schema: Some(ADAPTIVE_RECIPE_SCHEMA.to_owned()),
            recipe_version: ADAPTIVE_RECIPE_VERSION.to_owned(),
            toolchains: vec![ToolchainPin {
                name: Toolchain::Ffmpeg,
                version: "7.1.1".to_owned(),
                integrity: None,
            }],
            budget_profiles: vec![DeviceBudgetProfile {
                id: "modest-mobile".to_owned(),
                device: DeviceClass::ModestMobile,
                tiers: vec![
                    budget(Tier::Universal),
                    budget(Tier::Lite),
                    budget(Tier::Balanced),
                    budget(Tier::Signature),
                ],
            }],
            assets: vec![AssetRecipe {
                id: "hero".to_owned(),
                input: "hero.png".to_owned(),
                transform: TransformRecipe::Image {
                    outputs: vec![ImageOutput {
                        id: "small".to_owned(),
                        width: 2,
                        height: 2,
                        format: ImageFormat::Avif,
                        quality: 70,
                        policy: LoadPolicy {
                            tiers: BTreeSet::from([Tier::Universal]),
                            delivery: Delivery::Initial,
                            preload: true,
                            fetch_priority: FetchPriority::High,
                            motion: MotionPolicy::Static,
                        },
                    }],
                },
            }],
        }
    }

    fn budget(tier: Tier) -> TierBudget {
        TierBudget {
            tier,
            max_initial_bytes: 1024,
            max_deferred_bytes: 1024,
            max_on_demand_bytes: 1024,
            max_decoded_bytes: 1024,
            max_triangles: 0,
            max_draw_calls: 0,
            max_preloads: 1,
        }
    }
    fn pin(name: Toolchain) -> ToolchainPin {
        ToolchainPin {
            name,
            version: "test-1.0".to_owned(),
            integrity: None,
        }
    }
    fn glb(json: &[u8]) -> Vec<u8> {
        let mut json = json.to_vec();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let total = 12 + 8 + json.len();
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(b"glTF");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&(total as u32).to_le_bytes());
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"JSON");
        bytes.extend_from_slice(&json);
        bytes
    }
    fn temp() -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pliego-adaptive-test-{}-{id}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        path
    }
}
