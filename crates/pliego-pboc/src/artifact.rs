// SPDX-License-Identifier: Apache-2.0

use crate::PBOC_FILE_NAME;
use crate::model::PbocManifest;
use crate::validate::{PbocError, validate_manifest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleVerification {
    pub contract: String,
    pub manifest_sha256: String,
    pub artifact_count: usize,
    pub total_bytes: u64,
    pub artifact_set_sha256: String,
}

pub fn verify_bundle(
    root: &Path,
    manifest: &PbocManifest,
) -> Result<BundleVerification, PbocError> {
    validate_manifest(manifest)?;
    let root_metadata = fs::symlink_metadata(root).map_err(|source| io(root, source))?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(PbocError::Artifact(
            "bundle root must be a real directory".to_owned(),
        ));
    }
    let expected: BTreeMap<_, _> = manifest
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.clone(), artifact))
        .collect();
    let mut observed = BTreeSet::new();
    collect_files(root, root, &mut observed)?;
    observed.remove(PBOC_FILE_NAME);
    let expected_paths: BTreeSet<_> = expected.keys().cloned().collect();
    if observed != expected_paths {
        let missing: Vec<_> = expected_paths.difference(&observed).cloned().collect();
        let extra: Vec<_> = observed.difference(&expected_paths).cloned().collect();
        return Err(PbocError::Artifact(format!(
            "bundle exact set differs; missing=[{}] extra=[{}]",
            missing.join(","),
            extra.join(",")
        )));
    }
    let mut set_digest = sha2::Sha256::new();
    use sha2::Digest;
    let mut total = 0_u64;
    for (relative, artifact) in &expected {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        let before = fs::symlink_metadata(&path).map_err(|source| io(&path, source))?;
        if !before.is_file() || before.file_type().is_symlink() {
            return Err(PbocError::Artifact(format!(
                "{} is not a regular file",
                artifact.path
            )));
        }
        if before.len() != artifact.bytes {
            return Err(PbocError::Artifact(format!(
                "{} byte length differs",
                artifact.path
            )));
        }
        let bytes = fs::read(&path).map_err(|source| io(&path, source))?;
        let after = fs::symlink_metadata(&path).map_err(|source| io(&path, source))?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err(PbocError::Artifact(format!(
                "{} changed during verification",
                artifact.path
            )));
        }
        let digest = crate::util::sha256_bytes(&bytes);
        if digest != artifact.sha256 {
            return Err(PbocError::Artifact(format!(
                "{} digest differs",
                artifact.path
            )));
        }
        total = total
            .checked_add(artifact.bytes)
            .ok_or_else(|| PbocError::Artifact("bundle byte total overflows u64".to_owned()))?;
        set_digest.update((artifact.path.len() as u64).to_be_bytes());
        set_digest.update(artifact.path.as_bytes());
        set_digest.update(artifact.bytes.to_be_bytes());
        set_digest.update(artifact.sha256.as_bytes());
    }
    Ok(BundleVerification {
        contract: "dev.pliegors.pboc-bundle-verification/v1".to_owned(),
        manifest_sha256: manifest.sha256()?,
        artifact_count: expected.len(),
        total_bytes: total,
        artifact_set_sha256: hex(&set_digest.finalize()),
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    output: &mut BTreeSet<String>,
) -> Result<(), PbocError> {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .map_err(|source| io(directory, source))?
        .collect::<Result<_, _>>()
        .map_err(|source| io(directory, source))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| io(&path, source))?;
        if metadata.file_type().is_symlink() {
            return Err(PbocError::Artifact(format!(
                "symlink is forbidden: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_files(root, &path, output)?;
        } else if metadata.is_file() {
            let relative = path.strip_prefix(root).map_err(|_| {
                PbocError::Artifact(format!("path escaped bundle: {}", path.display()))
            })?;
            let portable = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            crate::util::validate_portable_path(&portable).map_err(PbocError::Artifact)?;
            output.insert(portable);
        } else {
            return Err(PbocError::Artifact(format!(
                "unsupported file type: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn io(path: &Path, source: std::io::Error) -> PbocError {
    PbocError::Io {
        path: path.display().to_string(),
        source,
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
