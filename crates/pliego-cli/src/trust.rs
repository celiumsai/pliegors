// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const REPORT_VERSION: &str = "1.0.0";
const PROJECT_FILE: &str = "pliego.toml";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DoctorOptions {
    pub(crate) format: OutputFormat,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DoctorReport {
    #[serde(rename = "reportVersion")]
    report_version: &'static str,
    #[serde(rename = "cliVersion")]
    cli_version: &'static str,
    host: HostIdentity,
    project: Option<ProjectIdentity>,
    checks: Vec<DoctorCheck>,
    summary: DoctorSummary,
}

#[derive(Clone, Debug, Serialize)]
struct HostIdentity {
    os: &'static str,
    architecture: &'static str,
    #[serde(rename = "executableSha256")]
    executable_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ProjectIdentity {
    present: bool,
    #[serde(rename = "rootName")]
    root_name: String,
    #[serde(rename = "requiresWasm")]
    requires_wasm: bool,
}

#[derive(Clone, Debug, Serialize)]
struct DoctorCheck {
    id: &'static str,
    label: &'static str,
    status: CheckStatus,
    cause: String,
    action: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[derive(Clone, Debug, Serialize)]
struct DoctorSummary {
    passed: usize,
    warnings: usize,
    failed: usize,
    skipped: usize,
}

pub(crate) fn parse_doctor_options(arguments: Vec<String>) -> Result<DoctorOptions, String> {
    let mut format = OutputFormat::Human;
    let mut configured = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let value = if argument == "--format" {
            Some(
                arguments
                    .next()
                    .ok_or_else(|| "--format requires `human` or `json`".to_owned())?,
            )
        } else {
            argument.strip_prefix("--format=").map(ToOwned::to_owned)
        };
        let Some(value) = value else {
            return Err(format!(
                "unknown doctor option `{argument}`; usage: pliego doctor [--format <human|json>]"
            ));
        };
        if configured {
            return Err("--format may only be provided once".to_owned());
        }
        format = match value.as_str() {
            "human" => OutputFormat::Human,
            "json" => OutputFormat::Json,
            _ => return Err("--format must be `human` or `json`".to_owned()),
        };
        configured = true;
    }
    Ok(DoctorOptions { format })
}

pub(crate) fn doctor() -> Result<DoctorReport, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut checks = Vec::new();
    checks.push(DoctorCheck::pass(
        "PLG-DOC-001",
        "PliegoRS CLI",
        format!("pliego {} is executable", env!("CARGO_PKG_VERSION")),
    ));

    command_check(
        &mut checks,
        "PLG-DOC-002",
        "Rust compiler",
        "rustc",
        &["--version"],
        true,
        "Install the PliegoRS MSRV toolchain with rustup.",
    );
    command_check(
        &mut checks,
        "PLG-DOC-003",
        "Cargo",
        "cargo",
        &["--version"],
        true,
        "Install Cargo through rustup and ensure it is on PATH.",
    );

    let project_root = find_project_root(&current);
    let project = match project_root.as_deref() {
        None => {
            checks.push(DoctorCheck::skip(
                "PLG-DOC-005",
                "Project discovery",
                "no pliego.toml was found in this directory or its ancestors",
                "Run inside a project to include project checks.",
            ));
            None
        }
        Some(root) => Some(check_project(root, &mut checks)?),
    };

    let requires_wasm = project
        .as_ref()
        .is_some_and(|project| project.requires_wasm);
    command_check(
        &mut checks,
        "PLG-DOC-004",
        "Rustup",
        "rustup",
        &["--version"],
        requires_wasm,
        "Install rustup or provide the required target and tools explicitly.",
    );
    if requires_wasm {
        wasm_target_check(&mut checks);
        command_check(
            &mut checks,
            "PLG-DOC-010",
            "wasm-bindgen CLI",
            "wasm-bindgen",
            &["--version"],
            true,
            "Install the exact wasm-bindgen CLI version required by the PliegoRS release.",
        );
    } else {
        checks.push(DoctorCheck::skip(
            "PLG-DOC-009",
            "WASM target",
            "the discovered project does not declare a client package",
            "No action is required for a server-only or static project.",
        ));
        checks.push(DoctorCheck::skip(
            "PLG-DOC-010",
            "wasm-bindgen CLI",
            "the discovered project does not declare a client package",
            "No action is required for a server-only or static project.",
        ));
    }

    let summary = DoctorSummary::from_checks(&checks);
    Ok(DoctorReport {
        report_version: REPORT_VERSION,
        cli_version: env!("CARGO_PKG_VERSION"),
        host: HostIdentity {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            executable_sha256: executable_sha256(),
        },
        project,
        checks,
        summary,
    })
}

impl DoctorReport {
    pub(crate) fn succeeded(&self) -> bool {
        self.summary.failed == 0
    }

    pub(crate) fn print(&self, format: OutputFormat) -> Result<(), String> {
        match format {
            OutputFormat::Json => {
                println!(
                    "{}",
                    serde_json::to_string(self).map_err(|error| error.to_string())?
                );
            }
            OutputFormat::Human => {
                println!(
                    "PliegoRS doctor {} on {}/{}",
                    self.cli_version, self.host.os, self.host.architecture
                );
                for check in &self.checks {
                    println!(
                        "[{:<4}] {} {}: {}",
                        check.status.label(),
                        check.id,
                        check.label,
                        check.cause
                    );
                    if matches!(check.status, CheckStatus::Warn | CheckStatus::Fail) {
                        println!("       action: {}", check.action);
                    }
                }
                println!(
                    "summary: {} passed, {} warnings, {} failed, {} skipped",
                    self.summary.passed,
                    self.summary.warnings,
                    self.summary.failed,
                    self.summary.skipped
                );
            }
        }
        Ok(())
    }
}

impl CheckStatus {
    const fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }
}

impl DoctorCheck {
    fn pass(id: &'static str, label: &'static str, cause: String) -> Self {
        Self {
            id,
            label,
            status: CheckStatus::Pass,
            cause,
            action: "No action required.".to_owned(),
        }
    }

    fn warn(
        id: &'static str,
        label: &'static str,
        cause: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            id,
            label,
            status: CheckStatus::Warn,
            cause: cause.into(),
            action: action.into(),
        }
    }

    fn fail(
        id: &'static str,
        label: &'static str,
        cause: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            id,
            label,
            status: CheckStatus::Fail,
            cause: cause.into(),
            action: action.into(),
        }
    }

    fn skip(
        id: &'static str,
        label: &'static str,
        cause: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            id,
            label,
            status: CheckStatus::Skip,
            cause: cause.into(),
            action: action.into(),
        }
    }
}

impl DoctorSummary {
    fn from_checks(checks: &[DoctorCheck]) -> Self {
        Self {
            passed: checks
                .iter()
                .filter(|check| check.status == CheckStatus::Pass)
                .count(),
            warnings: checks
                .iter()
                .filter(|check| check.status == CheckStatus::Warn)
                .count(),
            failed: checks
                .iter()
                .filter(|check| check.status == CheckStatus::Fail)
                .count(),
            skipped: checks
                .iter()
                .filter(|check| check.status == CheckStatus::Skip)
                .count(),
        }
    }
}

fn command_check(
    checks: &mut Vec<DoctorCheck>,
    id: &'static str,
    label: &'static str,
    program: &str,
    arguments: &[&str],
    required: bool,
    action: &'static str,
) {
    match Command::new(program).args(arguments).output() {
        Ok(output) if output.status.success() => {
            let value = first_output_line(&output.stdout)
                .or_else(|| first_output_line(&output.stderr))
                .unwrap_or_else(|| "command completed without version output".to_owned());
            checks.push(DoctorCheck::pass(id, label, value));
        }
        Ok(output) => {
            let cause = format!("`{program}` exited with {}", output.status);
            if required {
                checks.push(DoctorCheck::fail(id, label, cause, action));
            } else {
                checks.push(DoctorCheck::warn(id, label, cause, action));
            }
        }
        Err(error) => {
            let cause = format!("`{program}` is unavailable: {error}");
            if required {
                checks.push(DoctorCheck::fail(id, label, cause, action));
            } else {
                checks.push(DoctorCheck::warn(id, label, cause, action));
            }
        }
    }
}

fn first_output_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            line.chars()
                .filter(|character| !character.is_control())
                .take(512)
                .collect()
        })
}

fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(PROJECT_FILE).is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn check_project(root: &Path, checks: &mut Vec<DoctorCheck>) -> Result<ProjectIdentity, String> {
    let root_name = root
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("project")
        .to_owned();
    checks.push(DoctorCheck::pass(
        "PLG-DOC-005",
        "Project discovery",
        format!("found {PROJECT_FILE} in `{root_name}`"),
    ));

    let manifest_path = root.join(PROJECT_FILE);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {PROJECT_FILE}: {error}"))?;
    let manifest: toml::Value = match toml::from_str(&source) {
        Ok(manifest) => {
            checks.push(DoctorCheck::pass(
                "PLG-DOC-006",
                "Project manifest",
                "pliego.toml is valid TOML".to_owned(),
            ));
            manifest
        }
        Err(error) => {
            checks.push(DoctorCheck::fail(
                "PLG-DOC-006",
                "Project manifest",
                format!("pliego.toml is invalid: {error}"),
                "Correct the reported TOML location before running project commands.",
            ));
            return Ok(ProjectIdentity {
                present: true,
                root_name,
                requires_wasm: false,
            });
        }
    };
    let requires_wasm = manifest.get("client").is_some();
    check_output_path(&manifest, checks);
    check_lockfile(root, checks);
    check_first_party_versions(root, checks);
    Ok(ProjectIdentity {
        present: true,
        root_name,
        requires_wasm,
    })
}

fn check_output_path(manifest: &toml::Value, checks: &mut Vec<DoctorCheck>) {
    let value = manifest
        .get("project")
        .and_then(|project| project.get("output"))
        .and_then(toml::Value::as_str);
    let Some(value) = value else {
        checks.push(DoctorCheck::fail(
            "PLG-DOC-007",
            "Generated output path",
            "project.output is missing or is not a string",
            "Set project.output to a relative path such as `target/site`.",
        ));
        return;
    };
    let safe = !value.is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if safe {
        checks.push(DoctorCheck::pass(
            "PLG-DOC-007",
            "Generated output path",
            format!("project.output `{value}` is relative"),
        ));
    } else {
        checks.push(DoctorCheck::fail(
            "PLG-DOC-007",
            "Generated output path",
            format!("project.output `{value}` is not a safe relative path"),
            "Use a relative path with no parent, root, or platform prefix components.",
        ));
    }
}

fn check_lockfile(root: &Path, checks: &mut Vec<DoctorCheck>) {
    if root.join("Cargo.lock").is_file() {
        checks.push(DoctorCheck::pass(
            "PLG-DOC-008",
            "Cargo lockfile",
            "Cargo.lock is present".to_owned(),
        ));
    } else {
        checks.push(DoctorCheck::fail(
            "PLG-DOC-008",
            "Cargo lockfile",
            "Cargo.lock is missing; doctor will not create it",
            "Run `pliego check` once to resolve and verify dependencies.",
        ));
    }
}

fn check_first_party_versions(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let path = root.join("Cargo.lock");
    let Ok(source) = fs::read_to_string(&path) else {
        checks.push(DoctorCheck::skip(
            "PLG-DOC-011",
            "First-party version alignment",
            "Cargo.lock is unavailable",
            "Create a verified lockfile with `pliego check`.",
        ));
        return;
    };
    let Ok(lock): Result<toml::Value, _> = toml::from_str(&source) else {
        checks.push(DoctorCheck::fail(
            "PLG-DOC-011",
            "First-party version alignment",
            "Cargo.lock is not valid TOML",
            "Regenerate the lockfile through a reviewed dependency resolution.",
        ));
        return;
    };
    let versions = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| {
            let name = package.get("name")?.as_str()?;
            let version = package.get("version")?.as_str()?;
            name.starts_with("pliego-").then_some((name, version))
        })
        .collect::<Vec<_>>();
    let mismatched = versions
        .iter()
        .filter(|(_, version)| *version != env!("CARGO_PKG_VERSION"))
        .map(|(name, version)| format!("{name}@{version}"))
        .collect::<Vec<_>>();
    if mismatched.is_empty() {
        checks.push(DoctorCheck::pass(
            "PLG-DOC-011",
            "First-party version alignment",
            format!(
                "{} locked first-party packages agree with CLI {}",
                versions.len(),
                env!("CARGO_PKG_VERSION")
            ),
        ));
    } else {
        checks.push(DoctorCheck::fail(
            "PLG-DOC-011",
            "First-party version alignment",
            format!("version drift: {}", mismatched.join(", ")),
            "Pin all `pliego-*` packages to one exact release version.",
        ));
    }
}

fn wasm_target_check(checks: &mut Vec<DoctorCheck>) {
    match Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(output)
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.trim() == "wasm32-unknown-unknown") =>
        {
            checks.push(DoctorCheck::pass(
                "PLG-DOC-009",
                "WASM target",
                "wasm32-unknown-unknown is installed".to_owned(),
            ));
        }
        Ok(_) => checks.push(DoctorCheck::fail(
            "PLG-DOC-009",
            "WASM target",
            "wasm32-unknown-unknown is not installed",
            "Run `rustup target add wasm32-unknown-unknown` for the active toolchain.",
        )),
        Err(error) => checks.push(DoctorCheck::fail(
            "PLG-DOC-009",
            "WASM target",
            format!("cannot query rustup targets: {error}"),
            "Install rustup and the wasm32-unknown-unknown target.",
        )),
    }
}

fn executable_sha256() -> Option<String> {
    let path = std::env::current_exe().ok()?;
    let bytes = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_options_are_exact() {
        assert_eq!(
            parse_doctor_options(vec!["--format=json".to_owned()]).unwrap(),
            DoctorOptions {
                format: OutputFormat::Json
            }
        );
        assert!(parse_doctor_options(vec!["--json".to_owned()]).is_err());
        assert!(parse_doctor_options(vec!["--format".to_owned(), "yaml".to_owned()]).is_err());
    }

    #[test]
    fn output_paths_are_strictly_relative() {
        for accepted in ["target/site", "site"] {
            assert!(
                Path::new(accepted)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
            );
        }
        for rejected in ["../site", "/tmp/site"] {
            assert!(
                !Path::new(rejected)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
            );
        }
    }
}
