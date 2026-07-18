// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const REPORT_VERSION: &str = "1.0.0";
const PROJECT_FILE: &str = "pliego.toml";
const FIRST_PARTY_PACKAGES: &[&str] = &[
    "pliego-adapters",
    "pliego-artifact",
    "pliego-assets",
    "pliego-cli",
    "pliego-content",
    "pliego-dom",
    "pliego-fold",
    "pliego-hyphae",
    "pliego-inspect",
    "pliego-log",
    "pliego-macros",
    "pliego-reactive",
    "pliego-resume",
    "pliego-ssg",
    "pliego-starters",
];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckedManifest {
    project: CheckedProject,
    client: Option<CheckedClient>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckedProject {
    id: String,
    name: String,
    site_package: String,
    output: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckedClient {
    package: String,
    wasm_name: String,
    bindgen_output: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DoctorOptions {
    pub(crate) format: OutputFormat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReportOptions {
    output: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UpgradeOptions {
    target: semver::Version,
    pub(crate) format: OutputFormat,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpgradeReport {
    #[serde(rename = "reportVersion")]
    report_version: &'static str,
    status: UpgradeStatus,
    #[serde(rename = "cliVersion")]
    cli_version: &'static str,
    #[serde(rename = "targetVersion")]
    target_version: String,
    packages: Vec<LockedPackage>,
    reasons: Vec<String>,
    actions: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum UpgradeStatus {
    Compatible,
    MigrationRequired,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct BundleManifest {
    #[serde(rename = "manifestVersion")]
    manifest_version: &'static str,
    #[serde(rename = "cliVersion")]
    cli_version: &'static str,
    entries: Vec<BundleEntry>,
}

#[derive(Clone, Debug, Serialize)]
struct BundleEntry {
    path: String,
    bytes: usize,
    sha256: String,
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

pub(crate) fn parse_report_options(arguments: Vec<String>) -> Result<ReportOptions, String> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some("--bundle") {
        return Err("usage: pliego report --bundle [--output <path>]".to_owned());
    }
    let mut output = None;
    while let Some(argument) = arguments.next() {
        let value = if argument == "--output" {
            arguments
                .next()
                .ok_or_else(|| "--output requires a path".to_owned())?
        } else if let Some(value) = argument.strip_prefix("--output=") {
            value.to_owned()
        } else {
            return Err(format!("unknown report option `{argument}`"));
        };
        if output.replace(PathBuf::from(value)).is_some() {
            return Err("--output may only be provided once".to_owned());
        }
    }
    Ok(ReportOptions { output })
}

pub(crate) fn parse_upgrade_options(arguments: Vec<String>) -> Result<UpgradeOptions, String> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some("--check") {
        return Err(
            "usage: pliego upgrade --check [--target <version>] [--format <human|json>]".to_owned(),
        );
    }
    let mut target = None;
    let mut format = OutputFormat::Human;
    let mut format_set = false;
    while let Some(argument) = arguments.next() {
        let (name, value) = option_value(argument, &mut arguments)?;
        match name.as_str() {
            "target" => {
                if target.is_some() {
                    return Err("--target may only be provided once".to_owned());
                }
                target = Some(
                    semver::Version::parse(&value)
                        .map_err(|error| format!("invalid --target version: {error}"))?,
                );
            }
            "format" => {
                if format_set {
                    return Err("--format may only be provided once".to_owned());
                }
                format = match value.as_str() {
                    "human" => OutputFormat::Human,
                    "json" => OutputFormat::Json,
                    _ => return Err("--format must be `human` or `json`".to_owned()),
                };
                format_set = true;
            }
            _ => return Err(format!("unknown upgrade option `--{name}`")),
        }
    }
    Ok(UpgradeOptions {
        target: target.unwrap_or_else(|| {
            semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is SemVer")
        }),
        format,
    })
}

fn option_value(
    argument: String,
    arguments: &mut impl Iterator<Item = String>,
) -> Result<(String, String), String> {
    let Some(option) = argument.strip_prefix("--") else {
        return Err(format!("unexpected argument `{argument}`"));
    };
    if let Some((name, value)) = option.split_once('=') {
        if value.is_empty() {
            return Err(format!("--{name} requires a value"));
        }
        Ok((name.to_owned(), value.to_owned()))
    } else {
        Ok((
            option.to_owned(),
            arguments
                .next()
                .ok_or_else(|| format!("--{option} requires a value"))?,
        ))
    }
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

pub(crate) fn create_report(options: ReportOptions) -> Result<PathBuf, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let root = find_project_root(&current)
        .ok_or_else(|| "no pliego.toml was found in this directory or its ancestors".to_owned())?;
    let output = options
        .output
        .unwrap_or_else(|| root.join("pliego-report.tar"));
    if output.exists() {
        return Err(format!(
            "report output `{}` already exists; choose a new path",
            output.display()
        ));
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|error| format!("cannot resolve report output directory: {error}"))?;
    if !parent.is_dir() {
        return Err("report output parent is not a directory".to_owned());
    }
    let filename = output
        .file_name()
        .ok_or_else(|| "report output must name a file".to_owned())?;
    let output = parent.join(filename);
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        filename.to_string_lossy(),
        std::process::id()
    ));

    let files = collect_report_files(&root)?;
    write_report_tar(&temporary, &files)?;
    if let Err(error) = fs::rename(&temporary, &output) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot publish report bundle atomically: {error}"));
    }
    Ok(output)
}

fn collect_report_files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    let doctor = doctor()?;
    files.insert("report/doctor.json".to_owned(), json_bytes(&doctor)?);

    let manifest = fs::read(root.join(PROJECT_FILE))
        .map_err(|error| format!("cannot read {PROJECT_FILE}: {error}"))?;
    validate_report_payload("report/pliego.toml", &manifest, root, 64 * 1024)?;
    let checked = toml::from_str::<CheckedManifest>(&String::from_utf8_lossy(&manifest))
        .map_err(|error| format!("cannot bundle invalid pliego.toml: {error}"))?;
    validate_checked_manifest(&checked)?;
    files.insert("report/pliego.toml".to_owned(), manifest);

    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|error| format!("cannot read Cargo.lock without modifying it: {error}"))?;
    let packages = locked_packages(&lock)?;
    let dependencies = serde_json::json!({
        "schemaVersion": "1.0.0",
        "lockSha256": format!("{:x}", Sha256::digest(lock.as_bytes())),
        "firstParty": packages,
    });
    files.insert(
        "report/dependencies.json".to_owned(),
        json_bytes(&dependencies)?,
    );

    let mut omitted = vec![
        ".env*",
        ".git/**",
        "source and content files",
        "environment names and values",
        "credentials and private keys",
        "generated binaries and dependency caches",
    ];
    let build_report = root.join("target/site/pliego.build.json");
    if build_report.is_file() {
        let bytes = fs::read(&build_report)
            .map_err(|error| format!("cannot read existing build report: {error}"))?;
        match validate_report_payload(
            "report/artifacts/pliego.build.json",
            &bytes,
            root,
            1024 * 1024,
        ) {
            Ok(()) if serde_json::from_slice::<serde_json::Value>(&bytes).is_ok() => {
                files.insert("report/artifacts/pliego.build.json".to_owned(), bytes);
            }
            _ => omitted.push("target/site/pliego.build.json (unsafe or invalid)"),
        }
    } else {
        omitted.push("target/site/pliego.build.json (not present)");
    }
    files.insert(
        "report/omissions.json".to_owned(),
        json_bytes(&serde_json::json!({
            "schemaVersion": "1.0.0",
            "omitted": omitted,
            "note": "This bundle is local output and was not uploaded."
        }))?,
    );

    let entries = files
        .iter()
        .map(|(path, bytes)| BundleEntry {
            path: path.clone(),
            bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        })
        .collect();
    let manifest = BundleManifest {
        manifest_version: REPORT_VERSION,
        cli_version: env!("CARGO_PKG_VERSION"),
        entries,
    };
    files.insert("MANIFEST.json".to_owned(), json_bytes(&manifest)?);
    Ok(files)
}

fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_report_payload(
    label: &str,
    bytes: &[u8],
    root: &Path,
    limit: usize,
) -> Result<(), String> {
    if bytes.len() > limit {
        return Err(format!("{label} exceeds its {limit}-byte report limit"));
    }
    if bytes.contains(&0) {
        return Err(format!("{label} contains NUL bytes"));
    }
    let text = String::from_utf8_lossy(bytes).to_lowercase();
    let root = root.to_string_lossy().to_lowercase();
    let root_portable = root.replace('\\', "/");
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|value| value.to_string_lossy().to_lowercase());
    let mut forbidden = vec![root.as_str(), root_portable.as_str()];
    if let Some(home) = home.as_deref() {
        forbidden.push(home);
    }
    if forbidden
        .into_iter()
        .filter(|needle| !needle.is_empty())
        .any(|needle| text.contains(needle))
    {
        return Err(format!("{label} contains an absolute private path"));
    }
    for marker in [
        "api_key",
        "api-key",
        "authorization:",
        "private_key",
        "password =",
        "secret =",
        "token =",
    ] {
        if text.contains(marker) {
            return Err(format!(
                "{label} contains forbidden secret marker `{marker}`"
            ));
        }
    }
    Ok(())
}

fn write_report_tar(path: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("cannot create report bundle: {error}"))?;
    let result = (|| {
        let mut archive = tar::Builder::new(file);
        for (entry_path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header
                .set_path(entry_path)
                .map_err(|error| format!("invalid report entry `{entry_path}`: {error}"))?;
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            archive
                .append(&header, bytes.as_slice())
                .map_err(|error| format!("cannot append report entry `{entry_path}`: {error}"))?;
        }
        archive
            .finish()
            .map_err(|error| format!("cannot finish report bundle: {error}"))?;
        archive
            .into_inner()
            .map_err(|error| format!("cannot close report bundle: {error}"))?
            .flush()
            .map_err(|error| format!("cannot flush report bundle: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

pub(crate) fn check_upgrade(options: &UpgradeOptions) -> Result<UpgradeReport, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    let root = find_project_root(&current)
        .ok_or_else(|| "no pliego.toml was found in this directory or its ancestors".to_owned())?;
    let manifest_before = fs::read(root.join(PROJECT_FILE))
        .map_err(|error| format!("cannot read {PROJECT_FILE}: {error}"))?;
    let checked = toml::from_str::<CheckedManifest>(&String::from_utf8_lossy(&manifest_before))
        .map_err(|error| format!("cannot check upgrade with invalid pliego.toml: {error}"))?;
    validate_checked_manifest(&checked)?;
    let lock_before = fs::read_to_string(root.join("Cargo.lock"))
        .map_err(|error| format!("cannot check upgrade without Cargo.lock: {error}"))?;
    let packages = locked_packages(&lock_before)?;
    let cli = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("PliegoRS package version is SemVer");
    let mut reasons = Vec::new();
    let mut actions = Vec::new();
    let status = if packages.is_empty() {
        reasons.push("Cargo.lock contains no recognized first-party PliegoRS package".to_owned());
        actions.push(
            "Run `pliego check` and confirm the project uses released pliego-* packages."
                .to_owned(),
        );
        UpgradeStatus::Blocked
    } else if cli != options.target {
        reasons.push(format!(
            "running CLI {cli} cannot assess target {} authoritatively",
            options.target
        ));
        actions.push(format!(
            "Install pliego-cli {} and rerun the same read-only check.",
            options.target
        ));
        UpgradeStatus::Blocked
    } else {
        let drift = packages
            .iter()
            .filter(|package| package.version != options.target.to_string())
            .map(|package| format!("{}@{}", package.name, package.version))
            .collect::<Vec<_>>();
        if drift.is_empty() {
            reasons.push(format!(
                "all {} first-party packages match target {}",
                packages.len(),
                options.target
            ));
            actions.push("No migration is required for the inspected lockfile.".to_owned());
            UpgradeStatus::Compatible
        } else {
            reasons.push(format!("first-party version drift: {}", drift.join(", ")));
            actions.push(format!(
                "Review the {} migration notes, then update all pliego-* requirements together.",
                options.target
            ));
            UpgradeStatus::MigrationRequired
        }
    };
    if fs::read(root.join(PROJECT_FILE)).ok().as_deref() != Some(manifest_before.as_slice())
        || fs::read_to_string(root.join("Cargo.lock")).ok().as_deref() != Some(lock_before.as_str())
    {
        return Err("upgrade check detected an unexpected project mutation".to_owned());
    }
    Ok(UpgradeReport {
        report_version: REPORT_VERSION,
        status,
        cli_version: env!("CARGO_PKG_VERSION"),
        target_version: options.target.to_string(),
        packages,
        reasons,
        actions,
    })
}

fn locked_packages(source: &str) -> Result<Vec<LockedPackage>, String> {
    let lock: toml::Value =
        toml::from_str(source).map_err(|error| format!("Cargo.lock is invalid: {error}"))?;
    let mut packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| {
            let name = package.get("name")?.as_str()?;
            let version = package.get("version")?.as_str()?;
            if !FIRST_PARTY_PACKAGES.contains(&name) {
                return None;
            }
            let source = match package.get("source").and_then(toml::Value::as_str) {
                Some(value) if value.starts_with("registry+") => "registry",
                Some(value) if value.starts_with("git+") => "git",
                Some(_) => "external",
                None => "path",
            };
            Some(LockedPackage {
                name: name.to_owned(),
                version: version.to_owned(),
                source,
            })
        })
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(packages)
}

impl UpgradeReport {
    pub(crate) fn succeeded(&self) -> bool {
        self.status == UpgradeStatus::Compatible
    }

    pub(crate) fn print(&self, format: OutputFormat) -> Result<(), String> {
        if format == OutputFormat::Json {
            println!(
                "{}",
                serde_json::to_string(self).map_err(|error| error.to_string())?
            );
            return Ok(());
        }
        println!(
            "PliegoRS upgrade check: {} -> {} [{:?}]",
            self.cli_version, self.target_version, self.status
        );
        for reason in &self.reasons {
            println!("reason: {reason}");
        }
        for action in &self.actions {
            println!("action: {action}");
        }
        Ok(())
    }
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

fn validate_checked_manifest(manifest: &CheckedManifest) -> Result<(), String> {
    for (field, value) in [
        ("project.id", manifest.project.id.as_str()),
        ("project.name", manifest.project.name.as_str()),
        (
            "project.site_package",
            manifest.project.site_package.as_str(),
        ),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{field} must not be empty"));
        }
    }
    if manifest.project.output.as_os_str().is_empty() {
        return Err("project.output must not be empty".to_owned());
    }
    if let Some(client) = &manifest.client {
        for (field, value) in [
            ("client.package", client.package.as_str()),
            ("client.wasm_name", client.wasm_name.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty"));
            }
        }
        if client.bindgen_output.as_os_str().is_empty() {
            return Err("client.bindgen_output must not be empty".to_owned());
        }
    }
    Ok(())
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
    let manifest: CheckedManifest = match toml::from_str(&source) {
        Ok(manifest) => manifest,
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
    match validate_checked_manifest(&manifest) {
        Ok(()) => checks.push(DoctorCheck::pass(
            "PLG-DOC-006",
            "Project manifest",
            "pliego.toml matches the strict project schema".to_owned(),
        )),
        Err(error) => checks.push(DoctorCheck::fail(
            "PLG-DOC-006",
            "Project manifest",
            error,
            "Provide every required non-empty project and client field.",
        )),
    }
    let requires_wasm = manifest.client.is_some();
    check_output_path(&manifest.project.output, checks);
    check_lockfile(root, checks);
    check_first_party_versions(root, checks);
    Ok(ProjectIdentity {
        present: true,
        root_name,
        requires_wasm,
    })
}

fn check_output_path(value: &Path, checks: &mut Vec<DoctorCheck>) {
    let safe = !value.as_os_str().is_empty()
        && value
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if safe {
        checks.push(DoctorCheck::pass(
            "PLG-DOC-007",
            "Generated output path",
            format!("project.output `{}` is relative", value.display()),
        ));
    } else {
        checks.push(DoctorCheck::fail(
            "PLG-DOC-007",
            "Generated output path",
            format!(
                "project.output `{}` is not a safe relative path",
                value.display()
            ),
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
            FIRST_PARTY_PACKAGES
                .contains(&name)
                .then_some((name, version))
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
