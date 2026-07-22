// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

use pliego_pboc::{
    PbocManifest, decode_manifest, verify_bundle, verify_rollback_transition,
    verify_rolling_transition,
};
use serde_json::json;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(crate) struct CliError {
    usage: bool,
    message: String,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            usage: true,
            message: message.into(),
        }
    }

    fn contract(message: impl Into<String>) -> Self {
        Self {
            usage: false,
            message: message.into(),
        }
    }

    pub(crate) const fn is_usage(&self) -> bool {
        self.usage
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(crate) fn run(arguments: Vec<String>) -> Result<(), CliError> {
    let mut arguments = arguments.into_iter();
    match arguments.next().as_deref() {
        Some("validate") => validate(arguments.collect()),
        Some("admit") => admit(arguments.collect()),
        Some("compatibility") => compatibility(arguments.collect()),
        _ => Err(CliError::usage(
            "usage: pliego pboc <validate|admit|compatibility> ...",
        )),
    }
}

fn validate(arguments: Vec<String>) -> Result<(), CliError> {
    let (manifest_path, options) = parse_manifest_options(arguments, false)?;
    let manifest = load(&manifest_path)?;
    let verification = options
        .root
        .as_ref()
        .map(|root| verify_bundle(root, &manifest).map_err(contract))
        .transpose()?;
    println!(
        "{}",
        json!({
            "contract": "dev.pliegors.pboc-cli-validation/v1",
            "manifestSha256": manifest.sha256().map_err(contract)?,
            "releaseId": manifest.build.release_id,
            "artifactCount": manifest.artifacts.len(),
            "exactSetVerified": verification.is_some(),
            "bundle": verification,
        })
    );
    Ok(())
}

fn admit(arguments: Vec<String>) -> Result<(), CliError> {
    let (manifest_path, options) = parse_manifest_options(arguments, true)?;
    let manifest = load(&manifest_path)?;
    if let Some(root) = &options.root {
        verify_bundle(root, &manifest).map_err(contract)?;
    }
    let version = options
        .host_version
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    let admission = match options.host.as_deref() {
        Some("native") => manifest
            .admit(&pliego_runtime::native_pboc_host_profile(version))
            .map_err(contract)?,
        Some("cloudflare") => manifest
            .admit(&pliego_cloudflare::host_profile(version))
            .map_err(contract)?,
        _ => return Err(CliError::usage("--host must be `native` or `cloudflare`")),
    };
    println!("{}", serde_json::to_string(&admission).map_err(contract)?);
    Ok(())
}

fn compatibility(arguments: Vec<String>) -> Result<(), CliError> {
    if arguments.len() != 4 || arguments[2] != "--direction" {
        return Err(CliError::usage(
            "usage: pliego pboc compatibility <active> <candidate> --direction <rolling|rollback>",
        ));
    }
    let active = load(Path::new(&arguments[0]))?;
    let candidate = load(Path::new(&arguments[1]))?;
    let receipt = match arguments[3].as_str() {
        "rolling" => verify_rolling_transition(&active, &candidate).map_err(contract)?,
        "rollback" => verify_rollback_transition(&active, &candidate).map_err(contract)?,
        _ => {
            return Err(CliError::usage(
                "--direction must be `rolling` or `rollback`",
            ));
        }
    };
    println!("{}", serde_json::to_string(&receipt).map_err(contract)?);
    Ok(())
}

#[derive(Default)]
struct ManifestOptions {
    root: Option<PathBuf>,
    host: Option<String>,
    host_version: Option<String>,
}

fn parse_manifest_options(
    arguments: Vec<String>,
    require_host: bool,
) -> Result<(PathBuf, ManifestOptions), CliError> {
    let mut arguments = arguments.into_iter();
    let manifest = arguments
        .next()
        .filter(|value| !value.starts_with('-'))
        .map(PathBuf::from)
        .ok_or_else(|| CliError::usage("a PBOC manifest path is required"))?;
    let mut options = ManifestOptions::default();
    while let Some(option) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| CliError::usage(format!("{option} requires a value")))?;
        match option.as_str() {
            "--root" if options.root.is_none() => options.root = Some(PathBuf::from(value)),
            "--host" if options.host.is_none() => options.host = Some(value),
            "--host-version" if options.host_version.is_none() => {
                options.host_version = Some(value)
            }
            _ => {
                return Err(CliError::usage(format!(
                    "unknown or repeated option `{option}`"
                )));
            }
        }
    }
    if require_host && options.host.is_none() {
        return Err(CliError::usage("--host is required"));
    }
    if !require_host && (options.host.is_some() || options.host_version.is_some()) {
        return Err(CliError::usage(
            "host options are only valid for `pliego pboc admit`",
        ));
    }
    Ok((manifest, options))
}

fn load(path: &Path) -> Result<PbocManifest, CliError> {
    let bytes = fs::read(path)
        .map_err(|error| CliError::contract(format!("{}: {error}", path.display())))?;
    decode_manifest(&bytes).map_err(contract)
}

fn contract(error: impl Display) -> CliError {
    CliError::contract(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_explicit_and_bounded() {
        let (_, options) = parse_manifest_options(
            vec![
                "pliego.pboc.json".to_owned(),
                "--host".to_owned(),
                "cloudflare".to_owned(),
                "--root".to_owned(),
                "bundle".to_owned(),
            ],
            true,
        )
        .unwrap();
        assert_eq!(options.host.as_deref(), Some("cloudflare"));
        assert_eq!(options.root.as_deref(), Some(Path::new("bundle")));
        assert!(parse_manifest_options(vec!["manifest".to_owned()], true).is_err());
    }
}
