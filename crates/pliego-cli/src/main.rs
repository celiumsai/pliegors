// SPDX-License-Identifier: Apache-2.0

mod development;

use development::{HmrUpdate, explain_artifact, explain_rebuild, load_verified_graph};
use notify::{
    Config as WatchConfig, Event as WatchEvent, RecommendedWatcher, RecursiveMode, Watcher,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use pliego_artifact::{
    BUILD_CONTEXT_ENV, BuildContext, BuildInvocation, FrameworkEvidence, InputMaterialSpec,
    Ownership, PortablePath, ToolchainEvidence, capture_build_context_with_materials,
    verify_build_context_with_materials, verify_build_report, write_build_invocation,
};
use pliego_starters as templates;

const PROJECT_FILE: &str = "pliego.toml";
const SERVER_WORKERS: usize = 8;
const SERVER_QUEUE: usize = 64;
const RELOAD_WORKERS: usize = 16;
const RELOAD_QUEUE: usize = 32;
const MAX_REQUEST_TARGET_BYTES: usize = 4096;
const MAX_CAPTURED_DIAGNOSTIC_BYTES: usize = 256 * 1024;
const RELOAD_PATH: &str = "/_pliego/reload";
static BUILD_CONTEXT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod r1_material_tests;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ProjectManifest {
    project: Project,
    client: Option<Client>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Project {
    id: String,
    name: String,
    site_package: String,
    output: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Client {
    package: String,
    wasm_name: String,
    bindgen_output: PathBuf,
}

struct Context {
    root: PathBuf,
    manifest: ProjectManifest,
}

#[derive(Debug, Eq, PartialEq)]
struct CargoInputSelection {
    project_configuration: Vec<String>,
    materials: Vec<InputMaterialSpec>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_root: PathBuf,
    target_directory: PathBuf,
    resolve: Option<CargoResolve>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    id: String,
    source: Option<String>,
    manifest_path: PathBuf,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    kind: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoResolveNode>,
}

#[derive(Debug, Deserialize)]
struct CargoResolveNode {
    id: String,
    dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<CargoLockPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoLockPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ServerOptions {
    port: u16,
    host: IpAddr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureKind {
    Usage,
    Project,
    Scaffold,
    Check,
    Build,
    Artifact,
    Server,
}

impl FailureKind {
    const fn code(self) -> i32 {
        match self {
            Self::Usage => 2,
            Self::Project | Self::Scaffold => 3,
            Self::Check => 4,
            Self::Build => 5,
            Self::Artifact => 6,
            Self::Server => 7,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Project => "project",
            Self::Scaffold => "scaffold",
            Self::Check => "check",
            Self::Build => "build",
            Self::Artifact => "artifact",
            Self::Server => "server",
        }
    }

    const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::Usage => "PLG-ARG-001",
            Self::Project => "PLG-PRJ-001",
            Self::Scaffold => "PLG-NEW-001",
            Self::Check => "PLG-ENV-001",
            Self::Build => "PLG-BLD-001",
            Self::Artifact => "PLG-ART-001",
            Self::Server => "PLG-SRV-001",
        }
    }

    const fn help(self) -> &'static str {
        match self {
            Self::Usage => {
                "Run `pliego help` or `pliego templates` to inspect the command contract."
            }
            Self::Project => "Run the command inside a project and validate `pliego.toml`.",
            Self::Scaffold => {
                "Choose an empty destination and an explicit dependency source when developing locally."
            }
            Self::Check => {
                "Install or correct the named Rust target, package target, or external tool."
            }
            Self::Build => {
                "Correct the reported compiler or site builder error, then run `pliego build` again."
            }
            Self::Artifact => {
                "Produce a successful build before inspecting or previewing its ledger."
            }
            Self::Server => {
                "Choose an available port and confirm the output directory is readable."
            }
        }
    }
}

#[derive(Debug)]
struct CliFailure {
    kind: FailureKind,
    message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiagnosticSpan {
    file: Option<String>,
    line: u64,
    column: u64,
    label: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixSuggestion {
    message: String,
    applicability: &'static str,
}

impl CliFailure {
    fn new(kind: FailureKind, message: String) -> Self {
        Self { kind, message }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum DiagnosticFormat {
    #[default]
    Human,
    Json,
}

fn main() {
    let arguments = std::env::args().skip(1).collect();
    let (format, arguments) = match parse_global_options(arguments) {
        Ok(options) => options,
        Err(error) => {
            print_failure(DiagnosticFormat::Human, &error);
            std::process::exit(error.kind.code());
        }
    };
    if let Err(error) = run(arguments) {
        print_failure(format, &error);
        std::process::exit(error.kind.code());
    }
}

fn print_failure(format: DiagnosticFormat, error: &CliFailure) {
    let spans = diagnostic_spans(&error.message);
    let fixes = diagnostic_fixes(error);
    match format {
        DiagnosticFormat::Human => {
            eprintln!(
                "PLIEGO[{}] {}: {}\nhelp: {}",
                error.kind.diagnostic_code(),
                error.kind.label(),
                error.message,
                error.kind.help()
            );
            for span in &spans {
                let file = span.file.as_deref().unwrap_or(PROJECT_FILE);
                eprintln!("at: {file}:{}:{} ({})", span.line, span.column, span.label);
            }
            for fix in fixes.iter().filter(|fix| fix.message != error.kind.help()) {
                eprintln!("fix: {}", fix.message);
            }
        }
        DiagnosticFormat::Json => eprintln!(
            "{}",
            serde_json::json!({
                "code": error.kind.diagnostic_code(),
                "exit_code": error.kind.code(),
                "category": error.kind.label(),
                "message": error.message,
                "help": error.kind.help(),
                "spans": spans.iter().map(|span| serde_json::json!({
                    "file": span.file,
                    "line": span.line,
                    "column": span.column,
                    "label": span.label,
                })).collect::<Vec<_>>(),
                "fixes": fixes.iter().map(|fix| serde_json::json!({
                    "message": fix.message,
                    "applicability": fix.applicability,
                })).collect::<Vec<_>>(),
            })
        ),
    }
}

fn diagnostic_spans(message: &str) -> Vec<DiagnosticSpan> {
    let mut spans = Vec::new();
    for line in message.lines() {
        let Some(location) = line.trim().strip_prefix("--> ") else {
            continue;
        };
        if let Some(span) = parse_diagnostic_location(location, "compiler primary") {
            if !spans.contains(&span) {
                spans.push(span);
            }
        }
        if spans.len() == 16 {
            return spans;
        }
    }
    if spans.is_empty() {
        if let Some((file, location)) = message.split_once(": TOML parse error at line ") {
            let mut parts = location.split(|character: char| !character.is_ascii_digit());
            let line = parts.find_map(|value| value.parse::<u64>().ok());
            let column = parts.find_map(|value| value.parse::<u64>().ok());
            if let (Some(line), Some(column)) = (line, column) {
                spans.push(DiagnosticSpan {
                    file: Some(file.trim().to_owned()),
                    line,
                    column,
                    label: "manifest parse",
                });
            }
        }
    }
    spans
}

fn parse_diagnostic_location(location: &str, label: &'static str) -> Option<DiagnosticSpan> {
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?.parse::<u64>().ok()?;
    let line = parts.next()?.parse::<u64>().ok()?;
    let file = parts.next()?.trim();
    if file.is_empty() || file.len() > 4096 || line == 0 || column == 0 {
        return None;
    }
    Some(DiagnosticSpan {
        file: Some(file.to_owned()),
        line,
        column,
        label,
    })
}

fn diagnostic_fixes(error: &CliFailure) -> Vec<FixSuggestion> {
    let mut messages = BTreeSet::from([error.kind.help().to_owned()]);
    for line in error.message.lines() {
        let line = line.trim();
        let suggestion = line
            .strip_prefix("help: ")
            .or_else(|| line.strip_prefix("= help: "));
        let Some(suggestion) = suggestion else {
            continue;
        };
        let suggestion = sanitize_diagnostic(suggestion);
        let suggestion = suggestion.trim();
        if !suggestion.is_empty() && suggestion.len() <= 512 {
            messages.insert(suggestion.to_owned());
        }
        if messages.len() == 16 {
            break;
        }
    }
    messages
        .into_iter()
        .map(|message| FixSuggestion {
            message,
            applicability: "manual",
        })
        .collect()
}

fn parse_global_options(
    arguments: Vec<String>,
) -> Result<(DiagnosticFormat, Vec<String>), CliFailure> {
    let mut format = DiagnosticFormat::Human;
    let mut configured = false;
    let mut output = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if matches!(
            argument.as_str(),
            "--name" | "--framework-path" | "--template"
        ) {
            output.push(argument);
            if let Some(value) = arguments.next() {
                output.push(value);
            }
            continue;
        }
        let value = if argument == "--diagnostic-format" {
            Some(arguments.next().ok_or_else(|| {
                CliFailure::new(
                    FailureKind::Usage,
                    "--diagnostic-format requires `human` or `json`".to_owned(),
                )
            })?)
        } else {
            argument
                .strip_prefix("--diagnostic-format=")
                .map(ToOwned::to_owned)
        };
        if let Some(value) = value {
            if configured {
                return Err(CliFailure::new(
                    FailureKind::Usage,
                    "--diagnostic-format may only be provided once".to_owned(),
                ));
            }
            format = match value.as_str() {
                "human" => DiagnosticFormat::Human,
                "json" => DiagnosticFormat::Json,
                _ => {
                    return Err(CliFailure::new(
                        FailureKind::Usage,
                        "--diagnostic-format must be `human` or `json`".to_owned(),
                    ));
                }
            };
            configured = true;
        } else {
            output.push(argument);
        }
    }
    Ok((format, output))
}

fn run(arguments: Vec<String>) -> Result<(), CliFailure> {
    let mut arguments = arguments.into_iter();
    let command = arguments.next().unwrap_or_else(|| "help".to_owned());
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        print_help();
        return Ok(());
    }
    if matches!(command.as_str(), "version" | "--version" | "-V") {
        println!("pliego {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if command == "new" {
        let options = parse_new_options(arguments.collect())
            .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
        if options.destination.is_none() {
            return Err(CliFailure::new(
                FailureKind::Usage,
                "usage: pliego new <path> [--template <id>] [--name <name>] [--framework-path <path>]"
                    .to_owned(),
            ));
        }
        let template_id = options
            .template
            .as_deref()
            .unwrap_or(templates::DEFAULT_TEMPLATE_ID);
        let template = templates::find(template_id).ok_or_else(|| {
            let available = templates::TEMPLATES
                .iter()
                .map(|template| template.id)
                .collect::<Vec<_>>()
                .join(", ");
            CliFailure::new(
                FailureKind::Usage,
                format!("unknown template `{template_id}`; available templates: {available}"),
            )
        })?;
        return create_project(options, template)
            .map_err(|error| CliFailure::new(FailureKind::Scaffold, error));
    }
    if command == "templates" {
        reject_extra_arguments(arguments)
            .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
        print_templates();
        return Ok(());
    }
    if command == "css" {
        let css_arguments = parse_css_command(arguments.collect())
            .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
        let context =
            load_context().map_err(|error| CliFailure::new(FailureKind::Project, error))?;
        return css_check(&context, css_arguments)
            .map_err(|error| CliFailure::new(FailureKind::Check, error));
    }

    if !matches!(
        command.as_str(),
        "build" | "check" | "dev" | "preview" | "inspect" | "why" | "why-rebuilt"
    ) {
        return Err(CliFailure::new(
            FailureKind::Usage,
            format!("unknown command `{command}`; run `pliego help`"),
        ));
    }

    let context = load_context().map_err(|error| CliFailure::new(FailureKind::Project, error))?;
    match command.as_str() {
        "build" => build(&context).map_err(|error| CliFailure::new(FailureKind::Build, error)),
        "check" => check(&context).map_err(|error| CliFailure::new(FailureKind::Check, error)),
        "dev" => {
            let options = parse_server_options(arguments.collect(), 4400)
                .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
            validate_reproducible_command_context(&context)
                .map_err(|error| CliFailure::new(FailureKind::Check, error))?;
            dev(&context, options).map_err(DevFailure::into_cli_failure)
        }
        "preview" => {
            let options = parse_server_options(arguments.collect(), 4400)
                .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
            let output = context.root.join(&context.manifest.project.output);
            verify_project_build(&context, &output)
                .map_err(|error| CliFailure::new(FailureKind::Artifact, error))?;
            serve(output, options, "preview")
                .map_err(|error| CliFailure::new(FailureKind::Server, error))
        }
        "inspect" => {
            inspect(&context).map_err(|error| CliFailure::new(FailureKind::Artifact, error))
        }
        "why-rebuilt" => {
            reject_extra_arguments(arguments)
                .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
            why_rebuilt(&context).map_err(|error| CliFailure::new(FailureKind::Artifact, error))
        }
        "why" => why(&context, arguments.collect())
            .map_err(|error| CliFailure::new(FailureKind::Artifact, error)),
        _ => unreachable!("known commands were validated before project discovery"),
    }
}

fn print_help() {
    println!(
        "PliegoRS project tool\n\nUSAGE:\n  pliego new <path> [--template <id>] [--name <name>] [--framework-path <path>]\n  pliego templates\n  pliego check\n  pliego css check [pliego-cssc check options]\n  pliego build\n  pliego dev [port] [--host <ip>|--lan]\n  pliego preview [port] [--host <ip>|--lan]\n  pliego inspect\n  pliego why artifact <path|route>\n  pliego why-rebuilt\n  pliego version\n\nGLOBAL OPTIONS:\n  --diagnostic-format <human|json>\n\n`pliego css check` delegates to the separately installed `pliego-cssc` executable.\nServers bind to 127.0.0.1 unless --host or --lan is explicit.\nThe nearest pliego.toml defines an existing project."
    );
}

fn parse_css_command(arguments: Vec<String>) -> Result<Vec<String>, String> {
    let mut arguments = arguments.into_iter();
    match arguments.next().as_deref() {
        Some("check") => Ok(arguments.collect()),
        _ => Err("usage: pliego css check [pliego-cssc check options]".to_owned()),
    }
}

fn css_check(context: &Context, arguments: Vec<String>) -> Result<(), String> {
    validate_reproducible_command_context(context)?;
    let arguments = css_check_arguments(arguments);
    let program = std::env::var_os("PLIEGO_CSSC")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "pliego-cssc".into());
    let directory = command_working_directory(&context.root)?;
    let status = Command::new(&program)
        .arg("check")
        .args(&arguments)
        .current_dir(directory)
        .status()
        .map_err(|error| {
            format!(
                "required tool `{}` is unavailable: {error}; install PliegoCSS with `cargo install --locked --path crates/pliego-cssc`",
                Path::new(&program).display()
            )
        })?;
    if !status.success() {
        return Err(format!("pliego-cssc check exited with {status}"));
    }
    println!("PLIEGO css check: delegated to pliego-cssc");
    Ok(())
}

fn css_check_arguments(mut arguments: Vec<String>) -> Vec<String> {
    if !arguments
        .iter()
        .any(|argument| argument == "--source" || argument.starts_with("--source="))
    {
        arguments.splice(0..0, ["--source".to_owned(), "src".to_owned()]);
    }
    arguments
}

fn print_templates() {
    println!("PliegoRS project templates\n");
    for template in templates::TEMPLATES {
        let marker = if template.id == templates::DEFAULT_TEMPLATE_ID {
            " (default)"
        } else {
            ""
        };
        println!(
            "  {:<12} r{}{}  {} [{}]",
            template.id,
            template.revision,
            marker,
            template.description,
            template.capabilities.join(", ")
        );
    }
}

fn parse_port(value: Option<String>, default: u16) -> Result<u16, String> {
    let port = value
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| "port must be an integer between 1 and 65535".to_owned())
        .map(|port| port.unwrap_or(default))?;
    if port == 0 {
        Err("port must be an integer between 1 and 65535".to_owned())
    } else {
        Ok(port)
    }
}

fn parse_server_options(
    arguments: Vec<String>,
    default_port: u16,
) -> Result<ServerOptions, String> {
    let mut port = None;
    let mut host = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--lan" {
            if host.is_some() {
                return Err("server host may only be provided once".to_owned());
            }
            host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
            continue;
        }
        let host_value = if argument == "--host" {
            Some(
                arguments
                    .next()
                    .ok_or_else(|| "--host requires an IP address".to_owned())?,
            )
        } else {
            argument.strip_prefix("--host=").map(ToOwned::to_owned)
        };
        if let Some(value) = host_value {
            if host.is_some() {
                return Err("server host may only be provided once".to_owned());
            }
            host = Some(
                value
                    .parse::<IpAddr>()
                    .map_err(|_| "--host must be an IPv4 or IPv6 address".to_owned())?,
            );
            continue;
        }
        if argument.starts_with('-') || port.is_some() {
            return Err(format!("unexpected server argument `{argument}`"));
        }
        port = Some(parse_port(Some(argument), default_port)?);
    }
    Ok(ServerOptions {
        port: port.unwrap_or(default_port),
        host: host.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    })
}

fn reject_extra_arguments(mut arguments: impl Iterator<Item = String>) -> Result<(), String> {
    if let Some(argument) = arguments.next() {
        Err(format!("unexpected argument `{argument}`"))
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct NewOptions {
    destination: Option<PathBuf>,
    display_name: Option<String>,
    framework_path: Option<PathBuf>,
    template: Option<String>,
}

fn create_project(options: NewOptions, template: &templates::Template) -> Result<(), String> {
    let destination = options
        .destination
        .expect("new command validates the destination");
    let destination = if destination.is_absolute() {
        destination
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(destination)
    };
    if destination.exists() {
        let metadata = fs::symlink_metadata(&destination)
            .map_err(|error| format!("{}: {error}", destination.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing to scaffold through symbolic link {}",
                destination.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!("{} is not a directory", destination.display()));
        }
        if fs::read_dir(&destination)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err(format!("{} is not empty", destination.display()));
        }
    }

    let inferred_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "project path must end in a UTF-8 directory name".to_owned())?;
    let package_name = package_name(inferred_name)?;
    let display_name = options
        .display_name
        .unwrap_or_else(|| title_from_package(&package_name));
    if display_name.trim().is_empty() {
        return Err("project display name cannot be empty".to_owned());
    }

    let framework_path = options
        .framework_path
        .or_else(|| std::env::var_os("PLIEGO_FRAMEWORK_PATH").map(PathBuf::from));
    let (dom_dependency, ssg_dependency, fold_dependency, log_dependency, source_label) =
        if let Some(root) = framework_path {
            let root = validate_framework_root(&root)?;
            (
                local_dependency(&root.join("crates/pliego-dom"))?,
                local_dependency(&root.join("crates/pliego-ssg"))?,
                local_dependency(&root.join("crates/pliego-fold"))?,
                local_dependency(&root.join("crates/pliego-log"))?,
                format!("local framework {}", cargo_path(&root.to_string_lossy())),
            )
        } else {
            let dependency = registry_dependency();
            (
                dependency.clone(),
                dependency.clone(),
                dependency.clone(),
                dependency,
                format!("crates.io PliegoRS ={}", env!("CARGO_PKG_VERSION")),
            )
        };

    let cargo = template
        .cargo_toml
        .replace("__PACKAGE__", &package_name)
        .replace("__DOM_DEPENDENCY__", &dom_dependency)
        .replace("__SSG_DEPENDENCY__", &ssg_dependency)
        .replace("__FOLD_DEPENDENCY__", &fold_dependency)
        .replace("__LOG_DEPENDENCY__", &log_dependency);
    let manifest = template
        .project_toml
        .replace("__NAME__", &toml_escape(&display_name))
        .replace("__PACKAGE__", &package_name);
    scaffold_project(
        &destination,
        &package_name,
        &display_name,
        &cargo,
        &manifest,
        template,
    )?;

    println!("PLIEGO new: {}", destination.display());
    println!("PLIEGO template: {}", template.id);
    println!("PLIEGO source: {source_label}");
    println!(
        "Next: cd {} && pliego check && pliego dev",
        destination.display()
    );
    Ok(())
}

fn parse_new_options(arguments: Vec<String>) -> Result<NewOptions, String> {
    let mut options = NewOptions::default();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--name" => {
                if options.display_name.is_some() {
                    return Err("--name may only be provided once".to_owned());
                }
                options.display_name = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--name requires a value".to_owned())?,
                );
            }
            "--framework-path" => {
                if options.framework_path.is_some() {
                    return Err("--framework-path may only be provided once".to_owned());
                }
                options.framework_path =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        "--framework-path requires a value".to_owned()
                    })?));
            }
            "--template" => {
                if options.template.is_some() {
                    return Err("--template may only be provided once".to_owned());
                }
                options.template = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--template requires a value".to_owned())?,
                );
            }
            value if value.starts_with('-') => return Err(format!("unknown option `{value}`")),
            value if options.destination.is_none() => {
                options.destination = Some(PathBuf::from(value));
            }
            value => return Err(format!("unexpected argument `{value}`")),
        }
    }
    Ok(options)
}

fn scaffold_project(
    destination: &Path,
    package_name: &str,
    display_name: &str,
    cargo: &str,
    manifest: &str,
    template: &templates::Template,
) -> Result<(), String> {
    validate_template(template)?;
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", destination.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let staging = parent.join(format!(".{package_name}.pliego-new-{}", std::process::id()));
    if staging.exists() {
        return Err(format!(
            "staging path already exists; remove it and retry: {}",
            staging.display()
        ));
    }
    fs::create_dir(&staging)
        .map_err(|error| format!("cannot create {}: {error}", staging.display()))?;

    let result = (|| {
        write_scaffold_file(&staging, Path::new("Cargo.toml"), cargo.as_bytes())?;
        write_scaffold_file(&staging, Path::new("pliego.toml"), manifest.as_bytes())?;
        write_scaffold_file(&staging, Path::new(".gitignore"), template.gitignore)?;
        for file in template.files {
            let bytes = customize_template_file(file, package_name, display_name)?;
            write_scaffold_file(&staging, Path::new(file.path), &bytes)?;
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if destination.exists() {
        let metadata = fs::symlink_metadata(destination)
            .map_err(|error| format!("{}: {error}", destination.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "destination changed while scaffolding: {}",
                destination.display()
            ));
        }
        fs::remove_dir(destination).map_err(|error| {
            format!(
                "cannot replace empty directory {}: {error}",
                destination.display()
            )
        })?;
    }
    fs::rename(&staging, destination).map_err(|error| {
        format!(
            "cannot finalize project at {}; staged files remain at {}: {error}",
            destination.display(),
            staging.display()
        )
    })?;
    Ok(())
}

fn validate_template(template: &templates::Template) -> Result<(), String> {
    let mut paths: BTreeSet<String> = ["cargo.toml", "pliego.toml", ".gitignore"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    for file in template.files {
        let path = Path::new(file.path);
        validate_template_path(path)?;
        let normalized = file.path.replace('\\', "/").to_ascii_lowercase();
        if paths.iter().any(|existing| {
            existing == &normalized
                || existing.starts_with(&(normalized.clone() + "/"))
                || normalized.starts_with(&(existing.clone() + "/"))
        }) {
            return Err(format!(
                "template `{}` contains a reserved or colliding file `{}`",
                template.id, file.path
            ));
        }
        paths.insert(normalized);
    }
    Ok(())
}

fn validate_template_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "template file path must contain only relative directory and file names: {}",
            path.display()
        ));
    }
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            unreachable!("non-normal components were rejected");
        };
        let name = name
            .to_str()
            .ok_or_else(|| format!("template path is not UTF-8: {}", path.display()))?;
        let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
        let windows_reserved = matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        );
        if name.ends_with(' ')
            || name.ends_with('.')
            || name
                .chars()
                .any(|character| matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
            || windows_reserved
        {
            return Err(format!(
                "template path is not portable across supported systems: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn customize_template_file(
    file: &templates::TemplateFile,
    package_name: &str,
    display_name: &str,
) -> Result<Vec<u8>, String> {
    if file.mode == templates::TemplateFileMode::Copy {
        return Ok(file.bytes.to_vec());
    }
    let source = std::str::from_utf8(file.bytes)
        .map_err(|error| format!("template text file `{}` is not UTF-8: {error}", file.path))?;
    let display_name = if file.mode == templates::TemplateFileMode::JsonText {
        let encoded = serde_json::to_string(display_name).map_err(|error| error.to_string())?;
        encoded[1..encoded.len() - 1].to_owned()
    } else {
        display_name.to_owned()
    };
    Ok(source
        .replace("__PACKAGE__", package_name)
        .replace("__NAME__", &display_name)
        .into_bytes())
}

fn write_scaffold_file(root: &Path, relative: &Path, bytes: &[u8]) -> Result<(), String> {
    validate_relative_path(relative, "template file path")?;
    reject_unresolved_tokens(relative, bytes)?;
    let destination = root.join(relative);
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", destination.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("cannot write {}: {error}", destination.display()))
}

fn reject_unresolved_tokens(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Ok(());
    };
    const TOKENS: &[&str] = &[
        "__NAME__",
        "__PACKAGE__",
        "__DOM_DEPENDENCY__",
        "__SSG_DEPENDENCY__",
        "__FOLD_DEPENDENCY__",
        "__LOG_DEPENDENCY__",
    ];
    if let Some(token) = TOKENS.iter().find(|token| source.contains(**token)) {
        Err(format!(
            "unresolved template token `{token}` in {}",
            path.display()
        ))
    } else {
        Ok(())
    }
}

fn package_name(value: &str) -> Result<String, String> {
    let mut name = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !name.is_empty() {
                name.push('-');
            }
            name.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    if name.ends_with('-') {
        name.pop();
    }
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
    {
        return Err(
            "project directory must produce a package name starting with a letter".to_owned(),
        );
    }
    Ok(name)
}

fn title_from_package(package: &str) -> String {
    package
        .split('-')
        .map(|word| {
            let mut characters = word.chars();
            characters
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_framework_root(path: &Path) -> Result<PathBuf, String> {
    let root = path
        .canonicalize()
        .map_err(|error| format!("framework path {}: {error}", path.display()))?;
    if is_framework_root(&root) {
        Ok(root)
    } else {
        Err(format!(
            "{} is not a PliegoRS framework checkout",
            root.display()
        ))
    }
}

fn is_framework_root(path: &Path) -> bool {
    path.join("crates/pliego-dom/Cargo.toml").is_file()
        && path.join("crates/pliego-ssg/Cargo.toml").is_file()
        && path.join("crates/pliego-fold/Cargo.toml").is_file()
        && path.join("crates/pliego-log/Cargo.toml").is_file()
}

fn local_dependency(path: &Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("dependency path {}: {error}", path.display()))?;
    let path = cargo_path(&path.to_string_lossy());
    Ok(format!("{{ path = \"{}\" }}", toml_escape(&path)))
}

fn registry_dependency() -> String {
    format!("{{ version = \"={}\" }}", env!("CARGO_PKG_VERSION"))
}

fn cargo_path(path: &str) -> String {
    path.strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/")
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn load_context() -> Result<Context, String> {
    let mut current = std::env::current_dir().map_err(|error| error.to_string())?;
    loop {
        let manifest_path = current.join(PROJECT_FILE);
        if manifest_path.is_file() {
            let root = canonical_unlinked_project_root(&current)?;
            let manifest_path = root.join(PROJECT_FILE);
            let manifest_metadata = fs::symlink_metadata(&manifest_path)
                .map_err(|error| format!("cannot inspect {}: {error}", manifest_path.display()))?;
            if link_like(&manifest_metadata) || !manifest_metadata.is_file() {
                return Err(format!(
                    "{PROJECT_FILE} must be a regular, non-linked file: {}",
                    manifest_path.display()
                ));
            }
            let source = fs::read_to_string(&manifest_path)
                .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
            let manifest: ProjectManifest = toml::from_str(&source)
                .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
            validate_manifest(&manifest)?;
            return Ok(Context { root, manifest });
        }
        if !current.pop() {
            return Err(format!(
                "run pliego inside a project containing {PROJECT_FILE}"
            ));
        }
    }
}

fn canonical_unlinked_project_root(root: &Path) -> Result<PathBuf, String> {
    canonical_unlinked_directory(root, "project root", false)
}

fn canonical_unlinked_directory(
    path: &Path,
    label: &str,
    allow_missing: bool,
) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("{label} must be absolute: {}", path.display()));
    }
    let mut current = PathBuf::new();
    let mut missing_base = None;
    let mut missing_tail = PathBuf::new();
    for component in path.components() {
        if !matches!(
            component,
            std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::Normal(_)
        ) {
            return Err(format!(
                "{label} contains a non-normal component: {}",
                path.display()
            ));
        }
        if missing_base.is_some() {
            if let std::path::Component::Normal(component) = component {
                missing_tail.push(component);
            }
            continue;
        }
        current.push(component.as_os_str());
        if matches!(
            component,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        ) {
            continue;
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {
                let missing = current
                    .file_name()
                    .ok_or_else(|| format!("cannot resolve missing {label}"))?
                    .to_owned();
                current.pop();
                missing_base = Some(current.canonicalize().map_err(|error| {
                    format!(
                        "cannot canonicalize existing {label} ancestor {}: {error}",
                        current.display()
                    )
                })?);
                missing_tail.push(missing);
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect {label} {}: {error}",
                    current.display()
                ));
            }
        };
        if link_like(&metadata) {
            return Err(format!(
                "{label} cannot traverse a symbolic link or junction: {}",
                current.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "{label} ancestor is not a directory: {}",
                current.display()
            ));
        }
    }
    if let Some(base) = missing_base {
        return Ok(base.join(missing_tail));
    }
    path.canonicalize()
        .map_err(|error| format!("cannot canonicalize {label} {}: {error}", path.display()))
}

fn validate_loaded_context(context: &Context) -> Result<(), String> {
    let canonical = canonical_unlinked_project_root(&context.root)?;
    if canonical != context.root {
        return Err(format!(
            "project root is not canonical: expected {}, got {}",
            canonical.display(),
            context.root.display()
        ));
    }
    let manifest_path = context.root.join(PROJECT_FILE);
    let metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|error| format!("cannot inspect {}: {error}", manifest_path.display()))?;
    if link_like(&metadata) || !metadata.is_file() {
        return Err(format!(
            "{PROJECT_FILE} must remain a regular, non-linked file: {}",
            manifest_path.display()
        ));
    }
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let current: ProjectManifest =
        toml::from_str(&source).map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    validate_manifest(&current)?;
    if current != context.manifest {
        return Err(format!(
            "{PROJECT_FILE} changed after project discovery; restart the command so its paths and ownership are reloaded"
        ));
    }
    Ok(())
}

fn validate_manifest(manifest: &ProjectManifest) -> Result<(), String> {
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

fn generated_paths_overlap(left: &PortablePath, right: &PortablePath) -> bool {
    portable_path_prefix(left.collision_key(), right.collision_key())
        || portable_path_prefix(right.collision_key(), left.collision_key())
}

fn portable_path_prefix(prefix: &str, path: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn validate_relative_path(path: &Path, field: &str) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("{field} must be a non-empty relative path"));
    }
    Ok(())
}

fn validate_generated_path(path: &Path, field: &str) -> Result<PortablePath, String> {
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

fn validate_reproducible_build_environment() -> Result<(), String> {
    let mut forbidden = std::env::vars_os()
        .filter_map(|(name, _)| {
            forbidden_build_environment_name(&name).then(|| name.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    forbidden.sort();
    forbidden.dedup();
    if forbidden.is_empty() {
        return Ok(());
    }
    Err(format!(
        "reproducible PliegoRS commands reject build overrides from the environment: {}; move intentional settings into committed .cargo/config.toml",
        forbidden.join(", ")
    ))
}

fn forbidden_build_environment_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let name = name.to_ascii_uppercase();
    matches!(
        name.as_str(),
        "RUSTC"
            | "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "RUSTC_BOOTSTRAP"
            | "RUSTFLAGS"
            | "CARGO_ENCODED_RUSTFLAGS"
            | "CARGO_INCREMENTAL"
    ) || name.starts_with("CARGO_BUILD_")
        || name.starts_with("CARGO_PROFILE_")
        || (name.starts_with("CARGO_TARGET_") && name != "CARGO_TARGET_DIR")
}

fn validate_generated_write_paths(context: &Context) -> Result<(), String> {
    let paths = std::iter::once(("build context", Path::new("target/.pliego")))
        .chain(std::iter::once((
            "project.output",
            context.manifest.project.output.as_path(),
        )))
        .chain(
            context
                .manifest
                .client
                .iter()
                .map(|client| ("client.bindgen_output", client.bindgen_output.as_path())),
        );
    for (field, path) in paths {
        validate_generated_path(path, field)?;
        validate_existing_generated_ancestors(&context.root, path, field)?;
    }
    Ok(())
}

fn validate_existing_generated_ancestors(
    project_root: &Path,
    relative: &Path,
    field: &str,
) -> Result<(), String> {
    if !project_root.is_absolute() {
        return Err(format!(
            "project root must be absolute before validating {field}"
        ));
    }
    let root_metadata = fs::symlink_metadata(project_root).map_err(|error| {
        format!(
            "cannot inspect project root {} before writing {field}: {error}",
            project_root.display()
        )
    })?;
    if link_like(&root_metadata) || !root_metadata.is_dir() {
        return Err(format!(
            "refusing to write {field} through linked or non-directory project root {}",
            project_root.display()
        ));
    }

    let mut current = project_root.to_owned();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(format!("{field} must remain project-relative"));
        };
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!(
                    "cannot inspect generated path ancestor {} for {field}: {error}",
                    current.display()
                ));
            }
        };
        if link_like(&metadata) {
            return Err(format!(
                "refusing to write {field} through symbolic link or junction {}",
                current.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "generated path ancestor {} for {field} is not a directory",
                current.display()
            ));
        }
    }
    Ok(())
}

fn link_like(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn validate_reproducible_command_context(context: &Context) -> Result<(), String> {
    validate_reproducible_build_environment()?;
    validate_loaded_context(context)?;
    validate_generated_write_paths(context)
}

fn check(context: &Context) -> Result<(), String> {
    validate_reproducible_command_context(context)?;
    let metadata = prepare_cargo_metadata(&context.root)?;
    validate_cargo_target_directory(context, &metadata)?;
    require_package(&metadata, &context.manifest.project.site_package, "bin")?;
    println!(
        "PLIEGO check: site package `{}`",
        context.manifest.project.site_package
    );

    if let Some(client) = &context.manifest.client {
        require_package(&metadata, &client.package, "cdylib")?;
        require_command(&context.root, "wasm-bindgen", &["--version"])?;
        let targets = command_output(&context.root, "rustup", &["target", "list", "--installed"])?;
        if !targets
            .lines()
            .any(|target| target == "wasm32-unknown-unknown")
        {
            return Err(
                "missing Rust target `wasm32-unknown-unknown`; run `rustup target add wasm32-unknown-unknown`"
                    .to_owned(),
            );
        }
        println!("PLIEGO check: client package `{}`", client.package);
        println!("PLIEGO check: wasm32 target and wasm-bindgen");
    }

    let (build_context, material_specs) = capture_project_build_context(context, &metadata)?;
    revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;

    println!(
        "PLIEGO check: {} is valid",
        context.root.join(PROJECT_FILE).display()
    );
    Ok(())
}

fn require_package(metadata: &CargoMetadata, name: &str, target_kind: &str) -> Result<(), String> {
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name == name)
        .ok_or_else(|| {
            format!("Cargo package `{name}` declared in {PROJECT_FILE} was not found")
        })?;
    let has_target = package
        .targets
        .iter()
        .any(|target| target.kind.iter().any(|kind| kind == target_kind));
    if has_target {
        Ok(())
    } else {
        Err(format!(
            "Cargo package `{name}` must expose a `{target_kind}` target"
        ))
    }
}

fn require_command(directory: &Path, program: &str, arguments: &[&str]) -> Result<(), String> {
    command_output(directory, program, arguments)
        .map(|_| ())
        .map_err(|error| format!("required tool `{program}` is unavailable or failed: {error}"))
}

fn command_output(directory: &Path, program: &str, arguments: &[&str]) -> Result<String, String> {
    let directory = command_working_directory(directory)?;
    let output = Command::new(program)
        .args(arguments)
        .current_dir(&directory)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!(
            "{program} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn command_working_directory(directory: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let value = directory.to_str().ok_or_else(|| {
            format!(
                "command working directory is not valid UTF-8: {}",
                directory.display()
            )
        })?;
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(local) = value.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(local));
        }
    }
    Ok(directory.to_owned())
}

fn build(context: &Context) -> Result<(), String> {
    validate_reproducible_command_context(context)?;
    // Resolve the effective workspace and materialize Cargo.lock before the
    // receipt snapshot so first-use builds cannot create an unrecorded input.
    let metadata = prepare_cargo_metadata(&context.root)?;
    let cargo_target = validate_cargo_target_directory(context, &metadata)?;
    let (build_context, material_specs) = capture_project_build_context(context, &metadata)?;
    revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
    let context_path = write_project_build_context(context, &build_context, &material_specs)?;
    let _context_cleanup = FileCleanup::new(context_path.clone());

    if let Some(client) = &context.manifest.client {
        revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
        execute(
            &context.root,
            "cargo",
            &[
                "build",
                "--locked",
                "-p",
                &client.package,
                "--target",
                "wasm32-unknown-unknown",
                "--release",
            ],
        )?;
        revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
        create_generated_directory(context, &client.bindgen_output, "client.bindgen_output")?;
        let wasm_input = path_argument(
            &cargo_target
                .join("wasm32-unknown-unknown/release")
                .join(format!("{}.wasm", client.wasm_name)),
        )?;
        let bindgen_output = path_argument(&client.bindgen_output)?;
        revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
        execute(
            &context.root,
            "wasm-bindgen",
            &[
                &wasm_input,
                "--target",
                "web",
                "--out-dir",
                &bindgen_output,
                "--out-name",
                &client.wasm_name,
                "--no-typescript",
            ],
        )?;
        revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
    }

    let output = path_argument(&context.manifest.project.output)?;
    revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
    execute_with_environment(
        &context.root,
        "cargo",
        &[
            "run",
            "--locked",
            "-p",
            &context.manifest.project.site_package,
            "--",
            &output,
        ],
        Some((BUILD_CONTEXT_ENV, &context_path)),
    )?;
    revalidate_build_inputs(context, &metadata, &build_context, &material_specs)?;
    let output_root = context.root.join(&context.manifest.project.output);
    let verified = verify_build_report(&output_root).map_err(|error| error.to_string())?;
    if verified.report.receipt.context != build_context {
        return Err("site emitted a receipt for a different build context".to_owned());
    }
    println!(
        "PLIEGO build: {} -> {} [{}]",
        context.manifest.project.name,
        context.manifest.project.output.display(),
        &verified.report.receipt_sha256[..12]
    );
    Ok(())
}

fn revalidate_build_inputs(
    context: &Context,
    metadata: &CargoMetadata,
    expected: &BuildContext,
    material_specs: &[InputMaterialSpec],
) -> Result<(), String> {
    validate_reproducible_command_context(context)?;
    validate_cargo_target_directory(context, metadata)?;
    let current = cargo_input_selection(context, metadata)?;
    let expected_configuration = expected
        .configuration
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    if current.project_configuration != expected_configuration
        || current.materials != material_specs
    {
        return Err(
            "Cargo input topology changed after provenance capture; retry from a stable filesystem"
                .to_owned(),
        );
    }
    verify_build_context_with_materials(&context.root, material_specs, expected)
        .map_err(|error| format!("build inputs changed during compilation: {error}"))?;
    let framework_package = resolved_framework_package(context, metadata)?;
    let current_framework =
        resolved_framework_evidence(metadata, framework_package, Some(expected))?;
    if current_framework != expected.framework {
        return Err("resolved pliego-ssg provenance changed during the build".to_owned());
    }
    Ok(())
}

fn create_generated_directory(
    context: &Context,
    relative: &Path,
    field: &str,
) -> Result<(), String> {
    validate_generated_path(relative, field)?;
    validate_existing_generated_ancestors(&context.root, relative, field)?;
    let directory = context.root.join(relative);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {field} {}: {error}", directory.display()))?;
    validate_existing_generated_ancestors(&context.root, relative, field)
}

fn capture_project_build_context(
    context: &Context,
    metadata: &CargoMetadata,
) -> Result<(BuildContext, Vec<InputMaterialSpec>), String> {
    let selection = cargo_input_selection(context, metadata)?;
    let excluded = vec![path_argument(&context.manifest.project.output)?];
    let framework_package = resolved_framework_package(context, metadata)?;
    let preliminary_framework = resolved_framework_evidence(metadata, framework_package, None)?;
    let mut build_context = capture_build_context_with_materials(
        &context.root,
        Ownership {
            project_id: context.manifest.project.id.clone(),
            site_package: context.manifest.project.site_package.clone(),
        },
        preliminary_framework,
        &selection.project_configuration,
        &excluded,
        &selection.materials,
    )
    .map_err(|error| error.to_string())?;
    build_context.framework =
        resolved_framework_evidence(metadata, framework_package, Some(&build_context))?;
    if context.manifest.client.is_some() {
        build_context.toolchain.push(ToolchainEvidence {
            name: "wasm-bindgen".to_owned(),
            version: command_output(&context.root, "wasm-bindgen", &["--version"])
                .map_err(|error| format!("cannot capture wasm-bindgen provenance: {error}"))?,
        });
        build_context
            .toolchain
            .sort_by(|left, right| left.name.cmp(&right.name));
    }
    Ok((build_context, selection.materials))
}

fn resolved_framework_package<'a>(
    context: &Context,
    metadata: &'a CargoMetadata,
) -> Result<&'a CargoPackage, String> {
    let reachable = reachable_package_ids(context, metadata)?;
    let packages = metadata
        .packages
        .iter()
        .filter(|package| package.name == "pliego-ssg" && reachable.contains(&package.id))
        .collect::<Vec<_>>();
    match packages.as_slice() {
        [package] => Ok(*package),
        [] => Err(
            "the resolved site dependency graph does not contain pliego-ssg; framework provenance cannot be established"
                .to_owned(),
        ),
        _ => Err(
            "the resolved site dependency graph contains multiple pliego-ssg packages; framework provenance is ambiguous"
                .to_owned(),
        ),
    }
}

fn resolved_framework_evidence(
    metadata: &CargoMetadata,
    package: &CargoPackage,
    captured: Option<&BuildContext>,
) -> Result<FrameworkEvidence, String> {
    let workspace_root =
        canonical_unlinked_directory(&metadata.workspace_root, "Cargo workspace root", false)?;
    let lock_path = workspace_root.join("Cargo.lock");
    let lock_source = fs::read_to_string(&lock_path).map_err(|error| {
        format!(
            "cannot read resolved Cargo lock {}: {error}",
            lock_path.display()
        )
    })?;
    let lock: CargoLock = toml::from_str(&lock_source).map_err(|error| {
        format!(
            "invalid resolved Cargo lock {}: {error}",
            lock_path.display()
        )
    })?;
    let locked = lock
        .package
        .iter()
        .filter(|locked| {
            locked.name == package.name
                && locked.version == package.version
                && locked.source == package.source
        })
        .collect::<Vec<_>>();
    let locked = match locked.as_slice() {
        [locked] => *locked,
        [] => {
            return Err(format!(
                "resolved pliego-ssg {} from Cargo metadata is absent from {}",
                package.version,
                lock_path.display()
            ));
        }
        _ => {
            return Err(format!(
                "resolved pliego-ssg {} is ambiguous in {}",
                package.version,
                lock_path.display()
            ));
        }
    };

    let source_revision = match package.source.as_deref() {
        Some(source) if source.starts_with("git+") => {
            let revision = source
                .rsplit_once('#')
                .map(|(_, revision)| revision)
                .filter(|revision| {
                    revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or_else(|| {
                    format!(
                        "resolved pliego-ssg Git source does not contain a full commit: {source}"
                    )
                })?;
            revision.to_ascii_lowercase()
        }
        Some(source) if source.starts_with("registry+") || source.starts_with("sparse+") => {
            let checksum = locked.checksum.as_deref().filter(|checksum| {
                checksum.len() == 64
                    && checksum
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            });
            format!(
                "sha256:{}",
                checksum.ok_or_else(|| {
                    "resolved registry pliego-ssg is missing a valid Cargo.lock checksum".to_owned()
                })?
            )
        }
        Some(source) => {
            return Err(format!(
                "unsupported resolved source for pliego-ssg provenance: {source}"
            ));
        }
        None => {
            let Some(captured) = captured else {
                return Ok(FrameworkEvidence {
                    version: package.version.clone(),
                    source_revision: "pending-local-package-content".to_owned(),
                });
            };
            let material_id = cargo_path_material_id(package);
            let material = captured
                .materials
                .iter()
                .find(|material| material.id == material_id)
                .ok_or_else(|| {
                    format!(
                        "resolved local pliego-ssg has no captured input material {material_id:?}"
                    )
                })?;
            format!("sha256:{}", material.sha256)
        }
    };
    Ok(FrameworkEvidence {
        version: package.version.clone(),
        source_revision,
    })
}

fn cargo_path_material_id(package: &CargoPackage) -> String {
    format!("cargo-path/{}@{}", package.name, package.version)
}

fn write_project_build_context(
    context: &Context,
    build_context: &BuildContext,
    material_specs: &[InputMaterialSpec],
) -> Result<PathBuf, String> {
    let relative_directory = Path::new("target/.pliego");
    create_generated_directory(context, relative_directory, "build context")?;
    let directory = context.root.join(relative_directory);
    let path = directory.join(format!(
        "build-context-{}-{}-{}.json",
        std::process::id(),
        BUILD_CONTEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let invocation = BuildInvocation {
        context: build_context.clone(),
        project_root: context.root.clone(),
        output_path: path_argument(&context.manifest.project.output)?,
        material_specs: material_specs.to_vec(),
    };
    write_build_invocation(&path, &invocation).map_err(|error| error.to_string())?;
    validate_existing_generated_ancestors(&context.root, relative_directory, "build context")?;
    Ok(path)
}

fn cargo_metadata(root: &Path) -> Result<CargoMetadata, String> {
    cargo_metadata_with_lock_mode(root, true)
}

fn prepare_cargo_metadata(root: &Path) -> Result<CargoMetadata, String> {
    cargo_metadata_with_lock_mode(root, false)?;
    cargo_metadata(root)
}

fn cargo_metadata_with_lock_mode(root: &Path, locked: bool) -> Result<CargoMetadata, String> {
    let mut command = Command::new("cargo");
    command.args(["metadata", "--format-version=1"]);
    if locked {
        command.arg("--locked");
    }
    let directory = command_working_directory(root)?;
    let output = command
        .current_dir(&directory)
        .output()
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid cargo metadata: {error}"))
}

fn validate_cargo_target_directory(
    context: &Context,
    metadata: &CargoMetadata,
) -> Result<PathBuf, String> {
    if !metadata.target_directory.is_absolute() {
        return Err(format!(
            "Cargo metadata target_directory must be absolute: {}",
            metadata.target_directory.display()
        ));
    }
    portable_absolute_path_tail(&metadata.target_directory, "Cargo target directory")?;

    let raw_target = command_working_directory(&metadata.target_directory)?;
    let command_root = command_working_directory(&context.root)?;
    let raw_project_relative = raw_target
        .strip_prefix(&command_root)
        .ok()
        .map(|relative| portable_relative_path(relative, "Cargo target directory"))
        .transpose()?;
    let canonical_target =
        canonical_unlinked_directory(&raw_target, "Cargo target directory", true)?;
    let canonical_project_relative = canonical_target
        .strip_prefix(&context.root)
        .ok()
        .map(|relative| portable_relative_path(relative, "Cargo target directory"))
        .transpose()?;
    let project_relative = raw_project_relative
        .as_ref()
        .or(canonical_project_relative.as_ref());

    let state = PortablePath::parse("target/.pliego")
        .map_err(|error| format!("invalid reserved generated path: {error}"))?;
    let output = validate_generated_path(&context.manifest.project.output, "project.output")?;
    let mut generated = vec![("build context", state), ("project.output", output)];
    if let Some(client) = &context.manifest.client {
        generated.push((
            "client.bindgen_output",
            validate_generated_path(&client.bindgen_output, "client.bindgen_output")?,
        ));
    }

    let normal_layout = project_relative.is_some_and(|path| path.as_str() == "target");
    if normal_layout {
        let expected = canonical_unlinked_directory(
            &context.root.join("target"),
            "default Cargo target directory",
            true,
        )?;
        if canonical_target != expected {
            return Err(format!(
                "Cargo target directory aliases the default target/ path: {}",
                metadata.target_directory.display()
            ));
        }
        let cargo_children = cargo_owned_target_children(context)?;
        for (label, path) in &generated {
            validate_generated_path_outside_cargo_layout(path, label, &cargo_children)?;
        }
        return Ok(canonical_target);
    }

    for (label, path) in &generated {
        if project_relative.is_some_and(|target| generated_paths_overlap(target, path)) {
            return Err(format!(
                "Cargo target directory {} overlaps {label} {}; choose disjoint directories",
                metadata.target_directory.display(),
                path.as_str()
            ));
        }
        let generated_root =
            canonical_unlinked_directory(&context.root.join(path.as_str()), label, true)?;
        if absolute_paths_overlap(&canonical_target, &generated_root) {
            return Err(format!(
                "Cargo target directory {} overlaps {label} {}; choose disjoint directories",
                metadata.target_directory.display(),
                path.as_str()
            ));
        }
    }
    Ok(canonical_target)
}

fn portable_relative_path(path: &Path, label: &str) -> Result<PortablePath, String> {
    let value = portable_path_components(path, label)?;
    let portable = PortablePath::parse(&value)
        .map_err(|error| format!("{label} must be portable: {error}"))?;
    if portable.as_str() != value {
        return Err(format!("{label} must use canonical NFC spelling"));
    }
    Ok(portable)
}

fn portable_absolute_path_tail(path: &Path, label: &str) -> Result<PortablePath, String> {
    portable_relative_path(path, label)
}

fn portable_path_components(path: &Path, label: &str) -> Result<String, String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {}
            std::path::Component::Normal(component) => components.push(
                component
                    .to_str()
                    .ok_or_else(|| format!("{label} must be valid UTF-8"))?,
            ),
            _ => {
                return Err(format!(
                    "{label} contains a non-normal component: {}",
                    path.display()
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(format!("{label} cannot be a filesystem root"));
    }
    Ok(components.join("/"))
}

fn validate_generated_path_outside_cargo_layout(
    path: &PortablePath,
    label: &str,
    cargo_children: &BTreeSet<String>,
) -> Result<(), String> {
    let child = path
        .collision_key()
        .split('/')
        .nth(1)
        .ok_or_else(|| format!("{label} must be below target/"))?;
    if cargo_children.contains(child) || looks_like_rust_target_triple(child) {
        return Err(format!(
            "{label} {} overlaps Cargo-owned target layout {child:?}",
            path.as_str()
        ));
    }
    Ok(())
}

fn cargo_owned_target_children(context: &Context) -> Result<BTreeSet<String>, String> {
    let mut children = [
        "debug",
        "release",
        "doc",
        "package",
        "tmp",
        ".rustc_info.json",
        "cachedir.tag",
        "wasm32-unknown-unknown",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect::<BTreeSet<_>>();

    let built_in = command_output(&context.root, "rustc", &["--print", "target-list"])
        .map_err(|error| format!("cannot enumerate Rust target layouts: {error}"))?;
    for target in built_in.lines().filter(|target| !target.is_empty()) {
        children.insert(cargo_target_output_child(target)?);
    }
    for target in configured_cargo_build_targets(context)? {
        children.insert(cargo_target_output_child(&target)?);
    }
    Ok(children)
}

fn configured_cargo_build_targets(context: &Context) -> Result<Vec<String>, String> {
    let cargo_home = cargo_home_directory()?;
    let mut files = Vec::new();
    let mut ancestor = Some(context.root.as_path());
    while let Some(root) = ancestor {
        for relative in [".cargo/config.toml", ".cargo/config"] {
            if existing_regular_file_beneath(
                root,
                Path::new(relative),
                "effective Cargo configuration",
            )?
            .is_some()
            {
                files.push(root.join(relative));
            }
        }
        ancestor = root.parent();
    }
    for relative in ["config.toml", "config"] {
        if existing_regular_file_beneath(
            &cargo_home,
            Path::new(relative),
            "Cargo home configuration",
        )?
        .is_some()
        {
            files.push(cargo_home.join(relative));
        }
    }

    let mut targets = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read Cargo config {}: {error}", path.display()))?;
        let value: toml::Value = toml::from_str(&source)
            .map_err(|error| format!("invalid Cargo config {}: {error}", path.display()))?;
        let Some(target) = value
            .get("build")
            .and_then(toml::Value::as_table)
            .and_then(|build| build.get("target"))
        else {
            continue;
        };
        match target {
            toml::Value::String(target) => targets.push(target.clone()),
            toml::Value::Array(values) => {
                for target in values {
                    targets.push(
                        target
                            .as_str()
                            .ok_or_else(|| {
                                format!(
                                    "Cargo build.target in {} must contain only strings",
                                    path.display()
                                )
                            })?
                            .to_owned(),
                    );
                }
            }
            _ => {
                return Err(format!(
                    "Cargo build.target in {} must be a string or string array",
                    path.display()
                ));
            }
        }
    }
    Ok(targets)
}

fn cargo_target_output_child(target: &str) -> Result<String, String> {
    let name = target
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("invalid Cargo build target {target:?}"))?;
    let name = name.strip_suffix(".json").unwrap_or(name);
    let portable = PortablePath::parse(name)
        .map_err(|error| format!("Cargo build target {target:?} is not portable: {error}"))?;
    if portable.as_str() != name || name.contains('/') {
        return Err(format!(
            "Cargo build target {target:?} does not have a canonical portable output name"
        ));
    }
    Ok(portable.collision_key().to_owned())
}

fn looks_like_rust_target_triple(name: &str) -> bool {
    if name.split('-').count() < 3 {
        return false;
    }
    let architecture = name.split('-').next().unwrap_or_default();
    matches!(
        architecture,
        "aarch64"
            | "arm"
            | "avr"
            | "bpfeb"
            | "bpfel"
            | "hexagon"
            | "i586"
            | "i686"
            | "m68k"
            | "nvptx64"
            | "s390x"
            | "x86_64"
    ) || [
        "armv",
        "csky",
        "loongarch",
        "mips",
        "powerpc",
        "riscv",
        "sparc",
        "thumbv",
        "wasm",
    ]
    .iter()
    .any(|prefix| architecture.starts_with(prefix))
}

fn absolute_paths_overlap(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[cfg(test)]
fn cargo_input_materials(
    context: &Context,
    metadata: &CargoMetadata,
) -> Result<Vec<InputMaterialSpec>, String> {
    Ok(cargo_input_selection(context, metadata)?.materials)
}

fn cargo_input_selection(
    context: &Context,
    metadata: &CargoMetadata,
) -> Result<CargoInputSelection, String> {
    let cargo_home = cargo_home_directory()?;
    cargo_input_selection_with_home(context, metadata, &cargo_home)
}

fn cargo_input_selection_with_home(
    context: &Context,
    metadata: &CargoMetadata,
    cargo_home: &Path,
) -> Result<CargoInputSelection, String> {
    let project_root = canonical_unlinked_project_root(&context.root)?;
    if project_root != context.root {
        return Err("project root changed after canonical discovery".to_owned());
    }
    let workspace_root =
        canonical_unlinked_directory(&metadata.workspace_root, "Cargo workspace root", false)?;
    let cargo_home = canonical_unlinked_directory(cargo_home, "Cargo home", true)?;
    if cargo_home.starts_with(&project_root) || project_root.starts_with(&cargo_home) {
        return Err(format!(
            "Cargo home {} cannot overlap project root {}",
            cargo_home.display(),
            project_root.display()
        ));
    }

    let mut project_configuration = Vec::new();
    for path in [PROJECT_FILE, "Cargo.toml", "Cargo.lock"] {
        if existing_regular_file_beneath(&project_root, Path::new(path), "project configuration")?
            .is_some()
        {
            project_configuration.push(path.to_owned());
        }
    }
    let mut materials = Vec::new();

    let lockfile = workspace_root.join("Cargo.lock");
    if existing_regular_file_beneath(
        &workspace_root,
        Path::new("Cargo.lock"),
        "Cargo workspace lockfile",
    )?
    .is_none()
    {
        return Err(format!(
            "Cargo did not materialize the effective workspace lockfile at {}",
            lockfile.display()
        ));
    }
    materials.push(
        InputMaterialSpec::files(
            "cargo-lock",
            "cargo-lock",
            &workspace_root,
            vec!["Cargo.lock".to_owned()],
        )
        .map_err(|error| error.to_string())?,
    );
    if existing_regular_file_beneath(
        &workspace_root,
        Path::new("Cargo.toml"),
        "Cargo workspace manifest",
    )?
    .is_none()
    {
        return Err(format!(
            "Cargo workspace manifest is missing at {}",
            workspace_root.join("Cargo.toml").display()
        ));
    }
    materials.push(
        InputMaterialSpec::files(
            "cargo-workspace-config",
            "cargo-workspace-config",
            &workspace_root,
            vec!["Cargo.toml".to_owned()],
        )
        .map_err(|error| error.to_string())?,
    );

    let (mut local_configuration, mut external_configuration) =
        effective_configuration_materials(&project_root, &cargo_home)?;
    project_configuration.append(&mut local_configuration);
    materials.append(&mut external_configuration);

    let reachable = reachable_package_ids(context, metadata)?;
    for package in &metadata.packages {
        if !reachable.contains(&package.id) || package.source.is_some() {
            continue;
        }
        let package_root = package.manifest_path.parent().ok_or_else(|| {
            format!(
                "cargo metadata package {:?} has no manifest root",
                package.name
            )
        })?;
        let package_root = canonical_unlinked_directory(
            package_root,
            &format!("path package {:?}", package.name),
            false,
        )?;
        let inside_project = package_root.starts_with(&project_root);
        if inside_project && package.name != "pliego-ssg" {
            continue;
        }
        if (!inside_project && project_root.starts_with(&package_root))
            || materials.iter().any(|material| {
                material.included_paths.is_empty()
                    && (package_root.starts_with(&material.root)
                        || material.root.starts_with(&package_root))
            })
        {
            return Err(format!(
                "local package root {} overlaps another build input root",
                package_root.display()
            ));
        }
        materials.push(
            InputMaterialSpec::tree(
                cargo_path_material_id(package),
                "cargo-path-package",
                package_root,
                Vec::new(),
            )
            .map_err(|error| error.to_string())?,
        );
    }
    project_configuration.sort();
    project_configuration.dedup();
    materials.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(CargoInputSelection {
        project_configuration,
        materials,
    })
}

fn cargo_home_directory() -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("CARGO_HOME") {
        if value.is_empty() {
            return Err("CARGO_HOME cannot be empty for a verified build".to_owned());
        }
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(format!(
                "CARGO_HOME must be absolute for a verified build: {}",
                path.display()
            ));
        }
        return canonical_unlinked_directory(&path, "Cargo home", true);
    }

    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").or_else(|| {
        let drive = std::env::var_os("HOMEDRIVE")?;
        let path = std::env::var_os("HOMEPATH")?;
        let mut home = PathBuf::from(drive);
        home.push(path);
        Some(home.into_os_string())
    });
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");

    let home = home.ok_or_else(|| {
        "cannot resolve Cargo home; set CARGO_HOME to an absolute, non-linked directory".to_owned()
    })?;
    canonical_unlinked_directory(&PathBuf::from(home).join(".cargo"), "Cargo home", true)
}

fn effective_configuration_materials(
    project_root: &Path,
    cargo_home: &Path,
) -> Result<(Vec<String>, Vec<InputMaterialSpec>), String> {
    const ANCESTOR_FILES: [&str; 4] = [
        ".cargo/config.toml",
        ".cargo/config",
        "rust-toolchain.toml",
        "rust-toolchain",
    ];
    const CARGO_HOME_FILES: [&str; 2] = ["config.toml", "config"];

    let mut project_configuration = Vec::new();
    let mut materials = Vec::new();
    let mut selected_absolute = BTreeSet::new();
    let mut ancestor = Some(project_root);
    let mut selected_ancestor = 0usize;
    while let Some(root) = ancestor {
        let mut selected = Vec::new();
        for relative in ANCESTOR_FILES {
            if existing_regular_file_beneath(
                root,
                Path::new(relative),
                "effective Cargo or Rust toolchain configuration",
            )?
            .is_some()
            {
                let absolute = root.join(relative);
                let cargo_home_equivalent = match relative {
                    ".cargo/config.toml" => Some(cargo_home.join("config.toml")),
                    ".cargo/config" => Some(cargo_home.join("config")),
                    _ => None,
                };
                if cargo_home_equivalent.as_ref() == Some(&absolute) {
                    continue;
                }
                selected_absolute.insert(absolute);
                if root == project_root {
                    project_configuration.push(relative.to_owned());
                } else {
                    selected.push(relative.to_owned());
                }
            }
        }
        if !selected.is_empty() {
            materials.push(
                InputMaterialSpec::files(
                    format!("cargo-config/ancestor-{selected_ancestor:03}"),
                    "cargo-ancestor-config",
                    root,
                    selected,
                )
                .map_err(|error| error.to_string())?,
            );
            selected_ancestor = selected_ancestor
                .checked_add(1)
                .ok_or_else(|| "Cargo configuration ancestor count overflowed".to_owned())?;
        }
        ancestor = root.parent();
    }

    let mut cargo_home_files = Vec::new();
    for relative in CARGO_HOME_FILES {
        if existing_regular_file_beneath(
            cargo_home,
            Path::new(relative),
            "Cargo home configuration",
        )?
        .is_some()
        {
            let absolute = cargo_home.join(relative);
            if !selected_absolute.contains(&absolute) {
                cargo_home_files.push(relative.to_owned());
            }
        }
    }
    if !cargo_home_files.is_empty() {
        materials.push(
            InputMaterialSpec::files(
                "cargo-config/home",
                "cargo-home-config",
                cargo_home,
                cargo_home_files,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    Ok((project_configuration, materials))
}

fn existing_regular_file_beneath(
    root: &Path,
    relative: &Path,
    label: &str,
) -> Result<Option<()>, String> {
    if !relative
        .components()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("{label} path must be relative and normal"));
    }
    let mut current = root.to_owned();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "cannot inspect {label} {}: {error}",
                    current.display()
                ));
            }
        };
        if link_like(&metadata) {
            return Err(format!(
                "{label} cannot traverse a symbolic link or junction: {}",
                current.display()
            ));
        }
        let final_component = index + 1 == components.len();
        if final_component {
            if !metadata.is_file() {
                return Err(format!(
                    "{label} is not a regular file: {}",
                    current.display()
                ));
            }
        } else if !metadata.is_dir() {
            return Err(format!(
                "{label} ancestor is not a directory: {}",
                current.display()
            ));
        }
    }
    Ok(Some(()))
}

fn reachable_package_ids(
    context: &Context,
    metadata: &CargoMetadata,
) -> Result<BTreeSet<String>, String> {
    let project_root = context
        .root
        .canonicalize()
        .map_err(|error| format!("cannot canonicalize project root: {error}"))?;
    let site_roots = metadata
        .packages
        .iter()
        .filter(|package| package.name == context.manifest.project.site_package)
        .filter_map(|package| {
            let package_root = package.manifest_path.parent()?.canonicalize().ok()?;
            package_root
                .starts_with(&project_root)
                .then(|| package.id.clone())
        })
        .collect::<Vec<_>>();
    if site_roots.len() != 1 {
        return Err(format!(
            "Cargo resolution must contain exactly one site package {:?} below the project root",
            context.manifest.project.site_package
        ));
    }
    let mut root_ids = site_roots;
    if let Some(client) = &context.manifest.client {
        let matches = metadata
            .packages
            .iter()
            .filter(|package| package.name == client.package)
            .map(|package| package.id.clone())
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "Cargo resolution must contain exactly one client package {:?}",
                client.package
            ));
        }
        root_ids.extend(matches);
    }
    let nodes = &metadata
        .resolve
        .as_ref()
        .ok_or_else(|| "cargo metadata omitted the resolved dependency graph".to_owned())?
        .nodes;
    let dependencies = nodes
        .iter()
        .map(|node| (node.id.as_str(), node.dependencies.as_slice()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut reachable = BTreeSet::new();
    let mut pending = root_ids;
    while let Some(id) = pending.pop() {
        if !reachable.insert(id.clone()) {
            continue;
        }
        let next = dependencies.get(id.as_str()).ok_or_else(|| {
            format!("Cargo dependency graph omitted a node for resolved package {id:?}")
        })?;
        pending.extend(next.iter().cloned());
    }
    Ok(reachable)
}

fn path_argument(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))
}

fn execute(root: &Path, program: &str, arguments: &[&str]) -> Result<(), String> {
    execute_with_environment(root, program, arguments, None)
}

fn execute_with_environment(
    root: &Path,
    program: &str,
    arguments: &[&str],
    environment: Option<(&str, &Path)>,
) -> Result<(), String> {
    let directory = command_working_directory(root)?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(&directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((name, value)) = environment {
        command.env(name, value);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("cannot capture {program} standard output"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("cannot capture {program} standard error"))?;
    let stdout_thread = std::thread::spawn(move || tee_command_output(stdout, false));
    let stderr_thread = std::thread::spawn(move || tee_command_output(stderr, true));
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for {program}: {error}"))?;
    let stdout = stdout_thread
        .join()
        .map_err(|_| format!("cannot collect {program} standard output"))?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| format!("cannot collect {program} standard error"))?;
    if status.success() {
        Ok(())
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        if detail.is_empty() {
            Err(format!("{program} exited with {status}"))
        } else {
            Err(format!("{program} exited with {status}\n\n{detail}"))
        }
    }
}

struct FileCleanup {
    path: PathBuf,
}

impl FileCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for FileCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn tee_command_output(mut source: impl Read, error_stream: bool) -> String {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => break,
        };
        if error_stream {
            let mut target = std::io::stderr().lock();
            let _ = target.write_all(&buffer[..read]);
            let _ = target.flush();
        } else {
            let mut target = std::io::stdout().lock();
            let _ = target.write_all(&buffer[..read]);
            let _ = target.flush();
        }
        captured.extend_from_slice(&buffer[..read]);
        if captured.len() > MAX_CAPTURED_DIAGNOSTIC_BYTES {
            let excess = captured.len() - MAX_CAPTURED_DIAGNOSTIC_BYTES;
            captured.drain(..excess);
        }
    }
    String::from_utf8_lossy(&captured).into_owned()
}

fn verify_project_build(
    context: &Context,
    output_root: &Path,
) -> Result<pliego_artifact::VerifiedBuild, String> {
    validate_reproducible_command_context(context)?;
    let verified = verify_build_report(output_root).map_err(|error| error.to_string())?;
    let metadata = cargo_metadata(&context.root)?;
    validate_cargo_target_directory(context, &metadata)?;
    let (expected, _) = capture_project_build_context(context, &metadata)?;
    if verified.report.receipt.context != expected {
        return Err(
            "artifact receipt does not match the current project, sources, configuration, or toolchain"
                .to_owned(),
        );
    }
    Ok(verified)
}

fn inspect(context: &Context) -> Result<(), String> {
    let output_root = context.root.join(&context.manifest.project.output);
    let verified = verify_project_build(context, &output_root)?;
    let files = &verified.report.receipt.outputs.files;
    let html = files
        .iter()
        .filter(|file| file.path.ends_with(".html"))
        .count();
    println!(
        "PLIEGO inspect: VERIFIED {} / {html} HTML routes / {} files / {} bytes",
        &verified.report.receipt_sha256[..12],
        verified.files,
        verified.bytes
    );
    Ok(())
}

fn why(context: &Context, arguments: Vec<String>) -> Result<(), String> {
    if arguments.len() != 2 || arguments[0] != "artifact" {
        return Err("usage: pliego why artifact <path|route>".to_owned());
    }
    let output_root = context.root.join(&context.manifest.project.output);
    verify_project_build(context, &output_root)?;
    let verified = load_verified_graph(&output_root)?;
    println!(
        "PLIEGO why artifact {}\n{}",
        arguments[1],
        explain_artifact(&verified.graph, &arguments[1])?
    );
    Ok(())
}

fn why_rebuilt(context: &Context) -> Result<(), String> {
    validate_reproducible_command_context(context)?;
    let record = development::read_rebuild_record(&context.root)?;
    println!(
        "PLIEGO why-rebuilt generation {} / {:?}\nchanged sources: {}\naffected routes: {}\naffected artifacts: {}\nchanged artifacts: {}\nreceipt: {} -> {}",
        record.generation,
        record.hmr.kind,
        display_items(&record.changed_sources),
        display_items(&record.affected_routes),
        display_items(&record.affected_artifacts),
        display_items(&record.changed_artifacts),
        record.receipt_before.as_deref().unwrap_or("initial"),
        record.receipt_after,
    );
    Ok(())
}

fn display_items(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

enum DevFailure {
    Project(String),
    Server(String),
}

impl DevFailure {
    fn into_cli_failure(self) -> CliFailure {
        match self {
            Self::Project(message) => CliFailure::new(FailureKind::Project, message),
            Self::Server(message) => CliFailure::new(FailureKind::Server, message),
        }
    }
}

struct DevState {
    generation: AtomicU64,
    build_failure: RwLock<Option<String>>,
    update: RwLock<HmrUpdate>,
}

impl DevState {
    fn new(build_failure: Option<String>) -> Self {
        Self {
            generation: AtomicU64::new(0),
            build_failure: RwLock::new(build_failure),
            update: RwLock::new(HmrUpdate::reload()),
        }
    }

    fn publish(&self, build_failure: Option<String>, update: HmrUpdate) -> u64 {
        match self.build_failure.write() {
            Ok(mut current) => *current = build_failure,
            Err(poisoned) => *poisoned.into_inner() = build_failure,
        }
        match self.update.write() {
            Ok(mut current) => *current = update,
            Err(poisoned) => *poisoned.into_inner() = update,
        }
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn failure(&self) -> Option<String> {
        match self.build_failure.read() {
            Ok(current) => current.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn update(&self) -> HmrUpdate {
        match self.update.read() {
            Ok(current) => current.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

fn dev(context: &Context, options: ServerOptions) -> Result<(), DevFailure> {
    validate_reproducible_command_context(context).map_err(DevFailure::Project)?;
    let output_root = context.root.join(&context.manifest.project.output);
    let (initial_failure, mut current_graph) = match build(context) {
        Ok(()) => match load_verified_graph(&output_root) {
            Ok(graph) => (None, Some(graph)),
            Err(error) => (Some(error), None),
        },
        Err(error) => {
            eprintln!(
                "PLIEGO[PLG-BLD-001] build: {error}\nhelp: Fix the source and keep this server open; the browser will recover automatically."
            );
            (Some(error), None)
        }
    };
    let (server, root) =
        open_server(output_root, options, "dev", true).map_err(DevFailure::Server)?;
    let state = Arc::new(DevState::new(initial_failure));
    let server_state = Arc::clone(&state);
    let server_root = root.clone();
    std::thread::spawn(move || serve_requests(server, server_root, Some(server_state)));

    let (events, mut watcher) = native_watcher(&context.root).map_err(DevFailure::Project)?;
    watcher
        .watch(&context.root, RecursiveMode::Recursive)
        .map_err(|error| DevFailure::Project(format!("cannot watch project: {error}")))?;
    println!(
        "PLIEGO dev: watching {} with {:?}",
        context.root.display(),
        RecommendedWatcher::kind()
    );
    loop {
        let changed_sources =
            next_watch_changes(&events, &context.root, &context.manifest.project.output)
                .map_err(DevFailure::Project)?;
        match build(context) {
            Ok(()) => {
                let after = match load_verified_graph(&root) {
                    Ok(graph) => graph,
                    Err(error) => {
                        eprintln!("PLIEGO[PLG-ART-001] rebuilt graph is invalid: {error}");
                        state.publish(Some(error), HmrUpdate::reload());
                        continue;
                    }
                };
                let generation = state.generation.load(Ordering::SeqCst) + 1;
                let record =
                    explain_rebuild(generation, changed_sources, current_graph.as_ref(), &after);
                if let Err(error) = development::write_rebuild_record(&context.root, &record) {
                    eprintln!("PLIEGO[PLG-ART-001] cannot persist rebuild cause: {error}");
                }
                state.publish(None, record.hmr.clone());
                println!(
                    "PLIEGO dev: rebuilt generation {generation} / {:?}",
                    record.hmr.kind
                );
                current_graph = Some(after);
            }
            Err(error) => {
                eprintln!("PLIEGO[PLG-BLD-001] dev rebuild failed: {error}");
                state.publish(Some(error), HmrUpdate::reload());
            }
        }
    }
}

fn native_watcher(
    _root: &Path,
) -> Result<
    (
        mpsc::Receiver<Result<WatchEvent, notify::Error>>,
        RecommendedWatcher,
    ),
    String,
> {
    let (sender, receiver) = mpsc::channel();
    let watcher =
        RecommendedWatcher::new(sender, WatchConfig::default().with_follow_symlinks(false))
            .map_err(|error| format!("cannot initialize native filesystem watcher: {error}"))?;
    Ok((receiver, watcher))
}

fn next_watch_changes(
    events: &mpsc::Receiver<Result<WatchEvent, notify::Error>>,
    root: &Path,
    output: &Path,
) -> Result<BTreeSet<String>, String> {
    let mut changed = BTreeSet::new();
    loop {
        let first = events
            .recv()
            .map_err(|_| "native filesystem watcher stopped unexpectedly".to_owned())?;
        collect_watch_event(first, root, output, &mut changed)?;
        loop {
            match events.recv_timeout(Duration::from_millis(120)) {
                Ok(event) => collect_watch_event(event, root, output, &mut changed)?,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("native filesystem watcher stopped unexpectedly".to_owned());
                }
            }
        }
        if !changed.is_empty() {
            return Ok(changed);
        }
    }
}

fn collect_watch_event(
    event: Result<WatchEvent, notify::Error>,
    root: &Path,
    output: &Path,
    changed: &mut BTreeSet<String>,
) -> Result<(), String> {
    let event = match event {
        Ok(event) => event,
        Err(error) => {
            eprintln!("PLIEGO dev: filesystem watcher reported: {error}");
            return Ok(());
        }
    };
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return Ok(());
    }
    for path in event.paths {
        collect_watch_path(path, root, output, changed)?;
    }
    Ok(())
}

fn collect_watch_path(
    path: PathBuf,
    root: &Path,
    output: &Path,
    changed: &mut BTreeSet<String>,
) -> Result<(), String> {
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(());
    };
    if relative.as_os_str().is_empty()
        || should_ignore_directory(relative, output)
        || should_ignore_development_file(relative)
    {
        return Ok(());
    }
    let relative = relative.to_str().ok_or_else(|| {
        format!(
            "changed project path is not valid UTF-8: {}",
            relative.display()
        )
    })?;
    changed.insert(relative.replace('\\', "/"));
    Ok(())
}

fn should_ignore_directory(relative: &Path, output: &Path) -> bool {
    relative.starts_with(output)
        || relative.components().next().is_some_and(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some(".git" | "target" | "node_modules")
            )
        })
}

fn should_ignore_development_file(relative: &Path) -> bool {
    let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(private) = name.strip_prefix(".pliego-") else {
        return false;
    };
    if private.len() <= 64 {
        return false;
    }
    let (token, suffix) = private.split_at(64);
    token
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        && (suffix == ".lock"
            || suffix
                .strip_prefix("-stage-")
                .is_some_and(|value| !value.is_empty())
            || suffix
                .strip_prefix("-backup-")
                .is_some_and(|value| !value.is_empty()))
}

fn serve(root: PathBuf, options: ServerOptions, mode: &str) -> Result<(), String> {
    let (server, root) = open_server(root, options, mode, false)?;
    serve_requests(server, root, None)
}

fn open_server(
    root: PathBuf,
    options: ServerOptions,
    mode: &str,
    allow_missing_root: bool,
) -> Result<(Server, PathBuf), String> {
    let root = canonical_server_root(&root, allow_missing_root)?;
    let server = Server::http((options.host, options.port)).map_err(|error| error.to_string())?;
    let display_host = match options.host {
        IpAddr::V6(address) => format!("[{address}]"),
        address => address.to_string(),
    };
    println!("PLIEGO {mode}: http://{display_host}:{}", options.port);
    Ok((server, root))
}

fn canonical_server_root(root: &Path, allow_missing: bool) -> Result<PathBuf, String> {
    if root.exists() {
        return root
            .canonicalize()
            .map_err(|error| format!("{}: {error}", root.display()));
    }
    if !allow_missing {
        return Err(format!("{} does not exist", root.display()));
    }
    let mut ancestor = root;
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            format!(
                "cannot find an existing parent for development output {}",
                root.display()
            )
        })?;
        missing.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            format!(
                "cannot find an existing parent for development output {}",
                root.display()
            )
        })?;
    }
    let mut canonical = ancestor
        .canonicalize()
        .map_err(|error| format!("{}: {error}", ancestor.display()))?;
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn serve_requests(server: Server, root: PathBuf, state: Option<Arc<DevState>>) -> ! {
    let (sender, receiver) = mpsc::sync_channel(SERVER_QUEUE);
    spawn_request_workers(SERVER_WORKERS, receiver, &root, state.as_ref());
    let (reload_sender, reload_receiver) = mpsc::sync_channel(RELOAD_QUEUE);
    spawn_request_workers(RELOAD_WORKERS, reload_receiver, &root, state.as_ref());
    for request in server.incoming_requests() {
        let queue = if request.method() == &Method::Get && is_reload_target(request.url()) {
            &reload_sender
        } else {
            &sender
        };
        match queue.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(request)) => respond_busy(request),
            Err(TrySendError::Disconnected(request)) => {
                respond_busy(request);
                break;
            }
        }
    }
    unreachable!("tiny_http incoming request iterator ended")
}

fn spawn_request_workers(
    count: usize,
    receiver: mpsc::Receiver<tiny_http::Request>,
    root: &Path,
    state: Option<&Arc<DevState>>,
) {
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..count {
        let receiver = Arc::clone(&receiver);
        let root = root.to_path_buf();
        let state = state.cloned();
        std::thread::spawn(move || {
            loop {
                let request = match receiver.lock() {
                    Ok(receiver) => receiver.recv(),
                    Err(_) => return,
                };
                match request {
                    Ok(request) => respond(request, &root, state.as_deref()),
                    Err(_) => return,
                }
            }
        });
    }
}

fn is_reload_target(target: &str) -> bool {
    target.len() <= MAX_REQUEST_TARGET_BYTES
        && target
            .split('?')
            .next()
            .is_some_and(|path| path == RELOAD_PATH)
}

fn respond(request: tiny_http::Request, root: &Path, state: Option<&DevState>) {
    if request.url().len() > MAX_REQUEST_TARGET_BYTES {
        let _ = request.respond(
            Response::from_string("Request target too long").with_status_code(StatusCode(414)),
        );
        return;
    }
    if !matches!(request.method(), Method::Get | Method::Head) {
        let response = Response::from_string("Method not allowed")
            .with_status_code(StatusCode(405))
            .with_header(Header::from_bytes("Allow", "GET, HEAD").expect("valid header"));
        let _ = request.respond(response);
        return;
    }
    if is_reload_target(request.url()) {
        if request.method() == &Method::Get {
            respond_reload(request, state);
        } else {
            let response = Response::from_string("Method not allowed")
                .with_status_code(StatusCode(405))
                .with_header(Header::from_bytes("Allow", "GET").expect("valid header"));
            let _ = request.respond(response);
        }
        return;
    }
    let request_path = request.url().split('?').next().unwrap_or("/");
    if let Some(state) = state {
        if should_render_development_diagnostic(request_path) {
            if let Some(error) = state.failure() {
                let response = Response::from_data(development_error_document(
                    &error,
                    state.generation.load(Ordering::SeqCst),
                ))
                .with_status_code(StatusCode(500))
                .with_header(
                    Header::from_bytes("Content-Type", "text/html; charset=utf-8")
                        .expect("valid header"),
                )
                .with_header(
                    Header::from_bytes("Cache-Control", "no-store").expect("valid header"),
                );
                let _ = request.respond(response);
                return;
            }
        }
    }
    let (file, status) = resolve_request_file(root, request_path);
    let (mut bytes, status, content_type, no_store) = match fs::read(&file) {
        Ok(bytes) => (bytes, status, mime_for(&file), status != StatusCode(200)),
        Err(_) if status == StatusCode(404) => (
            not_found_document(request_path),
            status,
            "text/html; charset=utf-8",
            true,
        ),
        Err(_) => (
            internal_server_error_document(),
            StatusCode(500),
            "text/html; charset=utf-8",
            true,
        ),
    };
    if content_type.starts_with("text/html") {
        if let Some(state) = state {
            bytes = inject_live_reload(bytes, state.generation.load(Ordering::SeqCst));
        }
    }
    let mut response = Response::from_data(bytes)
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", content_type).expect("valid header"));
    if no_store || state.is_some() {
        response.add_header(Header::from_bytes("Cache-Control", "no-store").expect("valid header"));
    }
    let _ = request.respond(response);
}

fn should_render_development_diagnostic(request_path: &str) -> bool {
    let path = request_path.trim_end_matches('/');
    let file_name = path.rsplit('/').next().unwrap_or_default();
    file_name.is_empty() || file_name.ends_with(".html") || !file_name.contains('.')
}

fn resolve_request_file(root: &Path, request_path: &str) -> (PathBuf, StatusCode) {
    let fallback = || {
        let candidate = root.join("404.html");
        let file = candidate
            .canonicalize()
            .ok()
            .filter(|path| path.starts_with(root) && path.is_file())
            .unwrap_or_else(|| root.join(".pliego-missing-404"));
        (file, StatusCode(404))
    };
    if request_path.len() > MAX_REQUEST_TARGET_BYTES || request_path.contains(['\\', '\0']) {
        return fallback();
    }
    let relative = request_path.trim_start_matches('/');
    if relative.contains("..") {
        return fallback();
    }
    let mut candidate = if relative.is_empty() {
        root.join("index.html")
    } else {
        root.join(relative)
    };
    if candidate.is_dir() || (!candidate.exists() && candidate.extension().is_none()) {
        candidate = candidate.join("index.html");
    }
    match candidate.canonicalize() {
        Ok(file) if file.starts_with(root) && file.is_file() => (file, StatusCode(200)),
        _ => fallback(),
    }
}

fn respond_busy(request: tiny_http::Request) {
    let response = Response::from_string("Server busy")
        .with_status_code(StatusCode(503))
        .with_header(Header::from_bytes("Retry-After", "1").expect("valid header"));
    let _ = request.respond(response);
}

fn respond_reload(request: tiny_http::Request, state: Option<&DevState>) {
    let Some(state) = state else {
        let _ = request.respond(Response::empty(StatusCode(404)));
        return;
    };
    let since = request
        .url()
        .split_once('?')
        .map(|(_, query)| query)
        .and_then(|query| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix("since="))
        })
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| state.generation.load(Ordering::SeqCst));
    let deadline = Instant::now() + Duration::from_secs(25);
    while state.generation.load(Ordering::SeqCst) == since && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(150));
    }
    let current = state.generation.load(Ordering::SeqCst);
    let body = if current == since {
        ": heartbeat\n\n".to_owned()
    } else {
        let update = state.update();
        let payload = serde_json::json!({
            "generation": current,
            "kind": update.kind,
            "paths": update.paths,
            "routes": update.routes,
        });
        format!("event: pliego\ndata: {payload}\n\n")
    };
    let response = Response::from_string(body)
        .with_header(
            Header::from_bytes("Content-Type", "text/event-stream; charset=utf-8")
                .expect("valid header"),
        )
        .with_header(Header::from_bytes("Cache-Control", "no-cache").expect("valid header"));
    let _ = request.respond(response);
}

fn development_error_document(error: &str, generation: u64) -> Vec<u8> {
    framework_error_document(
        "PLG-BLD-001",
        "Build interrupted.",
        "PliegoRS kept the development server alive. Correct the diagnostic below and save; this page will reload automatically.",
        Some(error),
        Some(generation),
    )
}

fn not_found_document(request_path: &str) -> Vec<u8> {
    framework_error_document(
        "PLG-HTTP-404",
        "This route is not part of the fold.",
        "Check the address or add a /404.html page to own this response in your project.",
        Some(&format!("Requested route: {request_path}")),
        None,
    )
}

fn internal_server_error_document() -> Vec<u8> {
    framework_error_document(
        "PLG-HTTP-500",
        "The document could not be read.",
        "Rebuild the project and inspect the output ledger before restarting the server.",
        None,
        None,
    )
}

fn framework_error_document(
    code: &str,
    title: &str,
    explanation: &str,
    detail: Option<&str>,
    generation: Option<u64>,
) -> Vec<u8> {
    let detail = detail
        .map(|detail| {
            format!(
                "<section class=\"diagnostic\" aria-labelledby=\"diagnostic-title\"><div><span id=\"diagnostic-title\">DIAGNOSTIC</span><span>RUST / BUILD</span></div><pre><code>{}</code></pre></section>",
                escape_html(&sanitize_diagnostic(detail))
            )
        })
        .unwrap_or_default();
    let reload = generation.map(live_reload_script).unwrap_or_default();
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="robots" content="noindex"><meta name="generator" content="PliegoRS"><title>{code} / PliegoRS</title><style>:root{{color-scheme:dark;--ink:#eef0e8;--ground:#111310;--panel:#191c18;--line:#3b3f38;--muted:#a7aca3;--signal:#d6ff3f;--alert:#e95e52;--mono:"SFMono-Regular",Consolas,"Liberation Mono",monospace;--sans:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}}*{{box-sizing:border-box}}body{{min-height:100vh;margin:0;color:var(--ink);background:var(--ground);font-family:var(--sans)}}main{{width:min(100% - 40px,1180px);min-height:100vh;margin:auto;padding:34px 0 72px;display:flex;flex-direction:column}}header{{display:flex;align-items:center;justify-content:space-between;gap:24px;padding-bottom:22px;border-bottom:1px solid var(--line);font:700 .72rem/1 var(--mono)}}.brand{{display:flex;align-items:center;gap:11px}}.mark{{width:24px;height:24px;border:6px solid var(--ink);border-right-color:transparent;position:relative}}.mark:after{{content:"";width:6px;height:6px;position:absolute;right:-10px;bottom:-10px;background:var(--alert)}}.code{{color:var(--alert)}}.intro{{display:grid;grid-template-columns:minmax(0,1fr) minmax(260px,.5fr);gap:clamp(38px,8vw,120px);align-items:end;padding:clamp(72px,12vh,130px) 0 56px}}h1{{max-width:850px;margin:0;font-size:clamp(3rem,8vw,7rem);font-weight:560;line-height:.92;letter-spacing:0}}.intro p{{max-width:480px;margin:0;color:var(--muted);font-size:1rem;line-height:1.65}}.diagnostic{{border:1px solid var(--line);background:var(--panel)}}.diagnostic>div{{min-height:48px;padding:0 16px;display:flex;align-items:center;justify-content:space-between;gap:20px;border-bottom:1px solid var(--line);color:var(--muted);font:700 .66rem/1 var(--mono)}}.diagnostic>div span:first-child{{color:var(--signal)}}pre{{max-height:42vh;margin:0;padding:22px;overflow:auto;white-space:pre-wrap;word-break:break-word;color:#d9ddd5;font:400 .78rem/1.65 var(--mono)}}footer{{display:flex;justify-content:space-between;gap:20px;margin-top:auto;padding-top:32px;color:var(--muted);font:700 .66rem/1.5 var(--mono)}}@media(max-width:720px){{main{{width:min(100% - 28px,1180px);padding-top:24px}}.intro{{grid-template-columns:1fr;padding:64px 0 38px}}h1{{font-size:clamp(2.9rem,15vw,5rem)}}footer{{flex-direction:column}}}}</style></head><body><main><header><div class="brand"><span class="mark" aria-hidden="true"></span><span>PLIEGORS / DEVELOPMENT</span></div><span class="code">{}</span></header><section class="intro"><h1>{}</h1><p>{}</p></section>{}<footer><span>PLIEGORS DIAGNOSTIC SURFACE</span><span>THE SERVER IS STILL RUNNING</span></footer></main>{}</body></html>"#,
        escape_html(code),
        escape_html(title),
        escape_html(explanation),
        detail,
        reload
    )
    .into_bytes()
}

fn sanitize_diagnostic(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect()
}

fn escape_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
    output
}

fn inject_live_reload(bytes: Vec<u8>, generation: u64) -> Vec<u8> {
    let html = match String::from_utf8(bytes) {
        Ok(html) => html,
        Err(error) => return error.into_bytes(),
    };
    let script = live_reload_script(generation);
    if let Some(position) = html.rfind("</body>") {
        let mut output = String::with_capacity(html.len() + script.len());
        output.push_str(&html[..position]);
        output.push_str(&script);
        output.push_str(&html[position..]);
        output.into_bytes()
    } else {
        (html + &script).into_bytes()
    }
}

fn live_reload_script(generation: u64) -> String {
    format!(
        r#"<script data-pliego-reload>(()=>{{const route=()=>location.pathname.endsWith('/')?location.pathname:location.pathname.replace(/\/index\.html$/,'/');const connect=g=>{{const stream=new EventSource('/_pliego/reload?since='+g);stream.addEventListener('pliego',async event=>{{stream.close();let update;try{{update=JSON.parse(event.data)}}catch(_){{location.reload();return}}try{{if(update.kind==='none'){{connect(update.generation);return}}if(update.kind==='css'){{for(const link of document.querySelectorAll('link[rel="stylesheet"]')){{const url=new URL(link.href,location.href);if(update.paths.includes(url.pathname)){{url.searchParams.set('pliego-hmr',update.generation);link.href=url.href}}}}document.dispatchEvent(new CustomEvent('pliego:css-hmr',{{detail:update}}));connect(update.generation);return}}if(update.kind==='content'&&update.routes.includes(route())){{const response=await fetch(location.href,{{cache:'no-store',headers:{{'X-Pliego-HMR':'1'}}}});if(!response.ok)throw new Error('content fetch failed');const next=new DOMParser().parseFromString(await response.text(),'text/html');if(!next.body)throw new Error('content body missing');document.body.dispatchEvent(new CustomEvent('pliego:scope-dispose',{{bubbles:true,detail:{{reason:'content-hmr'}}}}));document.title=next.title;document.body.replaceWith(next.body);document.dispatchEvent(new CustomEvent('pliego:content-hmr',{{detail:update}}));connect(update.generation);return}}if(update.kind==='content'){{connect(update.generation);return}}if(update.kind==='adapter'){{const signal=new CustomEvent('pliego:adapter-hmr',{{cancelable:true,detail:update}});document.dispatchEvent(signal);if(signal.defaultPrevented){{connect(update.generation);return}}}}location.reload()}}catch(_){{location.reload()}}}});}};connect({generation})}})()</script>"#
    )
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        Some("webp") => "image/webp",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json" | "webmanifest") => "application/json; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        Some("mp4") => "video/mp4",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_delegation_defaults_to_src_without_overriding_explicit_sources() {
        assert_eq!(
            css_check_arguments(vec!["--seed".to_owned()]),
            ["--source", "src", "--seed"]
        );
        assert_eq!(
            css_check_arguments(vec!["--source=styles".to_owned(), "--seed".to_owned()]),
            ["--source=styles", "--seed"]
        );
        assert_eq!(
            css_check_arguments(vec![
                "--source".to_owned(),
                "styles".to_owned(),
                "--seed".to_owned(),
            ]),
            ["--source", "styles", "--seed"]
        );
    }

    #[test]
    fn preview_serves_plain_text_with_a_text_mime_type() {
        assert_eq!(
            mime_for(Path::new(".well-known/security.txt")),
            "text/plain; charset=utf-8"
        );
    }

    #[test]
    fn project_manifest_is_generic_and_strict() {
        let manifest: ProjectManifest = toml::from_str(
            r#"
                [project]
                id = "example-site"
                name = "Example"
                site_package = "example-site"
                output = "target/example"

                [client]
                package = "example-client"
                wasm_name = "example_client"
                bindgen_output = "target/example-client/pkg"
            "#,
        )
        .expect("valid manifest");
        assert_eq!(manifest.project.site_package, "example-site");
        assert!(validate_manifest(&manifest).is_ok());
    }

    #[test]
    fn project_manifest_rejects_parent_paths() {
        let manifest: ProjectManifest = toml::from_str(
            r#"
                [project]
                id = "bad-site"
                name = "Bad"
                site_package = "bad-site"
                output = "../outside"
            "#,
        )
        .expect("parseable manifest");
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn project_manifest_requires_a_stable_portable_owner_id() {
        for id in ["pliego-site", "site2"] {
            let manifest: ProjectManifest = toml::from_str(&format!(
                r#"
                    [project]
                    id = "{id}"
                    name = "Portable"
                    site_package = "portable-site"
                    output = "target/site"
                "#
            ))
            .expect("parseable manifest");
            assert!(
                validate_manifest(&manifest).is_ok(),
                "valid id rejected: {id}"
            );
        }

        for id in ["", "Uppercase", "starts_with_underscore", "two words"] {
            let manifest: ProjectManifest = toml::from_str(&format!(
                r#"
                    [project]
                    id = "{id}"
                    name = "Portable"
                    site_package = "portable-site"
                    output = "target/site"
                "#
            ))
            .expect("parseable manifest");
            assert!(
                validate_manifest(&manifest).is_err(),
                "non-portable id accepted: {id:?}"
            );
        }
    }

    #[test]
    fn generated_paths_cannot_target_project_sources_or_windows_prefixes() {
        assert!(validate_generated_path(Path::new("target/site"), "output").is_ok());
        for path in [
            ".",
            "target",
            "src",
            "../outside",
            r"\outside",
            "C:outside",
            "TARGET/site",
            "target/CON",
            "target/site.",
            "target/cafe\u{301}",
        ] {
            assert!(
                validate_generated_path(Path::new(path), "output").is_err(),
                "unsafe generated path accepted: {path}"
            );
        }
    }

    #[test]
    fn generated_manifest_paths_are_pairwise_disjoint_from_reserved_state() {
        let manifest = |output: &str, bindgen: &str| {
            toml::from_str::<ProjectManifest>(&format!(
                r#"
                    [project]
                    id = "path-proof"
                    name = "Path Proof"
                    site_package = "path-proof"
                    output = "{output}"

                    [client]
                    package = "path-proof-client"
                    wasm_name = "path_proof_client"
                    bindgen_output = "{bindgen}"
                "#
            ))
            .expect("parse manifest fixture")
        };
        assert!(validate_manifest(&manifest("target/site", "target/client/pkg")).is_ok());
        for (output, bindgen) in [
            ("target/.pliego", "target/client"),
            ("target/.pliego/site", "target/client"),
            ("target/site", "target/.pliego/cache"),
            ("target/.PLIEGO/site", "target/client"),
            ("target/site", "target/site/client"),
            ("target/site/client", "target/site"),
            ("target/Site", "target/site/client"),
            ("target/Stra\u{df}e", "target/STRASSE/client"),
            ("target/\u{ff26}\u{ff2f}\u{ff2f}", "target/foo/client"),
        ] {
            assert!(
                validate_manifest(&manifest(output, bindgen)).is_err(),
                "overlapping generated paths were accepted: {output}, {bindgen}"
            );
        }
    }

    #[test]
    fn command_output_uses_the_requested_project_root() {
        let root = std::env::temp_dir().join(format!(
            "pliego-command-root-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let canonical = root.canonicalize().unwrap();

        #[cfg(windows)]
        let reported = command_output(
            &canonical,
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-Location).Path",
            ],
        )
        .unwrap();
        #[cfg(not(windows))]
        let reported = command_output(&canonical, "pwd", &[]).unwrap();

        let reported = reported
            .strip_prefix("Microsoft.PowerShell.Core\\FileSystem::")
            .unwrap_or(&reported);
        assert_eq!(
            cargo_path(reported).to_ascii_lowercase(),
            cargo_path(canonical.to_str().unwrap()).to_ascii_lowercase(),
            "tool discovery must run from the discovered project root"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reproducible_commands_reject_undeclared_cargo_and_rustc_overrides() {
        for name in [
            "RUSTC",
            "rustc_wrapper",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTC_BOOTSTRAP",
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_INCREMENTAL",
            "CARGO_BUILD_TARGET",
            "CARGO_PROFILE_RELEASE_LTO",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
        ] {
            assert!(
                forbidden_build_environment_name(OsStr::new(name)),
                "override was accepted: {name}"
            );
        }
        for name in ["CARGO_TARGET_DIR", "CARGO_HOME", "RUSTUP_TOOLCHAIN", "PATH"] {
            assert!(
                !forbidden_build_environment_name(OsStr::new(name)),
                "supported environment was rejected: {name}"
            );
        }
    }

    #[test]
    fn loaded_context_requires_a_canonical_root_and_current_manifest() {
        let base = std::env::temp_dir().join(format!(
            "pliego-context-proof-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join(PROJECT_FILE),
            "[project]\nid = \"context-proof\"\nname = \"Proof\"\nsite_package = \"proof\"\noutput = \"target/site\"\n",
        )
        .unwrap();
        let root = base.canonicalize().unwrap();
        let manifest: ProjectManifest =
            toml::from_str(&fs::read_to_string(root.join(PROJECT_FILE)).unwrap()).unwrap();
        let context = Context {
            root: root.clone(),
            manifest: manifest.clone(),
        };
        validate_loaded_context(&context).expect("canonical current context");

        let noncanonical = Context {
            root: PathBuf::from("."),
            manifest: manifest.clone(),
        };
        assert!(validate_loaded_context(&noncanonical).is_err());

        fs::write(
            root.join(PROJECT_FILE),
            "[project]\nid = \"context-proof\"\nname = \"Changed\"\nsite_package = \"proof\"\noutput = \"target/site\"\n",
        )
        .unwrap();
        assert!(validate_loaded_context(&context).is_err());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn generated_path_validation_rejects_linked_ancestors() {
        let base = std::env::temp_dir().join(format!(
            "pliego-generated-link-proof-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = base.join("project");
        let outside = base.join("outside");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, project.join("target"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, project.join("target"));
        if linked.is_err() {
            let _ = fs::remove_dir_all(base);
            return;
        }
        let project = project.canonicalize().unwrap();
        assert!(
            validate_existing_generated_ancestors(
                &project,
                Path::new("target/site"),
                "project.output"
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn servers_are_loopback_only_unless_exposure_is_explicit() {
        let local = parse_server_options(Vec::new(), 4400).unwrap();
        assert_eq!(local.port, 4400);
        assert_eq!(local.host, IpAddr::V4(Ipv4Addr::LOCALHOST));
        let lan = parse_server_options(vec!["5100".into(), "--lan".into()], 4400).unwrap();
        assert_eq!(lan.port, 5100);
        assert_eq!(lan.host, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let explicit =
            parse_server_options(vec!["--host=192.168.1.8".into(), "5200".into()], 4400).unwrap();
        assert_eq!(explicit.host, "192.168.1.8".parse::<IpAddr>().unwrap());
        assert!(
            parse_server_options(vec!["--lan".into(), "--host=127.0.0.1".into()], 4400).is_err()
        );
        assert!(parse_server_options(vec!["--host".into()], 4400).is_err());
    }

    #[test]
    fn development_server_can_watch_an_output_root_before_it_exists() {
        let base = std::env::temp_dir().join(format!(
            "pliego-missing-output-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        let requested = base.join("target/site");
        let canonical = canonical_server_root(&requested, true).unwrap();
        assert!(!requested.exists());
        assert_eq!(canonical, base.canonicalize().unwrap().join("target/site"));
        assert!(canonical_server_root(&requested, false).is_err());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn new_options_accept_path_and_display_name_in_any_order() {
        let options = parse_new_options(vec![
            "--name".to_owned(),
            "Editorial Proof".to_owned(),
            "proof-site".to_owned(),
            "--framework-path".to_owned(),
            "../pliegors".to_owned(),
        ])
        .expect("valid new options");
        assert_eq!(options.destination, Some(PathBuf::from("proof-site")));
        assert_eq!(options.display_name.as_deref(), Some("Editorial Proof"));
        assert_eq!(options.framework_path, Some(PathBuf::from("../pliegors")));
    }

    #[test]
    fn package_names_are_normalized_and_must_start_with_a_letter() {
        assert_eq!(package_name("My New_Pliego").unwrap(), "my-new-pliego");
        assert!(package_name("2026 edition").is_err());
    }

    #[test]
    fn windows_verbatim_paths_are_valid_cargo_paths() {
        assert_eq!(
            cargo_path(r"\\?\C:\workspace\pliegors\crates\pliego-dom"),
            "C:/workspace/pliegors/crates/pliego-dom"
        );
    }

    #[test]
    fn released_starters_pin_the_exact_registry_version() {
        let dependency = registry_dependency();
        assert_eq!(
            dependency,
            format!("{{ version = \"={}\" }}", env!("CARGO_PKG_VERSION"))
        );
        assert!(!dependency.contains("git ="));
        assert!(!dependency.contains("path ="));
    }

    #[test]
    fn live_reload_is_injected_before_body_close() {
        let html = inject_live_reload(b"<html><body>Ready</body></html>".to_vec(), 7);
        let html = String::from_utf8(html).unwrap();
        assert!(html.contains("/_pliego/reload?since="));
        assert!(html.contains("connect(7)"));
        assert!(html.contains("pliego:content-hmr"));
        assert!(html.contains("pliego:adapter-hmr"));
        assert!(html.contains("pliego:css-hmr"));
        assert!(html.find("data-pliego-reload").unwrap() < html.find("</body>").unwrap());
    }

    #[test]
    fn development_failure_surface_is_safe_and_recovers_by_generation() {
        let state = DevState::new(Some(
            "error: unexpected <script>alert('owned')</script>\u{1b}[31m".to_owned(),
        ));
        let html = String::from_utf8(development_error_document(
            &state.failure().unwrap(),
            state.generation.load(Ordering::SeqCst),
        ))
        .unwrap();
        assert!(html.contains("PLG-BLD-001"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains('\u{1b}'));
        assert!(html.contains("/_pliego/reload?since="));
        assert!(html.contains("connect(0)"));

        state.publish(None, HmrUpdate::reload());
        assert!(state.failure().is_none());
        assert_eq!(state.generation.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn document_requests_receive_diagnostics_but_assets_do_not() {
        for target in ["/", "/guide", "/guide/", "/index.html"] {
            assert!(should_render_development_diagnostic(target), "{target}");
        }
        for target in ["/assets/site.css", "/favicon.svg", "/app.wasm"] {
            assert!(!should_render_development_diagnostic(target), "{target}");
        }
    }

    #[test]
    fn fallback_http_pages_are_branded_and_escape_the_request() {
        let not_found =
            String::from_utf8(not_found_document("/<script>alert(1)</script>")).unwrap();
        assert!(not_found.contains("PLG-HTTP-404"));
        assert!(not_found.contains("&lt;script&gt;"));
        assert!(!not_found.contains("<script>alert"));
        let internal = String::from_utf8(internal_server_error_document()).unwrap();
        assert!(internal.contains("PLG-HTTP-500"));
    }

    #[test]
    fn preview_resolution_rejects_escape_and_oversized_targets() {
        let root = std::env::temp_dir().join(format!(
            "pliego-preview-root-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("guide")).unwrap();
        fs::write(root.join("index.html"), b"home").unwrap();
        fs::write(root.join("guide/index.html"), b"guide").unwrap();
        fs::write(root.join("404.html"), b"missing").unwrap();
        let root = root.canonicalize().unwrap();

        let (home, status) = resolve_request_file(&root, "/");
        assert_eq!(status, StatusCode(200));
        assert_eq!(home, root.join("index.html"));
        let (guide, status) = resolve_request_file(&root, "/guide");
        assert_eq!(status, StatusCode(200));
        assert_eq!(guide, root.join("guide/index.html"));
        for target in ["/../secret", r"/..\secret"] {
            let (_, status) = resolve_request_file(&root, target);
            assert_eq!(status, StatusCode(404));
        }
        let oversized = format!("/{}", "a".repeat(MAX_REQUEST_TARGET_BYTES + 1));
        assert_eq!(resolve_request_file(&root, &oversized).1, StatusCode(404));
        let outside = root.with_extension("outside.txt");
        fs::write(&outside, b"private").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, root.join("linked.txt"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("linked.txt"));
        if linked.is_ok() {
            assert_eq!(
                resolve_request_file(&root, "/linked.txt").1,
                StatusCode(404)
            );
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn reload_long_poll_routing_is_exact_and_target_bounded() {
        assert!(is_reload_target("/_pliego/reload"));
        assert!(is_reload_target("/_pliego/reload?since=7"));
        assert!(!is_reload_target("/_pliego/reload-attacker?since=7"));
        assert!(!is_reload_target("/_pliego/reload/nested"));
        assert!(!is_reload_target(&format!(
            "/_pliego/reload?{}",
            "a".repeat(MAX_REQUEST_TARGET_BYTES)
        )));
    }

    #[test]
    fn development_snapshot_ignores_generated_directories() {
        assert!(should_ignore_directory(
            Path::new("target"),
            Path::new("target/site")
        ));
        assert!(should_ignore_directory(
            Path::new("node_modules/pkg"),
            Path::new("dist")
        ));
        assert!(should_ignore_directory(
            Path::new("dist"),
            Path::new("dist")
        ));
        assert!(!should_ignore_directory(
            Path::new("src"),
            Path::new("dist")
        ));
        assert!(!should_ignore_directory(
            Path::new("src/target"),
            Path::new("dist")
        ));
        assert!(!should_ignore_directory(
            Path::new("src/node_modules"),
            Path::new("dist")
        ));
    }

    #[test]
    fn development_snapshot_ignores_atomic_publication_coordination_files() {
        let token = "a".repeat(64);
        assert!(should_ignore_development_file(Path::new(&format!(
            "assets/.pliego-{token}.lock"
        ))));
        assert!(should_ignore_development_file(Path::new(&format!(
            "assets/.pliego-{token}-stage-123-0"
        ))));
        assert!(should_ignore_development_file(Path::new(&format!(
            "assets/.pliego-{token}-backup-123-0"
        ))));
        assert!(!should_ignore_development_file(Path::new(
            "assets/pliego.css"
        )));
        assert!(!should_ignore_development_file(Path::new(
            "assets/.keep.tmp"
        )));
        assert!(!should_ignore_development_file(Path::new(
            "assets/.pliego.css.pliego.lock"
        )));
        assert!(!should_ignore_development_file(Path::new(&format!(
            "assets/.pliego-{}-stage-123-0",
            "A".repeat(64)
        ))));
    }

    #[test]
    fn native_watch_paths_track_nested_target_but_ignore_generated_roots() {
        let root = std::env::temp_dir().join(format!(
            "pliego-watch-root-only-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut changed = BTreeSet::new();
        collect_watch_path(
            root.join("src/target/input.txt"),
            &root,
            Path::new("dist"),
            &mut changed,
        )
        .unwrap();
        collect_watch_path(
            root.join("target/generated.txt"),
            &root,
            Path::new("dist"),
            &mut changed,
        )
        .unwrap();
        collect_watch_path(
            root.join("dist/index.html"),
            &root,
            Path::new("dist"),
            &mut changed,
        )
        .unwrap();
        assert_eq!(changed, BTreeSet::from(["src/target/input.txt".to_owned()]));
    }

    #[test]
    fn stable_failure_codes_match_the_cli_contract() {
        assert_eq!(FailureKind::Usage.code(), 2);
        assert_eq!(FailureKind::Project.code(), 3);
        assert_eq!(FailureKind::Scaffold.code(), 3);
        assert_eq!(FailureKind::Check.code(), 4);
        assert_eq!(FailureKind::Build.code(), 5);
        assert_eq!(FailureKind::Artifact.code(), 6);
        assert_eq!(FailureKind::Server.code(), 7);
    }

    #[test]
    fn diagnostics_extract_compiler_and_manifest_spans_with_bounded_fixes() {
        let compiler = "error[E0000]: example\n  --> C:\\work\\src\\main.rs:12:7\nhelp: replace the invalid expression";
        assert_eq!(
            diagnostic_spans(compiler),
            vec![DiagnosticSpan {
                file: Some(r"C:\work\src\main.rs".to_owned()),
                line: 12,
                column: 7,
                label: "compiler primary",
            }]
        );
        let failure = CliFailure::new(FailureKind::Build, compiler.to_owned());
        assert!(
            diagnostic_fixes(&failure)
                .iter()
                .any(|fix| fix.message == "replace the invalid expression")
        );

        let manifest = r"C:\work\pliego.toml: TOML parse error at line 4, column 9";
        assert_eq!(diagnostic_spans(manifest)[0].line, 4);
        assert_eq!(diagnostic_spans(manifest)[0].column, 9);
    }

    #[test]
    fn global_diagnostic_format_is_removed_from_command_arguments() {
        let (format, arguments) = parse_global_options(vec![
            "build".to_owned(),
            "--diagnostic-format=json".to_owned(),
        ])
        .unwrap();
        assert_eq!(format, DiagnosticFormat::Json);
        assert_eq!(arguments, vec!["build"]);
    }

    #[test]
    fn template_paths_must_be_portable() {
        assert!(validate_template_path(Path::new("assets/image.webp")).is_ok());
        assert!(validate_template_path(Path::new("../escape")).is_err());
        assert!(validate_template_path(Path::new("assets/CON.txt")).is_err());
        assert!(validate_template_path(Path::new("assets/trailing.")).is_err());
        assert!(validate_template_path(Path::new("assets/bad:name")).is_err());
    }

    #[test]
    fn unresolved_render_tokens_are_rejected() {
        assert!(reject_unresolved_tokens(Path::new("README.md"), b"# __NAME__").is_err());
        assert!(reject_unresolved_tokens(Path::new("image.bin"), &[0xff, 0x00]).is_ok());
    }
}
