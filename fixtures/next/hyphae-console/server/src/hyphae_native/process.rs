// SPDX-License-Identifier: AGPL-3.0-only

use super::http::{NativeHttpClient, TransportError};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const AUTHORITY_JSON: &str = include_str!("../../../sidecar-authority.json");
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROCESS_OUTPUT: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidecarAuthority {
    contract: String,
    repository: String,
    release_tag: String,
    release_revision: String,
    release_checksums_sha256: String,
    engine_version: String,
    product_api_version: u16,
    native_directory_format: u16,
    transport: String,
    product_media_type: String,
    error_media_type: String,
    rust_msrv: String,
    artifacts: Vec<ArtifactAuthority>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactAuthority {
    target: String,
    archive: String,
    archive_sha256: String,
    executable_sha256: String,
}

impl SidecarAuthority {
    pub fn load() -> Result<Self, AdmissionError> {
        let authority: Self =
            serde_json::from_str(AUTHORITY_JSON).map_err(AdmissionError::Authority)?;
        if authority.contract != "dev.pliegors.hyphae-sidecar-authority/v1"
            || authority.repository != "https://github.com/celiumsai/hyphae"
            || authority.release_tag != "v1.0.1"
            || authority.release_revision != "84161cf067141b60f4847b965ef77c5b749749c0"
            || authority.release_checksums_sha256
                != "3e45b7056f27a3e2062e7d216cf03972108aae032bee36589210eb28cd569048"
            || authority.engine_version != "1.0.1"
            || authority.product_api_version != 1
            || authority.native_directory_format != 1
            || authority.transport != "native-http-v2-loopback"
            || authority.product_media_type != "application/vnd.hyphae.product-v1"
            || authority.error_media_type != "application/vnd.hyphae.error-v1"
            || authority.rust_msrv != "1.89.0"
        {
            return Err(AdmissionError::AuthorityMismatch);
        }
        Ok(authority)
    }

    pub fn release_tag(&self) -> &str {
        &self.release_tag
    }
    pub fn release_revision(&self) -> &str {
        &self.release_revision
    }

    fn artifact_for_current_platform(&self) -> Result<&ArtifactAuthority, AdmissionError> {
        let target = current_target().ok_or(AdmissionError::UnsupportedPlatform)?;
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
            .ok_or(AdmissionError::UnsupportedPlatform)
    }

    pub fn current_artifact(&self) -> Result<&str, AdmissionError> {
        Ok(&self.artifact_for_current_platform()?.archive)
    }
}

#[derive(Clone, Debug)]
pub struct HyphaeInstallation {
    executable: PathBuf,
    authority: SidecarAuthority,
}

impl HyphaeInstallation {
    pub fn admit(executable: &Path, authority: &SidecarAuthority) -> Result<Self, AdmissionError> {
        let details = fs::symlink_metadata(executable).map_err(AdmissionError::Io)?;
        if !details.file_type().is_file() || details.file_type().is_symlink() {
            return Err(AdmissionError::ExecutableType);
        }
        let artifact = authority.artifact_for_current_platform()?;
        if artifact.archive.is_empty() || artifact.archive_sha256.len() != 64 {
            return Err(AdmissionError::AuthorityMismatch);
        }
        let actual = stable_file_sha256(executable)?;
        if actual != artifact.executable_sha256 {
            return Err(AdmissionError::ExecutableDigest {
                expected: artifact.executable_sha256.clone(),
                actual,
            });
        }
        Ok(Self {
            executable: executable.to_path_buf(),
            authority: authority.clone(),
        })
    }

    pub fn verify_version(&self) -> Result<(), AdmissionError> {
        let output = run_bounded(&self.executable, &["version", "--json"], None)?;
        if !output.status.success() {
            return Err(AdmissionError::VersionCommand);
        }
        let version: VersionReport =
            serde_json::from_slice(&output.stdout).map_err(AdmissionError::VersionJson)?;
        if version.product != "hyphae"
            || version.api_version != "v1"
            || version.disk_format_version != 2
            || version.engine_version != self.authority.engine_version
            || version.product_api_version != self.authority.product_api_version
            || version.native_directory_format != self.authority.native_directory_format
        {
            return Err(AdmissionError::VersionMismatch);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionReport {
    api_version: String,
    disk_format_version: u16,
    engine_version: String,
    native_directory_format: u16,
    product: String,
    product_api_version: u16,
}

pub struct HyphaeSidecar {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
    client: NativeHttpClient,
}

impl HyphaeSidecar {
    pub async fn start(
        installation: &HyphaeInstallation,
        data_dir: &Path,
    ) -> Result<Self, SidecarError> {
        installation.verify_version()?;
        if !data_dir.exists() {
            let output = run_bounded(
                &installation.executable,
                &["init", "--data-dir", path_text(data_dir)?],
                None,
            )?;
            if !output.status.success() {
                return Err(SidecarError::Initialization);
            }
        }
        let port = available_port()?;
        let endpoint = local_endpoint(data_dir, port);
        let stdout = data_dir.with_extension("hyphae.stdout.log");
        let stderr = data_dir.with_extension("hyphae.stderr.log");
        let stdout_file = File::create(&stdout).map_err(SidecarError::Io)?;
        let stderr_file = File::create(&stderr).map_err(SidecarError::Io)?;
        let child = Command::new(&installation.executable)
            .args([
                "serve",
                "--data-dir",
                path_text(data_dir)?,
                "--endpoint",
                &endpoint,
                "--http-bind",
                &format!("127.0.0.1:{port}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .map_err(SidecarError::Io)?;
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let client = NativeHttpClient::new(address, Duration::from_secs(5))?;
        let mut sidecar = Self {
            child,
            stdout,
            stderr,
            client,
        };
        sidecar.wait_until_ready().await?;
        Ok(sidecar)
    }

    pub fn client(&self) -> &NativeHttpClient {
        &self.client
    }

    pub fn store(&self) -> super::ConsoleStore {
        super::ConsoleStore::new(self.client.clone())
    }

    pub fn shutdown(&mut self) -> Result<(), SidecarError> {
        if self.child.try_wait().map_err(SidecarError::Io)?.is_none() {
            terminate_process_tree(&mut self.child)?;
        }
        self.child.wait().map_err(SidecarError::Io)?;
        Ok(())
    }

    async fn wait_until_ready(&mut self) -> Result<(), SidecarError> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(SidecarError::Io)? {
                return Err(SidecarError::EarlyExit(
                    status.to_string(),
                    self.diagnostics(),
                ));
            }
            match self.client.capabilities(1).await {
                Ok(capabilities)
                    if capabilities.product_api_version
                        == self.client_authority().product_api_version
                        && capabilities.native_directory_format
                            == self.client_authority().native_directory_format =>
                {
                    return Ok(());
                }
                Ok(_) => return Err(SidecarError::Capabilities),
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                Err(error) => {
                    return Err(SidecarError::Readiness(Box::new(error), self.diagnostics()));
                }
            }
        }
    }

    fn client_authority(&self) -> SidecarAuthority {
        SidecarAuthority::load().expect("embedded authority was validated before process start")
    }

    fn diagnostics(&self) -> String {
        format!(
            "stdout:\n{}\nstderr:\n{}",
            bounded_log(&self.stdout),
            bounded_log(&self.stderr)
        )
    }
}

impl Drop for HyphaeSidecar {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Debug)]
pub enum AdmissionError {
    Authority(serde_json::Error),
    AuthorityMismatch,
    UnsupportedPlatform,
    Io(std::io::Error),
    ExecutableType,
    ExecutableDigest { expected: String, actual: String },
    VersionCommand,
    VersionJson(serde_json::Error),
    VersionMismatch,
    Timeout,
    OutputBound,
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Authority(_) | Self::AuthorityMismatch => "Hyphae authority is invalid",
            Self::UnsupportedPlatform => "Hyphae platform is not reviewed",
            Self::Io(_) => "Hyphae executable could not be inspected",
            Self::ExecutableType => "Hyphae executable is not a regular file",
            Self::ExecutableDigest { .. } => "Hyphae executable digest differs",
            Self::VersionCommand | Self::VersionJson(_) | Self::VersionMismatch => {
                "Hyphae version identity differs"
            }
            Self::Timeout => "Hyphae identity command timed out",
            Self::OutputBound => "Hyphae identity output exceeds its bound",
        })
    }
}

impl std::error::Error for AdmissionError {}

#[derive(Debug)]
pub enum SidecarError {
    Admission(AdmissionError),
    Transport(Box<TransportError>),
    Io(std::io::Error),
    InvalidPath,
    Initialization,
    Termination,
    EarlyExit(String, String),
    Capabilities,
    Readiness(Box<TransportError>, String),
}

impl fmt::Display for SidecarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Admission(_) => "Hyphae installation admission failed",
            Self::Transport(_) => "Hyphae sidecar transport failed",
            Self::Io(_) => "Hyphae sidecar process failed",
            Self::InvalidPath => "Hyphae data path is not Unicode",
            Self::Initialization => "Hyphae data directory initialization failed",
            Self::Termination => "Hyphae sidecar process tree could not be terminated",
            Self::EarlyExit(_, _) => "Hyphae sidecar exited before readiness",
            Self::Capabilities => "Hyphae sidecar capabilities differ",
            Self::Readiness(_, _) => "Hyphae sidecar readiness timed out",
        })
    }
}

impl std::error::Error for SidecarError {}
impl From<AdmissionError> for SidecarError {
    fn from(value: AdmissionError) -> Self {
        Self::Admission(value)
    }
}
impl From<TransportError> for SidecarError {
    fn from(value: TransportError) -> Self {
        Self::Transport(Box::new(value))
    }
}

fn stable_file_sha256(path: &Path) -> Result<String, AdmissionError> {
    let mut file = File::open(path).map_err(AdmissionError::Io)?;
    let before = file.metadata().map_err(AdmissionError::Io)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(AdmissionError::Io)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let after = file.metadata().map_err(AdmissionError::Io)?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(AdmissionError::ExecutableType);
    }
    file.rewind().map_err(AdmissionError::Io)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn run_bounded(
    executable: &Path,
    arguments: &[&str],
    directory: Option<&Path>,
) -> Result<Output, AdmissionError> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let mut child = command.spawn().map_err(AdmissionError::Io)?;
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    loop {
        if child.try_wait().map_err(AdmissionError::Io)?.is_some() {
            let output = child.wait_with_output().map_err(AdmissionError::Io)?;
            if output.stdout.len() > MAX_PROCESS_OUTPUT || output.stderr.len() > MAX_PROCESS_OUTPUT
            {
                return Err(AdmissionError::OutputBound);
            }
            return Ok(output);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AdmissionError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn available_port() -> Result<u16, SidecarError> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(SidecarError::Io)
}

fn local_endpoint(data_dir: &Path, port: u16) -> String {
    if cfg!(windows) {
        format!("pliegors-hyphae-{}-{port}", std::process::id())
    } else {
        data_dir
            .with_extension(format!("{port}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

fn path_text(path: &Path) -> Result<&str, SidecarError> {
    path.to_str().ok_or(SidecarError::InvalidPath)
}

fn bounded_log(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let start = bytes.len().saturating_sub(MAX_PROCESS_OUTPUT);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

fn terminate_process_tree(child: &mut Child) -> Result<(), SidecarError> {
    if cfg!(windows) {
        let status = Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(SidecarError::Io)?;
        if !status.success() && child.try_wait().map_err(SidecarError::Io)?.is_none() {
            return Err(SidecarError::Termination);
        }
        return Ok(());
    }
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(SidecarError::Io)?;
    if !status.success() {
        return child.kill().map_err(SidecarError::Io);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().map_err(SidecarError::Io)?.is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    child.kill().map_err(SidecarError::Io)
}

const fn current_target() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("x86_64-pc-windows-msvc")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-gnu")
    } else {
        None
    }
}
