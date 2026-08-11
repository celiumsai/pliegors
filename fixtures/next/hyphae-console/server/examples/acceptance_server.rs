// SPDX-License-Identifier: AGPL-3.0-only

use pliegors_next_hyphae_console_server::{
    HyphaeInstallation, HyphaeSidecar, SidecarAuthority, build_console_acceptance_runtime,
};
use serde_json::json;
use std::error::Error;
use std::io::{self, BufRead, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

const CONTRACT: &str = "dev.pliegors.hyphae-console-acceptance/v1";

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Hyphae Console acceptance server failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let executable = required_path("HYPHAE_V101_BIN")?;
    let data_dir = required_path("PLIEGO_HYPHAE_DATA_DIR")?;
    let parent_pid = required_u32("PLIEGO_HYPHAE_PARENT_PID")?;
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    let address = listener.local_addr()?;
    let origin = format!("http://127.0.0.1:{}", address.port());
    let authority = SidecarAuthority::load().map_err(context("load sidecar authority"))?;
    let installation = HyphaeInstallation::admit(&executable, &authority)
        .map_err(context("admit sidecar executable"))?;
    let mut sidecar = HyphaeSidecar::start(&installation, &data_dir)
        .await
        .map_err(context("start sidecar"))?;
    let observation = sidecar.observation();
    let runtime = build_console_acceptance_runtime(sidecar.store(), &origin)
        .map_err(context("build Console runtime"))?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    wait_for_shutdown_or_parent_exit(shutdown_tx, parent_pid);

    emit(json!({
        "contract": CONTRACT,
        "event": "ready",
        "appPid": std::process::id(),
        "origin": origin,
        "sidecarPid": observation.process_id,
        "sidecarHttpAddress": observation.http_address.to_string(),
        "nativeEndpoint": observation.native_endpoint,
        "stdoutLog": observation.stdout_log,
        "stderrLog": observation.stderr_log,
    }))?;

    let serve_result = runtime
        .serve(listener, async move {
            let _ = shutdown_rx.await;
        })
        .await;
    let shutdown_result = sidecar.shutdown();
    serve_result?;
    shutdown_result.map_err(context("stop sidecar"))?;
    emit(json!({
        "contract": CONTRACT,
        "event": "stopped",
        "appPid": std::process::id(),
        "sidecarPid": observation.process_id,
    }))?;
    Ok(())
}

fn required_path(name: &str) -> io::Result<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}

fn context<E>(operation: &'static str) -> impl FnOnce(E) -> io::Error
where
    E: std::fmt::Debug,
{
    move |error| io::Error::other(format!("could not {operation}: {error:?}"))
}

fn required_u32(name: &str) -> io::Result<u32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}

fn wait_for_shutdown_or_parent_exit(shutdown: oneshot::Sender<()>, parent_pid: u32) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let mut line = String::new();
        loop {
            if !parent_is_alive(parent_pid) {
                break;
            }
            line.clear();
            match input.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) if line.trim() == "shutdown" => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        let _ = shutdown.send(());
    });
}

#[cfg(windows)]
fn parent_is_alive(parent_pid: u32) -> bool {
    std::process::Command::new("tasklist.exe")
        .args(["/FI", &format!("PID eq {parent_pid}"), "/FO", "CSV", "/NH"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.starts_with(b"INFO:"))
}

#[cfg(not(windows))]
fn parent_is_alive(parent_pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &parent_pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn emit(value: serde_json::Value) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}
