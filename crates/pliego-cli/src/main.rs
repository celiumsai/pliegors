// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
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

use pliego_starters as templates;

const PROJECT_FILE: &str = "pliego.toml";
const SERVER_WORKERS: usize = 8;
const SERVER_QUEUE: usize = 64;
const RELOAD_WORKERS: usize = 16;
const RELOAD_QUEUE: usize = 32;
const MAX_REQUEST_TARGET_BYTES: usize = 4096;
const MAX_CAPTURED_DIAGNOSTIC_BYTES: usize = 256 * 1024;
const RELOAD_PATH: &str = "/_pliego/reload";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectManifest {
    project: Project,
    client: Option<Client>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    name: String,
    site_package: String,
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
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
    match format {
        DiagnosticFormat::Human => eprintln!(
            "PLIEGO[{}] {}: {}\nhelp: {}",
            error.kind.diagnostic_code(),
            error.kind.label(),
            error.message,
            error.kind.help()
        ),
        DiagnosticFormat::Json => eprintln!(
            "{}",
            serde_json::json!({
                "code": error.kind.diagnostic_code(),
                "exit_code": error.kind.code(),
                "category": error.kind.label(),
                "message": error.message,
                "help": error.kind.help(),
            })
        ),
    }
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

    if !matches!(
        command.as_str(),
        "build" | "check" | "dev" | "preview" | "inspect"
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
            dev(&context, options).map_err(DevFailure::into_cli_failure)
        }
        "preview" => {
            let options = parse_server_options(arguments.collect(), 4400)
                .map_err(|error| CliFailure::new(FailureKind::Usage, error))?;
            let output = context.root.join(&context.manifest.project.output);
            read_build_files(&output.join("pliego.build.json"))
                .map_err(|error| CliFailure::new(FailureKind::Artifact, error))?;
            serve(output, options, "preview")
                .map_err(|error| CliFailure::new(FailureKind::Server, error))
        }
        "inspect" => inspect(
            &context
                .root
                .join(&context.manifest.project.output)
                .join("pliego.build.json"),
        )
        .map_err(|error| CliFailure::new(FailureKind::Artifact, error)),
        _ => unreachable!("known commands were validated before project discovery"),
    }
}

fn print_help() {
    println!(
        "PliegoRS project tool\n\nUSAGE:\n  pliego new <path> [--template <id>] [--name <name>] [--framework-path <path>]\n  pliego templates\n  pliego check\n  pliego build\n  pliego dev [port] [--host <ip>|--lan]\n  pliego preview [port] [--host <ip>|--lan]\n  pliego inspect\n  pliego version\n\nGLOBAL OPTIONS:\n  --diagnostic-format <human|json>\n\nServers bind to 127.0.0.1 unless --host or --lan is explicit.\nThe nearest pliego.toml defines an existing project."
    );
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
    let (dom_dependency, ssg_dependency, source_label) = if let Some(root) = framework_path {
        let root = validate_framework_root(&root)?;
        (
            local_dependency(&root.join("crates/pliego-dom"))?,
            local_dependency(&root.join("crates/pliego-ssg"))?,
            format!("local framework {}", cargo_path(&root.to_string_lossy())),
        )
    } else {
        let dependency = source_dependency()?;
        let revision = source_revision()?;
        (
            dependency.clone(),
            dependency,
            format!("PliegoRS source revision {revision}"),
        )
    };

    let cargo = template
        .cargo_toml
        .replace("__PACKAGE__", &package_name)
        .replace("__DOM_DEPENDENCY__", &dom_dependency)
        .replace("__SSG_DEPENDENCY__", &ssg_dependency);
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
}

fn local_dependency(path: &Path) -> Result<String, String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("dependency path {}: {error}", path.display()))?;
    let path = cargo_path(&path.to_string_lossy());
    Ok(format!("{{ path = \"{}\" }}", toml_escape(&path)))
}

const PLIEGORS_SOURCE_REPOSITORY: &str = "https://github.com/celiumsai/pliegors";
const PLIEGORS_BUILD_SOURCE_REV: &str = env!("PLIEGORS_BUILD_SOURCE_REV");

fn source_revision() -> Result<&'static str, String> {
    validated_source_revision(PLIEGORS_BUILD_SOURCE_REV)
}

fn validated_source_revision(revision: &'static str) -> Result<&'static str, String> {
    if revision.is_empty() {
        return Err(
            "PliegoRS source revision is unavailable; rebuild from the repository or set \
             PLIEGORS_SOURCE_REV to a verified 40-character Git commit SHA"
                .to_owned(),
        );
    }
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("embedded PliegoRS source revision is not a full Git commit SHA".to_owned());
    }
    Ok(revision)
}

fn source_dependency() -> Result<String, String> {
    Ok(format!(
        "{{ git = \"{PLIEGORS_SOURCE_REPOSITORY}\", rev = \"{}\" }}",
        source_revision()?
    ))
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
            let source = fs::read_to_string(&manifest_path)
                .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
            let manifest: ProjectManifest = toml::from_str(&source)
                .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
            validate_manifest(&manifest)?;
            return Ok(Context {
                root: current,
                manifest,
            });
        }
        if !current.pop() {
            return Err(format!(
                "run pliego inside a project containing {PROJECT_FILE}"
            ));
        }
    }
}

fn validate_manifest(manifest: &ProjectManifest) -> Result<(), String> {
    let project = &manifest.project;
    if project.name.trim().is_empty() || project.site_package.trim().is_empty() {
        return Err("project name and site_package cannot be empty".to_owned());
    }
    validate_generated_path(&project.output, "project.output")?;
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
        validate_generated_path(&client.bindgen_output, "client.bindgen_output")?;
    }
    Ok(())
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

fn validate_generated_path(path: &Path, field: &str) -> Result<(), String> {
    validate_relative_path(path, field)?;
    let mut components = path.components();
    let first = components
        .next()
        .and_then(|component| component.as_os_str().to_str());
    if first != Some("target") || components.next().is_none() {
        return Err(format!(
            "{field} must be a generated path below target/, for example target/site"
        ));
    }
    Ok(())
}

fn check(context: &Context) -> Result<(), String> {
    let metadata = cargo_metadata(&context.root)?;
    require_package(&metadata, &context.manifest.project.site_package, "bin")?;
    println!(
        "PLIEGO check: site package `{}`",
        context.manifest.project.site_package
    );

    if let Some(client) = &context.manifest.client {
        require_package(&metadata, &client.package, "cdylib")?;
        require_command("wasm-bindgen", &["--version"])?;
        let targets = command_output("rustup", &["target", "list", "--installed"])?;
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

    println!(
        "PLIEGO check: {} is valid",
        context.root.join(PROJECT_FILE).display()
    );
    Ok(())
}

fn require_package(
    metadata: &serde_json::Value,
    name: &str,
    target_kind: &str,
) -> Result<(), String> {
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == name))
        .ok_or_else(|| {
            format!("Cargo package `{name}` declared in {PROJECT_FILE} was not found")
        })?;
    let has_target = package["targets"].as_array().is_some_and(|targets| {
        targets.iter().any(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == target_kind))
        })
    });
    if has_target {
        Ok(())
    } else {
        Err(format!(
            "Cargo package `{name}` must expose a `{target_kind}` target"
        ))
    }
}

fn require_command(program: &str, arguments: &[&str]) -> Result<(), String> {
    command_output(program, arguments)
        .map(|_| ())
        .map_err(|error| format!("required tool `{program}` is unavailable or failed: {error}"))
}

fn command_output(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
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

fn build(context: &Context) -> Result<(), String> {
    if let Some(client) = &context.manifest.client {
        let cargo_target = cargo_target_directory(&cargo_metadata(&context.root)?)?;
        execute(
            &context.root,
            "cargo",
            &[
                "build",
                "-p",
                &client.package,
                "--target",
                "wasm32-unknown-unknown",
                "--release",
            ],
        )?;
        let bindgen_output = context.root.join(&client.bindgen_output);
        fs::create_dir_all(&bindgen_output).map_err(|error| error.to_string())?;
        let wasm_input = path_argument(
            &cargo_target
                .join("wasm32-unknown-unknown/release")
                .join(format!("{}.wasm", client.wasm_name)),
        )?;
        let bindgen_output = path_argument(&client.bindgen_output)?;
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
    }

    let output = path_argument(&context.manifest.project.output)?;
    execute(
        &context.root,
        "cargo",
        &[
            "run",
            "-p",
            &context.manifest.project.site_package,
            "--",
            &output,
        ],
    )?;
    let ledger = context
        .root
        .join(&context.manifest.project.output)
        .join("pliego.build.json");
    read_build_files(&ledger)?;
    println!(
        "PLIEGO build: {} -> {}",
        context.manifest.project.name,
        context.manifest.project.output.display()
    );
    Ok(())
}

fn cargo_metadata(root: &Path) -> Result<serde_json::Value, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version=1", "--no-deps"])
        .current_dir(root)
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

fn cargo_target_directory(metadata: &serde_json::Value) -> Result<PathBuf, String> {
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata omitted target_directory".to_owned())
}

fn path_argument(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))
}

fn execute(root: &Path, program: &str, arguments: &[&str]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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

fn inspect(manifest: &Path) -> Result<(), String> {
    let files = read_build_files(manifest)?;
    let bytes: u64 = files.iter().filter_map(|file| file["bytes"].as_u64()).sum();
    let html = files
        .iter()
        .filter(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(".html"))
        })
        .count();
    println!(
        "PLIEGO inspect: {html} HTML routes / {} files / {bytes} bytes",
        files.len()
    );
    Ok(())
}

fn read_build_files(manifest: &Path) -> Result<Vec<serde_json::Value>, String> {
    let bytes = fs::read(manifest)
        .map_err(|error| format!("missing build ledger {}: {error}", manifest.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid build ledger {}: {error}", manifest.display()))?;
    value["files"]
        .as_array()
        .cloned()
        .ok_or_else(|| format!("invalid build ledger {}: missing files", manifest.display()))
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
}

impl DevState {
    fn new(build_failure: Option<String>) -> Self {
        Self {
            generation: AtomicU64::new(0),
            build_failure: RwLock::new(build_failure),
        }
    }

    fn publish(&self, build_failure: Option<String>) {
        match self.build_failure.write() {
            Ok(mut current) => *current = build_failure,
            Err(poisoned) => *poisoned.into_inner() = build_failure,
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn failure(&self) -> Option<String> {
        match self.build_failure.read() {
            Ok(current) => current.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

fn dev(context: &Context, options: ServerOptions) -> Result<(), DevFailure> {
    let initial_failure = match build(context) {
        Ok(()) => None,
        Err(error) => {
            eprintln!(
                "PLIEGO[PLG-BLD-001] build: {error}\nhelp: Fix the source and keep this server open; the browser will recover automatically."
            );
            Some(error)
        }
    };
    let root = context.root.join(&context.manifest.project.output);
    let (server, root) = open_server(root, options, "dev", true).map_err(DevFailure::Server)?;
    let state = Arc::new(DevState::new(initial_failure));
    let server_state = Arc::clone(&state);
    std::thread::spawn(move || serve_requests(server, root, Some(server_state)));

    println!("PLIEGO dev: watching {}", context.root.display());
    let mut previous = project_snapshot(&context.root, &context.manifest.project.output)
        .map_err(DevFailure::Project)?;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let current = match project_snapshot(&context.root, &context.manifest.project.output) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("PLIEGO dev: cannot scan project: {error}");
                continue;
            }
        };
        if current == previous {
            continue;
        }
        std::thread::sleep(Duration::from_millis(120));
        let build_input = match project_snapshot(&context.root, &context.manifest.project.output) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("PLIEGO dev: cannot scan project: {error}");
                continue;
            }
        };
        match build(context) {
            Ok(()) => {
                state.publish(None);
                println!("PLIEGO dev: rebuilt");
            }
            Err(error) => {
                eprintln!("PLIEGO[PLG-BLD-001] dev rebuild failed: {error}");
                state.publish(Some(error));
            }
        }
        // Keep the pre-build snapshot so edits made during compilation trigger
        // another pass instead of being silently absorbed.
        previous = build_input;
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FileStamp {
    path: PathBuf,
    modified: SystemTime,
    bytes: u64,
    sha256: [u8; 32],
}

fn project_snapshot(root: &Path, output: &Path) -> Result<Vec<FileStamp>, String> {
    let mut files = Vec::new();
    collect_file_stamps(root, root, output, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_file_stamps(
    root: &Path,
    directory: &Path,
    output: &Path,
    files: &mut Vec<FileStamp>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
        // Publication coordination files are short-lived by design. Filter them by name before
        // querying their type or metadata so a concurrent rename/removal cannot fail the snapshot.
        if should_ignore_development_file(relative) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            if should_ignore_directory(relative, output) {
                continue;
            }
            collect_file_stamps(root, &path, output, files)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata().map_err(|error| error.to_string())?;
            files.push(FileStamp {
                path: relative.to_path_buf(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                bytes: metadata.len(),
                sha256: file_digest(&path)?,
            });
        }
    }
    Ok(())
}

fn file_digest(path: &Path) -> Result<[u8; 32], String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("cannot open {} for fingerprinting: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            format!("cannot read {} for fingerprinting: {error}", path.display())
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

fn should_ignore_directory(relative: &Path, output: &Path) -> bool {
    relative.starts_with(output)
        || relative.components().any(|component| {
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
    name.starts_with('.')
        && (name.ends_with(".pliego.lock")
            || (name.contains(".pliego-") && (name.ends_with(".tmp") || name.ends_with(".bak"))))
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
    if no_store {
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
        format!("data: {current}\n\n")
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
    let reload = generation
        .map(|generation| {
            format!(
                "<script data-pliego-reload>new EventSource('/_pliego/reload?since={generation}').onmessage=()=>location.reload()</script>"
            )
        })
        .unwrap_or_default();
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
    let script = format!(
        r#"<script data-pliego-reload>new EventSource('/_pliego/reload?since={generation}').onmessage=()=>location.reload()</script>"#
    );
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
    fn project_manifest_is_generic_and_strict() {
        let manifest: ProjectManifest = toml::from_str(
            r#"
                [project]
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
                name = "Bad"
                site_package = "bad-site"
                output = "../outside"
            "#,
        )
        .expect("parseable manifest");
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn generated_paths_cannot_target_project_sources_or_windows_prefixes() {
        assert!(validate_generated_path(Path::new("target/site"), "output").is_ok());
        for path in [".", "target", "src", "../outside", r"\outside", "C:outside"] {
            assert!(
                validate_generated_path(Path::new(path), "output").is_err(),
                "unsafe generated path accepted: {path}"
            );
        }
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
    fn released_starters_pin_the_first_party_source_revision() {
        let revision = source_revision().unwrap();
        let dependency = source_dependency().unwrap();
        assert_eq!(revision.len(), 40);
        assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(dependency.contains(PLIEGORS_SOURCE_REPOSITORY));
        assert!(dependency.contains(&format!("rev = \"{revision}\"")));
        assert!(!dependency.contains("version ="));
    }

    #[test]
    fn unverified_source_revisions_fail_closed() {
        assert!(validated_source_revision("").is_err());
        assert!(validated_source_revision("not-a-commit").is_err());
    }

    #[test]
    fn live_reload_is_injected_before_body_close() {
        let html = inject_live_reload(b"<html><body>Ready</body></html>".to_vec(), 7);
        let html = String::from_utf8(html).unwrap();
        assert!(html.contains("/_pliego/reload?since=7"));
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
        assert!(html.contains("/_pliego/reload?since=0"));

        state.publish(None);
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
    }

    #[test]
    fn development_snapshot_ignores_atomic_publication_coordination_files() {
        assert!(should_ignore_development_file(Path::new(
            "assets/.pliego.css.pliego.lock"
        )));
        assert!(should_ignore_development_file(Path::new(
            "assets/.pliego.css.pliego-123-0.tmp"
        )));
        assert!(should_ignore_development_file(Path::new(
            "assets/.pliego.css.pliego-123-0.bak"
        )));
        assert!(!should_ignore_development_file(Path::new(
            "assets/pliego.css"
        )));
        assert!(!should_ignore_development_file(Path::new(
            "assets/.keep.tmp"
        )));
    }

    #[test]
    fn development_fingerprint_detects_same_length_edits() {
        let path = std::env::temp_dir().join(format!(
            "pliego-watch-fingerprint-{}-{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"before").unwrap();
        let before = file_digest(&path).unwrap();
        fs::write(&path, b"change").unwrap();
        let after = file_digest(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_ne!(before, after);
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
