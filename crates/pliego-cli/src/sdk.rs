// SPDX-License-Identifier: Apache-2.0

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, OpenOptions};
use pliego_sdk::{
    BuildTransformExecution, BuildTransformInput, Capability, CapabilityPolicy, ComponentHost,
    ComponentHostReceipt, EntryKind, ExtensionManifest, HostContract, McpHost, OPENSDK_API_VERSION,
    RpcHost, RpcRequest, ValidatedExtension, preview_compatibility_matrix,
};
use semver::Version;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::str::FromStr;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TRANSFORM_INPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RPC_LINE_BYTES: u64 = 1024 * 1024;
const MAX_RPC_MESSAGES: usize = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Check,
    Test,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Format {
    Human,
    Json,
}

#[derive(Debug)]
struct Options {
    action: Action,
    manifest: PathBuf,
    grants: Vec<Capability>,
    features: Vec<String>,
    format: Format,
    input: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct CommandError {
    usage: bool,
    message: String,
}

impl CommandError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            usage: true,
            message: message.into(),
        }
    }

    fn sdk(message: impl Into<String>) -> Self {
        Self {
            usage: false,
            message: message.into(),
        }
    }

    pub(crate) fn into_parts(self) -> (bool, String) {
        (self.usage, self.message)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConformanceReport<'a> {
    contract: &'static str,
    level: &'static str,
    result: &'static str,
    manifest_path: String,
    manifest_sha256: String,
    entry_sha256: String,
    admission: &'a pliego_sdk::AdmissionReceipt,
    #[serde(skip_serializing_if = "Option::is_none")]
    component_host: Option<ComponentHostReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_transform: Option<BuildTransformExecution>,
    checks: Vec<&'static str>,
}

pub(crate) fn run(arguments: Vec<String>) -> Result<(), CommandError> {
    if arguments.first().map(String::as_str) == Some("tooling-host") {
        return serve_tooling(&arguments[1..]);
    }
    if arguments.first().map(String::as_str) == Some("compatibility") {
        return print_compatibility(&arguments[1..]);
    }
    let options = parse(arguments)?;
    let manifest_path = absolute(&options.manifest)?;
    let manifest_parent = manifest_path
        .parent()
        .ok_or_else(|| CommandError::sdk("manifest has no parent directory"))?;
    let manifest_name = manifest_path
        .file_name()
        .ok_or_else(|| CommandError::sdk("manifest has no file name"))?;
    let manifest_root =
        Dir::open_ambient_dir(manifest_parent, ambient_authority()).map_err(|error| {
            CommandError::sdk(format!(
                "cannot open manifest root {}: {error}",
                manifest_parent.display()
            ))
        })?;
    let manifest_bytes = read_bounded_at(
        &manifest_root,
        Path::new(manifest_name),
        MAX_MANIFEST_BYTES,
        "manifest",
    )?;
    let manifest: ExtensionManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        CommandError::sdk(format!("cannot parse {}: {error}", manifest_path.display()))
    })?;
    let entry_bytes = read_bounded_at(
        &manifest_root,
        Path::new(&manifest.entry.path),
        MAX_ENTRY_BYTES,
        "entry",
    )?;

    let mut policy = CapabilityPolicy::deny_all();
    for capability in options.grants {
        policy = policy.grant(capability);
    }
    let mut host = HostContract::preview(
        Version::parse(OPENSDK_API_VERSION).expect("built-in OpenSDK version is valid"),
        policy,
    );
    for feature in options.features {
        host = host.with_feature(feature);
    }
    let admitted = match manifest.entry.kind {
        EntryKind::WasmComponent => host.admit_component(manifest, &entry_bytes),
        EntryKind::BrowserEsm | EntryKind::JsonRpcProcess => host.admit(manifest, &entry_bytes),
    }
    .map_err(|error| CommandError::sdk(error.to_string()))?;

    let (level, component_host, build_transform, checks) = match options.action {
        Action::Check => (
            "admission",
            None,
            None,
            vec![
                "schema",
                "identity",
                "compatibility",
                "digest",
                "capabilities",
            ],
        ),
        Action::Test => {
            let second = admit_again(&host, admitted.manifest().clone(), &entry_bytes)?;
            if second.receipt() != admitted.receipt() {
                return Err(CommandError::sdk(
                    "admission is not deterministic across repeated runs",
                ));
            }
            let transform_input = if let Some(path) = options.input.as_ref() {
                if admitted.manifest().entry.kind != EntryKind::WasmComponent
                    || admitted.manifest().entry.world.as_deref()
                        != Some("pliego:build/transformer@0.1.0")
                {
                    return Err(CommandError::sdk(
                        "--input requires world pliego:build/transformer@0.1.0",
                    ));
                }
                Some(read_transform_input(path)?)
            } else {
                None
            };
            let runtime = ComponentHost::deny_by_default()
                .map_err(|error| CommandError::sdk(error.to_string()))?;
            let component_host = if admitted.manifest().entry.kind == EntryKind::WasmComponent
                && transform_input.is_none()
            {
                Some(
                    runtime
                        .instantiate(&admitted, &entry_bytes)
                        .map_err(|error| CommandError::sdk(error.to_string()))?,
                )
            } else {
                None
            };
            let build_transform = transform_input
                .map(|input| {
                    runtime
                        .execute_build_transform(&admitted, &entry_bytes, input)
                        .map_err(|error| CommandError::sdk(error.to_string()))
                })
                .transpose()?;
            let level = if build_transform.is_some() {
                "build-transform-execution"
            } else if component_host.is_some() {
                "component-instantiation"
            } else {
                "admission"
            };
            let mut checks = vec![
                "schema",
                "identity",
                "compatibility",
                "digest",
                "capabilities",
                "component-shape",
                "deterministic-admission",
            ];
            if component_host.is_some() || build_transform.is_some() {
                checks.push("deny-by-default-runtime");
            }
            if build_transform.is_some() {
                checks.push("typed-build-transform");
                checks.push("runtime-budgets");
                checks.push("digest-bound-output");
            }
            (level, component_host, build_transform, checks)
        }
    };
    let report = ConformanceReport {
        contract: "dev.pliegors.sdk-conformance/v1",
        level,
        result: "pass",
        manifest_path: manifest_path.display().to_string(),
        manifest_sha256: format!("sha256:{:x}", Sha256::digest(&manifest_bytes)),
        entry_sha256: format!("sha256:{:x}", Sha256::digest(&entry_bytes)),
        admission: admitted.receipt(),
        component_host,
        build_transform,
        checks,
    };
    match options.format {
        Format::Human => println!(
            "PLIEGO SDK {} PASS: {} ({})",
            match options.action {
                Action::Check => "check",
                Action::Test => "test",
            },
            admitted.receipt().extension,
            report.level
        ),
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| CommandError::sdk(format!("cannot encode SDK report: {error}")))?
        ),
    }
    Ok(())
}

fn admit_again(
    host: &HostContract,
    manifest: ExtensionManifest,
    bytes: &[u8],
) -> Result<ValidatedExtension, CommandError> {
    match manifest.entry.kind {
        EntryKind::WasmComponent => host.admit_component(manifest, bytes),
        EntryKind::BrowserEsm | EntryKind::JsonRpcProcess => host.admit(manifest, bytes),
    }
    .map_err(|error| CommandError::sdk(error.to_string()))
}

fn parse(arguments: Vec<String>) -> Result<Options, CommandError> {
    let mut arguments = arguments.into_iter();
    let action = match arguments.next().as_deref() {
        Some("check") => Action::Check,
        Some("test") => Action::Test,
        _ => return Err(CommandError::usage(usage())),
    };
    let manifest = arguments
        .next()
        .filter(|value| !value.starts_with('-'))
        .map(PathBuf::from)
        .ok_or_else(|| CommandError::usage(usage()))?;
    let mut grants = Vec::new();
    let mut features = Vec::new();
    let mut format = Format::Human;
    let mut input = None;
    while let Some(argument) = arguments.next() {
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });
        let value = |arguments: &mut std::vec::IntoIter<String>| {
            inline
                .map(ToOwned::to_owned)
                .or_else(|| arguments.next())
                .ok_or_else(|| CommandError::usage(format!("{name} requires a value")))
        };
        match name {
            "--grant" => {
                let capability =
                    Capability::from_str(&value(&mut arguments)?).map_err(CommandError::usage)?;
                grants.push(capability);
            }
            "--feature" => features.push(validate_feature(value(&mut arguments)?)?),
            "--format" => {
                format = match value(&mut arguments)?.as_str() {
                    "human" => Format::Human,
                    "json" => Format::Json,
                    _ => return Err(CommandError::usage("--format must be `human` or `json`")),
                }
            }
            "--input" => input = Some(PathBuf::from(value(&mut arguments)?)),
            _ => {
                return Err(CommandError::usage(format!(
                    "unknown SDK option `{argument}`"
                )));
            }
        }
    }
    grants.sort();
    grants.dedup();
    features.sort();
    features.dedup();
    if action == Action::Check && input.is_some() {
        return Err(CommandError::usage("--input is only valid with `sdk test`"));
    }
    Ok(Options {
        action,
        manifest,
        grants,
        features,
        format,
        input,
    })
}

fn read_transform_input(path: &Path) -> Result<BuildTransformInput, CommandError> {
    let path = absolute(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| CommandError::sdk("transform input has no parent directory"))?;
    let name = path
        .file_name()
        .ok_or_else(|| CommandError::sdk("transform input has no file name"))?;
    let root = Dir::open_ambient_dir(parent, ambient_authority()).map_err(|error| {
        CommandError::sdk(format!(
            "cannot open transform input root {}: {error}",
            parent.display()
        ))
    })?;
    let bytes = read_bounded_at(
        &root,
        Path::new(name),
        MAX_TRANSFORM_INPUT_BYTES,
        "transform input",
    )?;
    serde_json::from_slice(&bytes)
        .map_err(|error| CommandError::sdk(format!("cannot parse {}: {error}", path.display())))
}

fn validate_feature(value: String) -> Result<String, CommandError> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(CommandError::usage(
            "feature IDs use lowercase ASCII letters, digits, and interior hyphens",
        ));
    }
    Ok(value)
}

fn absolute(path: &Path) -> Result<PathBuf, CommandError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| CommandError::sdk(format!("cannot read current directory: {error}")))
    }
}

fn read_bounded_at(
    root: &Dir,
    path: &Path,
    ceiling: u64,
    label: &str,
) -> Result<Vec<u8>, CommandError> {
    let components = path
        .components()
        .map(|component| match component {
            PathComponent::Normal(value) => Ok(value),
            _ => Err(CommandError::sdk(format!(
                "{label} path must contain only normalized relative components: {}",
                path.display()
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (name, parents) = components.split_last().ok_or_else(|| {
        CommandError::sdk(format!("{label} path cannot be empty: {}", path.display()))
    })?;
    let mut directory = root.try_clone().map_err(|error| {
        CommandError::sdk(format!(
            "cannot clone {label} root for {}: {error}",
            path.display()
        ))
    })?;
    for component in parents {
        directory = directory.open_dir_nofollow(component).map_err(|error| {
            CommandError::sdk(format!(
                "cannot open non-symlink {label} ancestor {}: {error}",
                path.display()
            ))
        })?;
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory.open_with(name, &options).map_err(|error| {
        CommandError::sdk(format!("cannot open {label} {}: {error}", path.display()))
    })?;
    let metadata = file.metadata().map_err(|error| {
        CommandError::sdk(format!(
            "cannot inspect opened {label} {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(CommandError::sdk(format!(
            "{label} must be a regular non-symlink file: {}",
            path.display()
        )));
    }
    if metadata.len() > ceiling {
        return Err(CommandError::sdk(format!(
            "{label} exceeds {ceiling} bytes: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(ceiling + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CommandError::sdk(format!("cannot read {label} {}: {error}", path.display()))
        })?;
    if bytes.len() as u64 > ceiling {
        return Err(CommandError::sdk(format!(
            "{label} grew beyond {ceiling} bytes while reading: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn usage() -> &'static str {
    "usage: pliego sdk <check|test> <manifest> [--input <transform.json>] [--grant <capability>] [--feature <id>] [--format <human|json>] | pliego sdk compatibility [--format <human|json>] | pliego sdk tooling-host --protocol <pliego|mcp> [--feature <id>]"
}

fn print_compatibility(arguments: &[String]) -> Result<(), CommandError> {
    let format = match arguments {
        [] => Format::Human,
        [flag, value] if flag == "--format" => match value.as_str() {
            "human" => Format::Human,
            "json" => Format::Json,
            _ => return Err(CommandError::usage("--format must be `human` or `json`")),
        },
        [argument] if argument.starts_with("--format=") => match &argument[9..] {
            "human" => Format::Human,
            "json" => Format::Json,
            _ => return Err(CommandError::usage("--format must be `human` or `json`")),
        },
        _ => {
            return Err(CommandError::usage(
                "usage: pliego sdk compatibility [--format <human|json>]",
            ));
        }
    };
    let matrix = preview_compatibility_matrix();
    matrix.validate().map_err(|error| {
        CommandError::sdk(format!("invalid embedded compatibility matrix: {error}"))
    })?;
    match format {
        Format::Human => println!(
            "PliegoRS OpenSDK {}: {} host, {} toolchains, {} surfaces, {} deprecations",
            matrix.protocol_version,
            matrix.hosts.len(),
            matrix.toolchains.len(),
            matrix.surfaces.len(),
            matrix.deprecations.len()
        ),
        Format::Json => println!(
            "{}",
            serde_json::to_string_pretty(&matrix).map_err(|error| {
                CommandError::sdk(format!("cannot encode compatibility matrix: {error}"))
            })?
        ),
    }
    Ok(())
}

fn serve_tooling(arguments: &[String]) -> Result<(), CommandError> {
    let mut protocol = None;
    let mut features = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });
        let value = if let Some(value) = inline {
            value.to_owned()
        } else {
            index += 1;
            arguments
                .get(index)
                .cloned()
                .ok_or_else(|| CommandError::usage(format!("{name} requires a value")))?
        };
        match name {
            "--protocol" => {
                if protocol.is_some() {
                    return Err(CommandError::usage("--protocol may only be provided once"));
                }
                protocol = Some(match value.as_str() {
                    "pliego" => ToolingProtocol::Pliego,
                    "mcp" => ToolingProtocol::Mcp,
                    _ => return Err(CommandError::usage("--protocol must be `pliego` or `mcp`")),
                });
            }
            "--feature" => features.push(validate_feature(value)?),
            _ => {
                return Err(CommandError::usage(format!(
                    "unknown tooling option `{argument}`"
                )));
            }
        }
        index += 1;
    }
    let protocol =
        protocol.ok_or_else(|| CommandError::usage("tooling-host requires --protocol"))?;
    features.sort();
    features.dedup();
    let mut pliego = RpcHost::new(OPENSDK_API_VERSION);
    let mut mcp = McpHost::new(OPENSDK_API_VERSION);
    for feature in features {
        pliego = pliego.with_feature(feature.clone());
        mcp = mcp.with_feature(feature);
    }

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    for _ in 0..MAX_RPC_MESSAGES {
        let Some(line) = read_rpc_line(&mut reader)? else {
            return Ok(());
        };
        let request = match serde_json::from_slice::<RpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                let code = if serde_json::from_slice::<serde_json::Value>(&line).is_ok() {
                    -32600
                } else {
                    -32700
                };
                write_rpc_value(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": code, "message": error.to_string() },
                    }),
                )?;
                continue;
            }
        };
        let response = match protocol {
            ToolingProtocol::Pliego => pliego.handle(request),
            ToolingProtocol::Mcp => mcp.handle(request),
        };
        if let Some(response) = response {
            write_rpc_value(&mut writer, &response)?;
        }
    }
    Err(CommandError::sdk(format!(
        "tooling host exceeded {MAX_RPC_MESSAGES} messages"
    )))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolingProtocol {
    Pliego,
    Mcp,
}

fn read_rpc_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, CommandError> {
    let mut line = Vec::new();
    let read = Read::by_ref(reader)
        .take(MAX_RPC_LINE_BYTES + 1)
        .read_until(b'\n', &mut line)
        .map_err(|error| CommandError::sdk(format!("cannot read tooling request: {error}")))?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() as u64 > MAX_RPC_LINE_BYTES {
        return Err(CommandError::sdk(format!(
            "tooling request exceeds {MAX_RPC_LINE_BYTES} bytes"
        )));
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
    if line.is_empty() {
        return Err(CommandError::sdk("tooling request cannot be empty"));
    }
    Ok(Some(line))
}

fn write_rpc_value(writer: &mut impl Write, value: &impl Serialize) -> Result<(), CommandError> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|error| CommandError::sdk(format!("cannot encode tooling response: {error}")))?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|error| CommandError::sdk(format!("cannot write tooling response: {error}")))
}
