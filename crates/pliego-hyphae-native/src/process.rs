// SPDX-License-Identifier: AGPL-3.0-only

use crate::error::TransportErrorKind;
use crate::{NativeHttpClient, TransportError};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROCESS_OUTPUT: usize = 64 * 1024;
const AUTHORITY_JSON: &str = r#"{
  "contract": "dev.pliegors.hyphae-sidecar-authority/v1",
  "repository": "https://github.com/celiumsai/hyphae",
  "releaseTag": "v1.0.1",
  "releaseRevision": "84161cf067141b60f4847b965ef77c5b749749c0",
  "releaseChecksumsSha256": "3e45b7056f27a3e2062e7d216cf03972108aae032bee36589210eb28cd569048",
  "engineVersion": "1.0.1",
  "productApiVersion": 1,
  "nativeDirectoryFormat": 1,
  "transport": "native-http-v2-loopback",
  "productMediaType": "application/vnd.hyphae.product-v1",
  "errorMediaType": "application/vnd.hyphae.error-v1",
  "rustMsrv": "1.89.0",
  "artifacts": [
    {
      "target": "x86_64-pc-windows-msvc",
      "archive": "hyphae-1.0.1-x86_64-pc-windows-msvc.zip",
      "archiveSha256": "2864d7370e78e8e49b8d72a8f12ce07f7c4434d0ea191c93e1a3237db6c3c07b",
      "executableSha256": "23db588b23955a0a6c61a0813b82f2e9ea569510a10cf8ce10c72bcdf903e99a"
    },
    {
      "target": "x86_64-unknown-linux-gnu",
      "archive": "hyphae-1.0.1-x86_64-unknown-linux-gnu.tar.gz",
      "archiveSha256": "8745c3615fb5b21747a190b4c922860ff8d3c1c69efe35068a80be56942bcd13",
      "executableSha256": "8a34113a51ffd795db4669419674918f0e11da41f2146f9a4cbe7dcf1878fde1"
    }
  ]
}"#;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Exact release and artifact authority embedded by this adapter version.
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
    /// Loads and validates the embedded Hyphae `v1.0.1` authority.
    ///
    /// # Errors
    ///
    /// Returns an error when the embedded authority is malformed or differs
    /// from the compile-time pin.
    pub fn load() -> Result<Self, TransportError> {
        let authority: Self = serde_json::from_str(AUTHORITY_JSON)
            .map_err(|_| TransportErrorKind::AuthorityMalformed)?;
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
            return Err(TransportErrorKind::AuthorityMismatch.into());
        }
        Ok(authority)
    }

    /// Returns the exact admitted release tag.
    pub fn release_tag(&self) -> &str {
        &self.release_tag
    }

    /// Returns the exact admitted release commit.
    pub fn release_revision(&self) -> &str {
        &self.release_revision
    }

    /// Returns the reviewed archive name for the current platform.
    ///
    /// # Errors
    ///
    /// Returns an error when the current platform has no reviewed artifact.
    pub fn current_artifact(&self) -> Result<&str, TransportError> {
        Ok(&self.artifact_for_current_platform()?.archive)
    }

    fn artifact_for_current_platform(&self) -> Result<&ArtifactAuthority, TransportError> {
        let target = current_target().ok_or(TransportErrorKind::UnsupportedPlatform)?;
        self.artifacts
            .iter()
            .find(|artifact| artifact.target == target)
            .ok_or_else(|| TransportErrorKind::UnsupportedPlatform.into())
    }
}

#[derive(Clone, Debug)]
/// A local executable admitted against [`SidecarAuthority`].
pub struct HyphaeInstallation {
    executable: PathBuf,
    authority: SidecarAuthority,
}

impl HyphaeInstallation {
    /// Admits the executable named by `HYPHAE_BIN`.
    ///
    /// # Errors
    ///
    /// Returns an error when the variable is absent or executable admission
    /// fails.
    pub fn from_env(authority: &SidecarAuthority) -> Result<Self, TransportError> {
        let executable = std::env::var_os("HYPHAE_BIN")
            .ok_or(TransportErrorKind::MissingEnvironment("HYPHAE_BIN"))?;
        Self::admit(Path::new(&executable), authority)
    }

    /// Admits one regular executable by its reviewed SHA-256 digest.
    ///
    /// # Errors
    ///
    /// Returns an error for missing, non-regular, symlinked, unsupported, or
    /// digest-mismatched executables.
    pub fn admit(executable: &Path, authority: &SidecarAuthority) -> Result<Self, TransportError> {
        if executable.as_os_str().is_empty() || !executable.exists() {
            return Err(TransportErrorKind::MissingExecutable.into());
        }
        let details = fs::symlink_metadata(executable).map_err(TransportErrorKind::Io)?;
        if !details.file_type().is_file() || details.file_type().is_symlink() {
            return Err(TransportErrorKind::ExecutableType.into());
        }
        let artifact = authority.artifact_for_current_platform()?;
        if artifact.archive.is_empty() || artifact.archive_sha256.len() != 64 {
            return Err(TransportErrorKind::AuthorityMismatch.into());
        }
        let actual = stable_file_sha256(executable)?;
        if actual != artifact.executable_sha256 {
            return Err(TransportErrorKind::ExecutableDigest {
                expected: artifact.executable_sha256.clone(),
                actual,
            }
            .into());
        }
        Ok(Self {
            executable: executable.to_path_buf(),
            authority: authority.clone(),
        })
    }

    /// Runs the bounded `hyphae version --json` identity check.
    ///
    /// # Errors
    ///
    /// Returns an error when the process fails, exceeds a bound, or reports an
    /// identity that differs from the admitted authority.
    pub fn verify_version(&self) -> Result<(), TransportError> {
        let output = run_bounded(&self.executable, &["version", "--json"])?;
        if !output.status.success() {
            return Err(TransportErrorKind::VersionCommand.into());
        }
        let version: VersionReport = serde_json::from_slice(&output.stdout)
            .map_err(|_| TransportErrorKind::VersionMismatch)?;
        if version.product != "hyphae"
            || version.api_version != "v1"
            || version.disk_format_version != 2
            || version.engine_version != self.authority.engine_version
            || version.product_api_version != self.authority.product_api_version
            || version.native_directory_format != self.authority.native_directory_format
        {
            return Err(TransportErrorKind::VersionMismatch.into());
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

/// A supervised Hyphae process with an admitted loopback client.
pub struct HyphaeSidecar {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
    client: NativeHttpClient,
}

impl HyphaeSidecar {
    /// Starts the sidecar with the directory named by `HYPHAE_DATA_DIR`.
    ///
    /// # Errors
    ///
    /// Returns an error when the variable is absent or startup fails.
    pub async fn start_from_env(installation: &HyphaeInstallation) -> Result<Self, TransportError> {
        let data_dir = std::env::var_os("HYPHAE_DATA_DIR")
            .ok_or(TransportErrorKind::MissingEnvironment("HYPHAE_DATA_DIR"))?;
        Self::start(installation, Path::new(&data_dir)).await
    }

    /// Initializes, starts, and admits the sidecar for `data_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error for identity, initialization, process, loopback
    /// transport, capability, or readiness failures.
    pub async fn start(
        installation: &HyphaeInstallation,
        data_dir: &Path,
    ) -> Result<Self, TransportError> {
        installation.verify_version()?;
        if !data_dir.exists() {
            let output = run_bounded(
                &installation.executable,
                &["init", "--data-dir", path_text(data_dir)?],
            )?;
            if !output.status.success() {
                return Err(TransportErrorKind::Initialization.into());
            }
        }
        let port = available_port()?;
        let endpoint = local_endpoint(data_dir, port);
        let stdout = data_dir.with_extension("hyphae.stdout.log");
        let stderr = data_dir.with_extension("hyphae.stderr.log");
        let stdout_file = File::create(&stdout).map_err(TransportErrorKind::Io)?;
        let stderr_file = File::create(&stderr).map_err(TransportErrorKind::Io)?;
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
            .map_err(TransportErrorKind::Io)?;
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

    /// Borrows the admitted loopback product client.
    pub fn client(&self) -> &NativeHttpClient {
        &self.client
    }

    /// Terminates and waits for the sidecar process.
    ///
    /// # Errors
    ///
    /// Returns an error when process inspection, termination, or waiting fails.
    pub fn shutdown(&mut self) -> Result<(), TransportError> {
        if self
            .child
            .try_wait()
            .map_err(TransportErrorKind::Io)?
            .is_none()
        {
            terminate_process_tree(&mut self.child)?;
        }
        self.child.wait().map_err(TransportErrorKind::Io)?;
        Ok(())
    }

    async fn wait_until_ready(&mut self) -> Result<(), TransportError> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(TransportErrorKind::Io)? {
                return Err(TransportErrorKind::EarlyExit(status.to_string()).into());
            }
            match self.client.validate_capabilities(1).await {
                Ok(_) => return Ok(()),
                Err(error) if error.code() == "incompatible_capabilities" => return Err(error),
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => {
                    let _ = error;
                    let _ = self.diagnostics();
                    return Err(TransportErrorKind::Readiness.into());
                }
            }
        }
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

fn stable_file_sha256(path: &Path) -> Result<String, TransportError> {
    let mut file = File::open(path).map_err(TransportErrorKind::Io)?;
    let before = file.metadata().map_err(TransportErrorKind::Io)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(TransportErrorKind::Io)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let after = file.metadata().map_err(TransportErrorKind::Io)?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(TransportErrorKind::ExecutableType.into());
    }
    file.rewind().map_err(TransportErrorKind::Io)?;
    Ok(format!("{:x}", hash.finalize()))
}

fn run_bounded(executable: &Path, arguments: &[&str]) -> Result<Output, TransportError> {
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(TransportErrorKind::Io)?;
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    loop {
        if child.try_wait().map_err(TransportErrorKind::Io)?.is_some() {
            let output = child.wait_with_output().map_err(TransportErrorKind::Io)?;
            if output.stdout.len() > MAX_PROCESS_OUTPUT || output.stderr.len() > MAX_PROCESS_OUTPUT
            {
                return Err(TransportErrorKind::OutputBound.into());
            }
            return Ok(output);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TransportErrorKind::Timeout.into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn available_port() -> Result<u16, TransportError> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| TransportErrorKind::Io(error).into())
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

fn path_text(path: &Path) -> Result<&str, TransportError> {
    path.to_str()
        .ok_or_else(|| TransportErrorKind::InvalidPath.into())
}

fn bounded_log(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let start = bytes.len().saturating_sub(MAX_PROCESS_OUTPUT);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

fn terminate_process_tree(child: &mut Child) -> Result<(), TransportError> {
    if cfg!(windows) {
        let status = Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(TransportErrorKind::Io)?;
        if !status.success() && child.try_wait().map_err(TransportErrorKind::Io)?.is_none() {
            return Err(TransportErrorKind::Termination.into());
        }
        return Ok(());
    }
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(TransportErrorKind::Io)?;
    if !status.success() {
        return child
            .kill()
            .map_err(|error| TransportErrorKind::Io(error).into());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().map_err(TransportErrorKind::Io)?.is_some() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    child
        .kill()
        .map_err(|error| TransportErrorKind::Io(error).into())
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
