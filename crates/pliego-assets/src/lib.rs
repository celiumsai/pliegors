//! Deterministic responsive raster orchestration for PliegoRS.
//!
//! Encoding is delegated to a pinned external backend. This crate owns recipes,
//! deterministic ordering, content-addressed names, hashes, and build ledgers.

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

mod adaptive;

pub use adaptive::*;

pub const REPORT_VERSION: &str = "1.0.0";
const MAX_RASTER_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RASTER_VARIANT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RASTER_TOTAL_OUTPUT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_RASTER_WIDTHS: usize = 32;
const MAX_RASTER_VARIANTS: usize = 96;
const MAX_RASTER_DIMENSION: u32 = 16_384;
const MAX_RASTER_PIXELS: u64 = 64 * 1024 * 1024;
static STAGE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum AssetError {
    InvalidRecipe(String),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Backend(String),
    Json(serde_json::Error),
}

impl fmt::Display for AssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRecipe(message) | Self::Backend(message) => formatter.write_str(message),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json(source) => write!(formatter, "cannot serialize asset ledger: {source}"),
        }
    }
}

impl std::error::Error for AssetError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RasterFormat {
    Avif,
    Webp,
    Jpeg,
}

impl RasterFormat {
    pub const ALL: [Self; 3] = [Self::Avif, Self::Webp, Self::Jpeg];

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Avif => "avif",
            Self::Webp => "webp",
            Self::Jpeg => "jpg",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Avif => "image/avif",
            Self::Webp => "image/webp",
            Self::Jpeg => "image/jpeg",
        }
    }
}

impl std::str::FromStr for RasterFormat {
    type Err = AssetError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "avif" => Ok(Self::Avif),
            "webp" => Ok(Self::Webp),
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            other => Err(AssetError::InvalidRecipe(format!(
                "unsupported raster format {other}; expected avif, webp, or jpeg"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RasterRecipe {
    pub asset_id: String,
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub widths: Vec<u32>,
    pub formats: Vec<RasterFormat>,
    pub quality: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendDescriptor {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDescriptor {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub dimensions: Dimensions,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeDescriptor {
    pub asset_id: String,
    pub widths: Vec<u32>,
    pub formats: Vec<RasterFormat>,
    pub quality: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedVariant {
    pub id: String,
    pub path: String,
    pub media_type: String,
    pub format: RasterFormat,
    pub sha256: String,
    pub bytes: u64,
    pub dimensions: Dimensions,
    pub estimated_vram_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RasterBuildReport {
    pub report_version: &'static str,
    pub backend: BackendDescriptor,
    pub source: SourceDescriptor,
    pub recipe: RecipeDescriptor,
    pub variants: Vec<GeneratedVariant>,
}

impl RasterBuildReport {
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, AssetError> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(AssetError::Json)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

pub trait RasterBackend {
    fn descriptor(&self) -> BackendDescriptor;
    fn dimensions(&self, input: &Path) -> Result<Dimensions, AssetError>;
    fn encode(
        &self,
        input: &Path,
        output: &Path,
        width: u32,
        format: RasterFormat,
        quality: u8,
    ) -> Result<Dimensions, AssetError>;
}

pub fn build_raster<B: RasterBackend>(
    recipe: &RasterRecipe,
    backend: &B,
) -> Result<RasterBuildReport, AssetError> {
    validate_recipe(recipe)?;
    let (source_bytes, source_sha256) =
        bounded_file_fingerprint(&recipe.input, MAX_RASTER_SOURCE_BYTES)?;
    let source_dimensions = backend.dimensions(&recipe.input)?;
    validate_raster_dimensions(source_dimensions, "source")?;
    fs::create_dir_all(&recipe.output_dir).map_err(|source| AssetError::Io {
        path: recipe.output_dir.clone(),
        source,
    })?;

    let widths = normalized_widths(&recipe.widths, source_dimensions.width);
    let formats: BTreeSet<RasterFormat> = recipe.formats.iter().copied().collect();
    let variant_count = widths
        .len()
        .checked_mul(formats.len())
        .ok_or_else(|| AssetError::InvalidRecipe("raster variant count overflow".to_owned()))?;
    if variant_count > MAX_RASTER_VARIANTS {
        return Err(AssetError::InvalidRecipe(format!(
            "raster recipe expands to {variant_count} variants; maximum is {MAX_RASTER_VARIANTS}"
        )));
    }
    let stage_id = STAGE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stage_dir = recipe.output_dir.join(format!(
        ".pliego-assets-stage-{}-{stage_id}",
        std::process::id()
    ));
    fs::create_dir(&stage_dir).map_err(|source| AssetError::Io {
        path: stage_dir.clone(),
        source,
    })?;

    let result = (|| {
        let mut variants = Vec::new();
        let mut total_output_bytes = 0_u64;
        for width in &widths {
            for format in &formats {
                let temporary = stage_dir.join(format!(
                    "{}.w{}.{}",
                    recipe.asset_id,
                    width,
                    format.extension()
                ));
                let dimensions =
                    backend.encode(&recipe.input, &temporary, *width, *format, recipe.quality)?;
                reject_symlink(&temporary)?;
                validate_raster_dimensions(dimensions, "encoded variant")?;
                let (bytes, sha256) =
                    bounded_file_fingerprint(&temporary, MAX_RASTER_VARIANT_BYTES)?;
                if bytes == 0 {
                    return Err(AssetError::Backend(format!(
                        "backend emitted an empty variant: {}",
                        temporary.display()
                    )));
                }
                if dimensions.width != *width || dimensions.width > source_dimensions.width {
                    return Err(AssetError::Backend(format!(
                        "backend emitted invalid dimensions {}x{} for requested width {width}",
                        dimensions.width, dimensions.height
                    )));
                }
                total_output_bytes = total_output_bytes.checked_add(bytes).ok_or_else(|| {
                    AssetError::Backend("raster output byte total overflow".to_owned())
                })?;
                if total_output_bytes > MAX_RASTER_TOTAL_OUTPUT_BYTES {
                    return Err(AssetError::Backend(format!(
                        "raster outputs exceed {MAX_RASTER_TOTAL_OUTPUT_BYTES} aggregate bytes"
                    )));
                }
                let file_name = format!(
                    "{}.w{}.{}.{}",
                    recipe.asset_id,
                    dimensions.width,
                    &sha256[..16],
                    format.extension()
                );
                let destination = recipe.output_dir.join(&file_name);
                publish_content_addressed(&temporary, &destination, &sha256)?;
                variants.push(GeneratedVariant {
                    id: format!(
                        "{}-w{}-{}",
                        recipe.asset_id,
                        dimensions.width,
                        format.extension()
                    ),
                    path: file_name,
                    media_type: format.media_type().to_owned(),
                    format: *format,
                    sha256,
                    bytes,
                    dimensions,
                    estimated_vram_bytes: decoded_rgba_bytes(dimensions)?,
                });
            }
        }
        Ok(variants)
    })();
    let cleanup = fs::remove_dir_all(&stage_dir);
    if let Err(source) = cleanup {
        if result.is_ok() {
            return Err(AssetError::Io {
                path: stage_dir,
                source,
            });
        }
    }
    let variants = result?;

    Ok(RasterBuildReport {
        report_version: REPORT_VERSION,
        backend: backend.descriptor(),
        source: SourceDescriptor {
            file_name: recipe
                .input
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| {
                    AssetError::InvalidRecipe("input must have a UTF-8 file name".to_owned())
                })?
                .to_owned(),
            sha256: source_sha256,
            bytes: source_bytes,
            dimensions: source_dimensions,
        },
        recipe: RecipeDescriptor {
            asset_id: recipe.asset_id.clone(),
            widths,
            formats: formats.into_iter().collect(),
            quality: recipe.quality,
        },
        variants,
    })
}

fn validate_recipe(recipe: &RasterRecipe) -> Result<(), AssetError> {
    if !valid_identifier(&recipe.asset_id) {
        return Err(AssetError::InvalidRecipe(
            "asset id must be lowercase kebab-case ASCII".to_owned(),
        ));
    }
    if !recipe.input.is_file() {
        return Err(AssetError::InvalidRecipe(format!(
            "input is not a file: {}",
            recipe.input.display()
        )));
    }
    if recipe.widths.is_empty()
        || recipe.widths.len() > MAX_RASTER_WIDTHS
        || recipe
            .widths
            .iter()
            .any(|width| *width == 0 || *width > MAX_RASTER_DIMENSION)
    {
        return Err(AssetError::InvalidRecipe(format!(
            "raster widths must contain 1..={MAX_RASTER_WIDTHS} values in 1..={MAX_RASTER_DIMENSION}"
        )));
    }
    if recipe.formats.is_empty() || recipe.formats.len() > RasterFormat::ALL.len() {
        return Err(AssetError::InvalidRecipe(
            "raster formats must contain between one and three entries".to_owned(),
        ));
    }
    if !(1..=100).contains(&recipe.quality) {
        return Err(AssetError::InvalidRecipe(
            "quality must be between 1 and 100".to_owned(),
        ));
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'-' => index > 0 && index + 1 < value.len(),
            _ => false,
        })
        && !value.contains("--")
}

fn normalized_widths(requested: &[u32], source_width: u32) -> Vec<u32> {
    requested
        .iter()
        .map(|width| (*width).min(source_width))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_raster_dimensions(dimensions: Dimensions, label: &str) -> Result<(), AssetError> {
    let pixels = u64::from(dimensions.width)
        .checked_mul(u64::from(dimensions.height))
        .ok_or_else(|| AssetError::Backend(format!("{label} pixel count overflow")))?;
    if dimensions.width == 0
        || dimensions.height == 0
        || dimensions.width > MAX_RASTER_DIMENSION
        || dimensions.height > MAX_RASTER_DIMENSION
        || pixels > MAX_RASTER_PIXELS
    {
        return Err(AssetError::Backend(format!(
            "{label} dimensions {}x{} exceed the raster limits",
            dimensions.width, dimensions.height
        )));
    }
    Ok(())
}

fn publish_content_addressed(
    temporary: &Path,
    destination: &Path,
    expected_sha256: &str,
) -> Result<(), AssetError> {
    if destination.exists() {
        reject_symlink(destination)?;
        let (_, existing_sha256) = bounded_file_fingerprint(destination, MAX_RASTER_VARIANT_BYTES)?;
        if existing_sha256 != expected_sha256 {
            return Err(AssetError::Backend(format!(
                "content-address collision at {}",
                destination.display()
            )));
        }
        fs::remove_file(temporary).map_err(|source| AssetError::Io {
            path: temporary.to_path_buf(),
            source,
        })?;
        return Ok(());
    }
    publish_no_clobber(temporary, destination, expected_sha256)
}

fn publish_no_clobber(
    temporary: &Path,
    destination: &Path,
    expected_sha256: &str,
) -> Result<(), AssetError> {
    let mut source_file = open_regular_nofollow(temporary)?;
    publish_open_file_no_clobber(&mut source_file, temporary, destination, expected_sha256)
}

fn publish_open_file_no_clobber(
    source_file: &mut fs::File,
    source_path: &Path,
    destination: &Path,
    expected_sha256: &str,
) -> Result<(), AssetError> {
    let mut destination_file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(destination)
    {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            reject_symlink(destination)?;
            if sha256_file(destination)? != expected_sha256 {
                return Err(AssetError::Backend(format!(
                    "content-address collision at {}",
                    destination.display()
                )));
            }
            fs::remove_file(source_path).map_err(|source| AssetError::Io {
                path: source_path.to_path_buf(),
                source,
            })?;
            return Ok(());
        }
        Err(source) => {
            return Err(AssetError::Io {
                path: destination.to_path_buf(),
                source,
            });
        }
    };
    source_file
        .seek(SeekFrom::Start(0))
        .map_err(|source| AssetError::Io {
            path: source_path.to_path_buf(),
            source,
        })?;
    if let Err(source) =
        io::copy(source_file, &mut destination_file).and_then(|_| destination_file.sync_all())
    {
        drop(destination_file);
        let _ = fs::remove_file(destination);
        return Err(AssetError::Io {
            path: destination.to_path_buf(),
            source,
        });
    }
    if hash_open_file(&mut destination_file, destination)? != expected_sha256 {
        drop(destination_file);
        let _ = fs::remove_file(destination);
        return Err(AssetError::Backend(format!(
            "asset source changed during publication: {}",
            source_path.display()
        )));
    }
    drop(destination_file);
    fs::remove_file(source_path).map_err(|source| AssetError::Io {
        path: source_path.to_path_buf(),
        source,
    })
}

fn sha256_file(path: &Path) -> Result<String, AssetError> {
    let mut file = open_regular_nofollow(path)?;
    hash_open_file(&mut file, path)
}

fn hash_open_file(file: &mut fs::File, path: &Path) -> Result<String, AssetError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn bounded_file_fingerprint(path: &Path, limit: u64) -> Result<(u64, String), AssetError> {
    let file = open_regular_nofollow(path)?;
    let metadata = file.metadata().map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        return Err(AssetError::InvalidRecipe(format!(
            "{} is {} bytes; maximum is {limit}",
            path.display(),
            metadata.len()
        )));
    }
    let mut file = file.take(limit.saturating_add(1));
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .ok_or_else(|| AssetError::Backend("raster byte count overflow".to_owned()))?;
        if bytes > limit {
            return Err(AssetError::InvalidRecipe(format!(
                "{} grew beyond the {limit}-byte limit while reading",
                path.display()
            )));
        }
        digest.update(&buffer[..read]);
    }
    Ok((bytes, format!("{:x}", digest.finalize())))
}

fn open_regular_nofollow(path: &Path) -> Result<fs::File, AssetError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| AssetError::InvalidRecipe(format!("{} has no file name", path.display())))?;
    let directory =
        Dir::open_ambient_dir(parent, ambient_authority()).map_err(|source| AssetError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(name, &options)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let metadata = file.metadata().map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(AssetError::InvalidRecipe(format!(
            "expected a regular file, not a link or device: {}",
            path.display()
        )));
    }
    Ok(file.into_std())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn reject_symlink(path: &Path) -> Result<(), AssetError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AssetError::Backend(format!(
            "asset output must be a regular file, not a link or device: {}",
            path.display()
        )));
    }
    Ok(())
}

fn decoded_rgba_bytes(dimensions: Dimensions) -> Result<u64, AssetError> {
    u64::from(dimensions.width)
        .checked_mul(u64::from(dimensions.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AssetError::Backend("decoded raster dimensions overflow u64".to_owned()))
}

#[derive(Clone, Debug)]
pub struct FfmpegBackend {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    descriptor: BackendDescriptor,
}

impl FfmpegBackend {
    pub fn discover(
        ffmpeg: impl Into<PathBuf>,
        ffprobe: impl Into<PathBuf>,
    ) -> Result<Self, AssetError> {
        let ffmpeg = ffmpeg.into();
        let ffprobe = ffprobe.into();
        let output = run_command(&ffmpeg, [OsString::from("-version")])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let first_line = stdout.lines().next().unwrap_or_default();
        let version = first_line
            .strip_prefix("ffmpeg version ")
            .and_then(|rest| rest.split_whitespace().next())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AssetError::Backend("cannot parse ffmpeg version".to_owned()))?;
        let backend = Self {
            ffmpeg,
            ffprobe,
            descriptor: BackendDescriptor {
                name: "ffmpeg".to_owned(),
                version: version.to_owned(),
            },
        };
        backend.dimensions_probe_available()?;
        Ok(backend)
    }

    fn dimensions_probe_available(&self) -> Result<(), AssetError> {
        run_command(&self.ffprobe, [OsString::from("-version")]).map(|_| ())
    }

    fn encode_arguments(
        input: &Path,
        output: &Path,
        width: u32,
        format: RasterFormat,
        quality: u8,
    ) -> Vec<OsString> {
        let mut arguments = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-nostdin".into(),
            "-y".into(),
            "-fflags".into(),
            "+bitexact".into(),
            "-i".into(),
            input.as_os_str().to_owned(),
            "-map_metadata".into(),
            "-1".into(),
            "-frames:v".into(),
            "1".into(),
            "-an".into(),
            "-threads".into(),
            "1".into(),
            "-vf".into(),
            format!("scale={width}:-2:flags=lanczos").into(),
            "-flags:v".into(),
            "+bitexact".into(),
        ];
        match format {
            RasterFormat::Avif => arguments.extend([
                "-c:v".into(),
                "libaom-av1".into(),
                "-still-picture".into(),
                "1".into(),
                "-cpu-used".into(),
                "4".into(),
                "-crf".into(),
                avif_crf(quality).to_string().into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]),
            RasterFormat::Webp => arguments.extend([
                "-c:v".into(),
                "libwebp".into(),
                "-quality".into(),
                quality.to_string().into(),
                "-compression_level".into(),
                "6".into(),
                "-preset".into(),
                "picture".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ]),
            RasterFormat::Jpeg => arguments.extend([
                "-c:v".into(),
                "mjpeg".into(),
                "-q:v".into(),
                jpeg_qscale(quality).to_string().into(),
                "-pix_fmt".into(),
                "yuvj420p".into(),
            ]),
        }
        arguments.push(output.as_os_str().to_owned());
        arguments
    }
}

impl RasterBackend for FfmpegBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }

    fn dimensions(&self, input: &Path) -> Result<Dimensions, AssetError> {
        let output = run_command(
            &self.ffprobe,
            [
                OsString::from("-v"),
                OsString::from("error"),
                OsString::from("-select_streams"),
                OsString::from("v:0"),
                OsString::from("-show_entries"),
                OsString::from("stream=width,height"),
                OsString::from("-of"),
                OsString::from("json"),
                input.as_os_str().to_owned(),
            ],
        )?;
        #[derive(Deserialize)]
        struct Probe {
            streams: Vec<Dimensions>,
        }
        let probe: Probe = serde_json::from_slice(&output.stdout).map_err(|source| {
            AssetError::Backend(format!("cannot parse ffprobe dimensions: {source}"))
        })?;
        probe
            .streams
            .into_iter()
            .next()
            .ok_or_else(|| AssetError::Backend("ffprobe found no video stream".to_owned()))
    }

    fn encode(
        &self,
        input: &Path,
        output: &Path,
        width: u32,
        format: RasterFormat,
        quality: u8,
    ) -> Result<Dimensions, AssetError> {
        run_command(
            &self.ffmpeg,
            Self::encode_arguments(input, output, width, format, quality),
        )?;
        self.dimensions(output)
    }
}

fn run_command<I, S>(program: &Path, arguments: I) -> Result<Output, AssetError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    const MAX_COMMAND_OUTPUT: u64 = 1024 * 1024;
    use std::io::Read;

    let mut child = Command::new(program)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| AssetError::Io {
            path: program.to_path_buf(),
            source,
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AssetError::Backend("cannot capture backend stdout".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AssetError::Backend("cannot capture backend stderr".to_owned()))?;
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_COMMAND_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .take(MAX_COMMAND_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = child.wait().map_err(|source| AssetError::Io {
        path: program.to_path_buf(),
        source,
    })?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| AssetError::Backend("backend stdout reader panicked".to_owned()))?
        .map_err(|source| AssetError::Io {
            path: program.to_path_buf(),
            source,
        })?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| AssetError::Backend("backend stderr reader panicked".to_owned()))?
        .map_err(|source| AssetError::Io {
            path: program.to_path_buf(),
            source,
        })?;
    if stdout.len() as u64 > MAX_COMMAND_OUTPUT || stderr.len() as u64 > MAX_COMMAND_OUTPUT {
        return Err(AssetError::Backend(format!(
            "{} exceeded the 1 MiB diagnostic output limit",
            program.display()
        )));
    }
    let output = Output {
        status,
        stdout,
        stderr,
    };
    if output.status.success() {
        return Ok(output);
    }
    Err(AssetError::Backend(format!(
        "{} exited with {}: {}",
        program.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn avif_crf(quality: u8) -> u8 {
    (((100_u16 - u16::from(quality)) * 63 + 50) / 100) as u8
}

fn jpeg_qscale(quality: u8) -> u8 {
    let reduction = (u16::from(quality) * 29 + 50) / 100;
    u8::try_from(31_u16.saturating_sub(reduction).max(2)).unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    #[derive(Clone)]
    struct FakeBackend {
        salt: &'static str,
    }

    impl RasterBackend for FakeBackend {
        fn descriptor(&self) -> BackendDescriptor {
            BackendDescriptor {
                name: "fake".to_owned(),
                version: "1".to_owned(),
            }
        }

        fn dimensions(&self, _input: &Path) -> Result<Dimensions, AssetError> {
            Ok(Dimensions {
                width: 400,
                height: 200,
            })
        }

        fn encode(
            &self,
            _input: &Path,
            output: &Path,
            width: u32,
            format: RasterFormat,
            quality: u8,
        ) -> Result<Dimensions, AssetError> {
            let bytes = format!("{}:{format:?}:{width}:{quality}", self.salt);
            fs::write(output, bytes).map_err(|source| AssetError::Io {
                path: output.to_path_buf(),
                source,
            })?;
            Ok(Dimensions {
                width,
                height: width / 2,
            })
        }
    }

    #[test]
    fn identical_builds_produce_identical_ledgers_and_content_addresses() {
        let root = temporary_directory();
        let input = root.join("source.ppm");
        fs::write(&input, b"source bytes").unwrap();
        let recipe = RasterRecipe {
            asset_id: "hero-frame".to_owned(),
            input,
            output_dir: root.join("output"),
            widths: vec![400, 200, 200, 900],
            formats: vec![RasterFormat::Webp, RasterFormat::Avif, RasterFormat::Webp],
            quality: 78,
        };
        let first = build_raster(&recipe, &FakeBackend { salt: "same" }).unwrap();
        let second = build_raster(&recipe, &FakeBackend { salt: "same" }).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.to_json_bytes().unwrap(),
            second.to_json_bytes().unwrap()
        );
        assert_eq!(first.recipe.widths, vec![200, 400]);
        assert_eq!(first.variants.len(), 4);
        assert!(
            first
                .variants
                .iter()
                .all(|variant| variant.path.contains(&variant.sha256[..16]))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_encoded_bytes_change_the_content_address() {
        let root = temporary_directory();
        let input = root.join("source.ppm");
        fs::write(&input, b"source bytes").unwrap();
        let mut recipe = RasterRecipe {
            asset_id: "poster".to_owned(),
            input,
            output_dir: root.join("one"),
            widths: vec![320],
            formats: vec![RasterFormat::Jpeg],
            quality: 80,
        };
        let first = build_raster(&recipe, &FakeBackend { salt: "one" }).unwrap();
        recipe.output_dir = root.join("two");
        let second = build_raster(&recipe, &FakeBackend { salt: "two" }).unwrap();
        assert_ne!(first.variants[0].sha256, second.variants[0].sha256);
        assert_ne!(first.variants[0].path, second.variants[0].path);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quality_maps_stay_inside_codec_ranges() {
        assert_eq!(avif_crf(100), 0);
        assert_eq!(avif_crf(1), 62);
        assert_eq!(jpeg_qscale(100), 2);
        assert_eq!(jpeg_qscale(1), 31);
    }

    #[test]
    fn legacy_raster_rejects_unbounded_counts_dimensions_and_files() {
        let root = temporary_directory();
        let input = root.join("source.ppm");
        fs::write(&input, b"four").unwrap();
        let mut recipe = RasterRecipe {
            asset_id: "bounded-raster".to_owned(),
            input: input.clone(),
            output_dir: root.join("output"),
            widths: vec![320; MAX_RASTER_WIDTHS + 1],
            formats: vec![RasterFormat::Webp],
            quality: 78,
        };
        assert!(validate_recipe(&recipe).is_err());

        recipe.widths = vec![MAX_RASTER_DIMENSION + 1];
        assert!(validate_recipe(&recipe).is_err());
        recipe.widths = vec![320];
        recipe.formats = vec![RasterFormat::Webp; RasterFormat::ALL.len() + 1];
        assert!(validate_recipe(&recipe).is_err());
        assert!(
            validate_raster_dimensions(
                Dimensions {
                    width: MAX_RASTER_DIMENSION,
                    height: MAX_RASTER_DIMENSION,
                },
                "hostile source",
            )
            .is_err()
        );

        assert!(bounded_file_fingerprint(&input, 3).is_err());
        let (bytes, digest) = bounded_file_fingerprint(&input, 4).unwrap();
        assert_eq!(bytes, 4);
        assert_eq!(digest, format!("{:x}", Sha256::digest(b"four")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn held_asset_handle_cannot_be_substituted_by_replacing_its_path() {
        let root = temporary_directory();
        let staged = root.join("staged.bin");
        let moved = root.join("moved.bin");
        let destination = root.join("published.bin");
        fs::write(&staged, b"approved bytes").unwrap();
        let mut file = open_regular_nofollow(&staged).unwrap();
        let expected = format!("{:x}", Sha256::digest(b"approved bytes"));

        if fs::rename(&staged, &moved).is_ok() {
            fs::write(&staged, b"replacement bytes").unwrap();
            publish_open_file_no_clobber(&mut file, &staged, &destination, &expected).unwrap();
            assert_eq!(fs::read(&destination).unwrap(), b"approved bytes");
        }
        drop(file);
        fs::remove_dir_all(root).unwrap();
    }

    fn temporary_directory() -> PathBuf {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pliego-assets-test-{}-{id}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        path
    }
}
