use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use pliego_assets::{
    AdaptivePlan, AdaptiveRecipe, FfmpegBackend, RasterFormat, RasterRecipe, build_raster,
    finalize_adaptive_assets, plan_adaptive_assets,
};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

const MAX_ADAPTIVE_JSON_BYTES: u64 = 16 * 1024 * 1024;
const USAGE: &str = r#"PLIEGO adaptive asset pipeline

Usage:
  pliego-assets raster <input> --out <directory> --id <asset-id>
      --widths <w1,w2,...> [--formats avif,webp,jpeg] [--quality 1-100]
      [--manifest <file>] [--ffmpeg <program>] [--ffprobe <program>]

  pliego-assets plan <recipe.json> --source-root <directory> --out <plan.json>

  pliego-assets finalize <plan.json> --output-root <directory>
      --manifest <manifest.json>
"#;

#[derive(Debug, Eq, PartialEq)]
struct RasterOptions {
    input: PathBuf,
    output: PathBuf,
    asset_id: String,
    widths: Vec<u32>,
    formats: Vec<RasterFormat>,
    quality: u8,
    manifest: Option<PathBuf>,
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("pliego-assets: {message}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    if matches!(arguments.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{USAGE}");
        return Ok(());
    }
    match arguments.first().map(String::as_str) {
        Some("raster") => run_raster(&arguments),
        Some("plan") => run_plan(&arguments),
        Some("finalize") => run_finalize(&arguments),
        _ => Err("expected the raster, plan, or finalize subcommand".to_owned()),
    }
}

fn run_raster(arguments: &[String]) -> Result<(), String> {
    let options = parse_raster(arguments)?;
    let backend = FfmpegBackend::discover(&options.ffmpeg, &options.ffprobe)
        .map_err(|error| error.to_string())?;
    let report = build_raster(
        &RasterRecipe {
            asset_id: options.asset_id,
            input: options.input,
            output_dir: options.output.clone(),
            widths: options.widths,
            formats: options.formats,
            quality: options.quality,
        },
        &backend,
    )
    .map_err(|error| error.to_string())?;
    let manifest = options
        .manifest
        .unwrap_or_else(|| options.output.join("pliego.generated-assets.json"));
    if let Some(parent) = manifest.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(
        &manifest,
        report.to_json_bytes().map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("{}: {error}", manifest.display()))?;
    println!(
        "PLIEGO assets: {} variants | backend {} {} | {}",
        report.variants.len(),
        report.backend.name,
        report.backend.version,
        manifest.display()
    );
    Ok(())
}

fn run_plan(arguments: &[String]) -> Result<(), String> {
    let (recipe_path, options) = parse_path_command(
        arguments,
        "plan",
        &["--source-root", "--out"],
        &["--source-root", "--out"],
    )?;
    let source_root = PathBuf::from(required(&options, "--source-root")?);
    let destination = PathBuf::from(required(&options, "--out")?);
    let bytes = read_json_input(&recipe_path)?;
    let recipe = AdaptiveRecipe::from_json(&bytes).map_err(|error| error.to_string())?;
    let plan = plan_adaptive_assets(&recipe, &source_root).map_err(|error| error.to_string())?;
    write_json(
        &destination,
        &plan.to_json_bytes().map_err(|error| error.to_string())?,
    )?;
    println!(
        "PLIEGO assets: {} sources | {} jobs | {}",
        plan.sources.len(),
        plan.jobs.len(),
        destination.display()
    );
    Ok(())
}

fn run_finalize(arguments: &[String]) -> Result<(), String> {
    let (plan_path, options) = parse_path_command(
        arguments,
        "finalize",
        &["--output-root", "--manifest"],
        &["--output-root", "--manifest"],
    )?;
    let output_root = PathBuf::from(required(&options, "--output-root")?);
    let destination = PathBuf::from(required(&options, "--manifest")?);
    let bytes = read_json_input(&plan_path)?;
    let plan = AdaptivePlan::from_json(&bytes).map_err(|error| error.to_string())?;
    let manifest =
        finalize_adaptive_assets(&plan, &output_root).map_err(|error| error.to_string())?;
    write_json(
        &destination,
        &manifest
            .to_json_bytes()
            .map_err(|error| error.to_string())?,
    )?;
    println!(
        "PLIEGO assets: {} published variants | {} budget gates | {}",
        manifest.variants.len(),
        manifest.budget_results.len(),
        destination.display()
    );
    Ok(())
}

fn parse_path_command(
    arguments: &[String],
    command: &str,
    allowed: &[&str],
    required_flags: &[&str],
) -> Result<(PathBuf, BTreeMap<String, String>), String> {
    if arguments.first().map(String::as_str) != Some(command) {
        return Err(format!("expected the {command} subcommand"));
    }
    let path = arguments
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .map(PathBuf::from)
        .ok_or_else(|| format!("{command} input is required"))?;
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let mut options = BTreeMap::new();
    let mut index = 2;
    while index < arguments.len() {
        let flag = arguments[index].as_str();
        if !allowed.contains(flag) {
            return Err(format!("unknown option {flag}"));
        }
        let value = arguments
            .get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| format!("{flag} requires a value"))?;
        if options.insert(flag.to_owned(), value.clone()).is_some() {
            return Err(format!("duplicate option {flag}"));
        }
        index += 2;
    }
    for flag in required_flags {
        if !options.contains_key(*flag) {
            return Err(format!("{flag} is required"));
        }
    }
    Ok((path, options))
}

fn required<'a>(options: &'a BTreeMap<String, String>, flag: &str) -> Result<&'a str, String> {
    options
        .get(flag)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} is required"))
}

fn write_json(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "refusing to replace linked or non-file JSON output {}",
                path.display()
            ));
        }
    }
    fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_json_input(path: &Path) -> Result<Vec<u8>, String> {
    read_bounded_input(path, MAX_ADAPTIVE_JSON_BYTES)
}

fn read_bounded_input(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = open_regular_nofollow(path)?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > limit {
        return Err(format!(
            "{} exceeds the {}-byte JSON input limit",
            path.display(),
            limit
        ));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!(
            "{} grew beyond the {limit}-byte JSON input limit while reading",
            path.display()
        ));
    }
    Ok(bytes)
}

fn open_regular_nofollow(path: &Path) -> Result<fs::File, String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let directory = Dir::open_ambient_dir(parent, ambient_authority())
        .map_err(|error| format!("{}: {error}", parent.display()))?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(name, &options)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} must be a regular JSON file", path.display()));
    }
    Ok(file.into_std())
}

fn parse_raster(arguments: &[String]) -> Result<RasterOptions, String> {
    if arguments.first().map(String::as_str) != Some("raster") {
        return Err("expected the raster subcommand".to_owned());
    }
    let input = arguments
        .get(1)
        .filter(|value| !value.starts_with("--"))
        .map(PathBuf::from)
        .ok_or_else(|| "raster input is required".to_owned())?;
    let mut output = None;
    let mut asset_id = None;
    let mut widths = None;
    let mut formats = RasterFormat::ALL.to_vec();
    let mut quality = 78;
    let mut manifest = None;
    let mut ffmpeg = PathBuf::from("ffmpeg");
    let mut ffprobe = PathBuf::from("ffprobe");
    let mut index = 2;
    while index < arguments.len() {
        let flag = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag.as_str() {
            "--out" => output = Some(PathBuf::from(value)),
            "--id" => asset_id = Some(value.clone()),
            "--widths" => widths = Some(parse_widths(value)?),
            "--formats" => formats = parse_formats(value)?,
            "--quality" => {
                quality = value
                    .parse::<u8>()
                    .map_err(|_| "quality must be an integer from 1 to 100".to_owned())?;
            }
            "--manifest" => manifest = Some(PathBuf::from(value)),
            "--ffmpeg" => ffmpeg = PathBuf::from(value),
            "--ffprobe" => ffprobe = PathBuf::from(value),
            other => return Err(format!("unknown option {other}")),
        }
        index += 2;
    }
    Ok(RasterOptions {
        input,
        output: output.ok_or_else(|| "--out is required".to_owned())?,
        asset_id: asset_id.ok_or_else(|| "--id is required".to_owned())?,
        widths: widths.ok_or_else(|| "--widths is required".to_owned())?,
        formats,
        quality,
        manifest,
        ffmpeg,
        ffprobe,
    })
}

fn parse_widths(value: &str) -> Result<Vec<u32>, String> {
    let widths = value
        .split(',')
        .map(|item| {
            item.trim()
                .parse::<u32>()
                .map_err(|_| format!("invalid width {item}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if widths.is_empty() || widths.contains(&0) {
        return Err("widths must contain positive integers".to_owned());
    }
    Ok(widths)
}

fn parse_formats(value: &str) -> Result<Vec<RasterFormat>, String> {
    let formats = value
        .split(',')
        .map(RasterFormat::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if formats.is_empty() {
        return Err("formats must not be empty".to_owned());
    }
    Ok(formats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_builds_a_raster_recipe() {
        let options = parse_raster(&[
            "raster".into(),
            "hero.png".into(),
            "--out".into(),
            "dist".into(),
            "--id".into(),
            "hero".into(),
            "--widths".into(),
            "640,1280".into(),
            "--formats".into(),
            "avif,webp".into(),
            "--quality".into(),
            "82".into(),
        ])
        .unwrap();
        assert_eq!(options.widths, vec![640, 1280]);
        assert_eq!(
            options.formats,
            vec![RasterFormat::Avif, RasterFormat::Webp]
        );
        assert_eq!(options.quality, 82);
    }

    #[test]
    fn parser_rejects_missing_contract_fields() {
        assert!(parse_raster(&["raster".into(), "hero.png".into()]).is_err());
    }

    #[test]
    fn adaptive_commands_reject_duplicate_and_unknown_options() {
        assert!(
            parse_path_command(
                &[
                    "plan".into(),
                    "recipe.json".into(),
                    "--out".into(),
                    "one".into(),
                    "--out".into(),
                    "two".into(),
                    "--source-root".into(),
                    ".".into()
                ],
                "plan",
                &["--out", "--source-root"],
                &["--out", "--source-root"]
            )
            .is_err()
        );
        assert!(
            parse_path_command(
                &[
                    "plan".into(),
                    "recipe.json".into(),
                    "--shell".into(),
                    "oops".into()
                ],
                "plan",
                &["--out", "--source-root"],
                &["--out", "--source-root"]
            )
            .is_err()
        );
    }

    #[test]
    fn json_input_limit_is_enforced_on_the_open_handle() {
        let path =
            std::env::temp_dir().join(format!("pliego-assets-json-limit-{}", std::process::id()));
        fs::write(&path, b"1234").unwrap();
        assert!(read_bounded_input(&path, 3).is_err());
        assert_eq!(read_bounded_input(&path, 4).unwrap(), b"1234");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn json_input_never_follows_a_symbolic_link() {
        let root =
            std::env::temp_dir().join(format!("pliego-assets-json-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let outside = root.with_extension("outside.json");
        fs::write(&outside, b"{}").unwrap();
        let linked = root.join("linked.json");
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(&outside, &linked);
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(&outside, &linked);
        if result.is_ok() {
            assert!(read_json_input(&linked).is_err());
        }
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }
}
