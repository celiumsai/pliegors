// SPDX-License-Identifier: Apache-2.0

use pliego_artifact::PortablePath;
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

#[doc(hidden)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub project: Project,
    pub client: Option<Client>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub site_package: String,
    pub output: PathBuf,
}

#[doc(hidden)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub package: String,
    pub wasm_name: String,
    pub bindgen_output: PathBuf,
}

/// Parse and validate the exact `pliego.toml` contract used by the CLI.
///
/// This is public only so the maintained fuzz harness can exercise the real
/// trust boundary without copying its rules. It is not a stable framework API.
#[doc(hidden)]
pub fn parse_project_manifest(source: &str) -> Result<ProjectManifest, String> {
    let manifest = toml::from_str(source).map_err(|error| error.to_string())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

#[doc(hidden)]
pub fn validate_manifest(manifest: &ProjectManifest) -> Result<(), String> {
    let project = &manifest.project;
    if project.name.trim().is_empty() || project.site_package.trim().is_empty() {
        return Err("project name and site_package cannot be empty".to_owned());
    }
    if project.id.len() > 64
        || !project
            .id
            .starts_with(|character: char| character.is_ascii_lowercase())
        || !project.id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        return Err(
            "project.id must start with a lowercase ASCII letter and contain at most 64 lowercase letters, digits, or hyphens"
                .to_owned(),
        );
    }
    let output = validate_generated_path(&project.output, "project.output")?;
    let reserved = PortablePath::parse("target/.pliego")
        .map_err(|error| format!("invalid reserved generated path: {error}"))?;
    if generated_paths_overlap(&output, &reserved) {
        return Err("project.output cannot overlap reserved target/.pliego".to_owned());
    }
    if let Some(client) = &manifest.client {
        if client.package.trim().is_empty() || client.wasm_name.trim().is_empty() {
            return Err("client package and wasm_name cannot be empty".to_owned());
        }
        if !client
            .wasm_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err("client.wasm_name must be a Rust artifact identifier".to_owned());
        }
        let bindgen_output =
            validate_generated_path(&client.bindgen_output, "client.bindgen_output")?;
        if generated_paths_overlap(&bindgen_output, &reserved) {
            return Err("client.bindgen_output cannot overlap reserved target/.pliego".to_owned());
        }
        if generated_paths_overlap(&output, &bindgen_output) {
            return Err(
                "project.output and client.bindgen_output must be disjoint generated paths"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

#[doc(hidden)]
pub fn generated_paths_overlap(left: &PortablePath, right: &PortablePath) -> bool {
    portable_path_prefix(left.collision_key(), right.collision_key())
        || portable_path_prefix(right.collision_key(), left.collision_key())
}

fn portable_path_prefix(prefix: &str, path: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[doc(hidden)]
pub fn validate_relative_path(path: &Path, field: &str) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!("{field} must be a non-empty relative path"));
    }
    Ok(())
}

#[doc(hidden)]
pub fn validate_generated_path(path: &Path, field: &str) -> Result<PortablePath, String> {
    validate_relative_path(path, field)?;
    let value = path
        .to_str()
        .ok_or_else(|| format!("{field} must be valid UTF-8"))?;
    let portable = PortablePath::parse(value)
        .map_err(|error| format!("{field} must be a portable generated path: {error}"))?;
    if portable.as_str() != value {
        return Err(format!("{field} must use canonical NFC spelling"));
    }
    let mut components = portable.as_str().split('/');
    if components.next() != Some("target") || components.next().is_none() {
        return Err(format!(
            "{field} must be a generated path below target/, for example target/site"
        ));
    }
    Ok(portable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_and_validator_share_one_entrypoint() {
        let source = r#"
[project]
id = "sample"
name = "Sample"
site_package = "sample-site"
output = "target/site"
"#;
        assert_eq!(parse_project_manifest(source).unwrap().project.id, "sample");
        assert!(parse_project_manifest(&format!("{source}\nunknown = true\n")).is_err());
    }
}
