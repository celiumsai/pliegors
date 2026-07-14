// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const SOURCE_REV_ENV: &str = "PLIEGORS_SOURCE_REV";
const BUILD_SOURCE_REV_ENV: &str = "PLIEGORS_BUILD_SOURCE_REV";

fn main() {
    println!("cargo:rerun-if-env-changed={SOURCE_REV_ENV}");

    let revision = match env::var(SOURCE_REV_ENV) {
        Ok(revision) => validate_revision(&revision).unwrap_or_else(|error| panic!("{error}")),
        Err(env::VarError::NotUnicode(_)) => {
            panic!("{SOURCE_REV_ENV} must contain a UTF-8 Git commit SHA")
        }
        Err(env::VarError::NotPresent) => match revision_from_checkout() {
            Ok(revision) => revision,
            Err(error) => {
                println!(
                    "cargo:warning=PliegoRS source revision unavailable: {error}. \
                     Source-backed starter generation will fail closed."
                );
                String::new()
            }
        },
    };

    println!("cargo:rustc-env={BUILD_SOURCE_REV_ENV}={revision}");
}

fn revision_from_checkout() -> Result<String, String> {
    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| "Cargo did not provide CARGO_MANIFEST_DIR".to_owned())?,
    );
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "cannot locate the PliegoRS workspace root".to_owned())?
        .to_path_buf();

    let git_root = git(&workspace_root, &["rev-parse", "--show-toplevel"])?;
    let expected_root = workspace_root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize workspace root: {error}"))?;
    let actual_root = PathBuf::from(git_root)
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize Git root: {error}"))?;
    if actual_root != expected_root {
        return Err(format!(
            "Git root {} does not match workspace root {}",
            actual_root.display(),
            expected_root.display()
        ));
    }

    track_git_revision_files(&workspace_root);
    validate_revision(&git(
        &workspace_root,
        &["rev-parse", "--verify", "HEAD^{commit}"],
    )?)
}

fn track_git_revision_files(workspace_root: &Path) {
    let Ok(git_dir) = git(workspace_root, &["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = PathBuf::from(git_dir);
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());

    if let Ok(reference) = git(workspace_root, &["symbolic-ref", "-q", "HEAD"]) {
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join(reference).display()
        );
    }

    let packed_refs = git_dir.join("packed-refs");
    if packed_refs.is_file() {
        println!("cargo:rerun-if-changed={}", packed_refs.display());
    }
}

fn git(workspace_root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(arguments)
        .output()
        .map_err(|error| format!("cannot execute Git: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("Git exited with {}", output.status)
        } else {
            format!("Git failed: {stderr}")
        });
    }

    let stdout =
        String::from_utf8(output.stdout).map_err(|_| "Git returned non-UTF-8 output".to_owned())?;
    Ok(stdout.trim().to_owned())
}

fn validate_revision(revision: &str) -> Result<String, String> {
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{SOURCE_REV_ENV} must be a full 40-character Git commit SHA"
        ));
    }
    Ok(revision.to_ascii_lowercase())
}
