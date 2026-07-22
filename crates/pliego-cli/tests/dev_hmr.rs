// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

struct DevProcess {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl DevProcess {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn diagnostics(&self) -> String {
        format!(
            "stdout:\n{}\nstderr:\n{}",
            fs::read_to_string(&self.stdout).unwrap_or_default(),
            fs::read_to_string(&self.stderr).unwrap_or_default()
        )
    }
}

impl Drop for DevProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn pliego(arguments: &[&str], directory: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pliego"))
        .args(arguments)
        .current_dir(directory)
        .env("CARGO_TARGET_DIR", test_target_directory())
        .output()
        .expect("run pliego test binary")
}

fn framework_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI crate lives below framework root")
        .to_path_buf()
}

fn temporary_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "pliego-dev-hmr-{}-{}",
        std::process::id(),
        NEXT_TEST.fetch_add(1, Ordering::Relaxed)
    ))
}

fn test_target_directory() -> PathBuf {
    option_env!("CARGO_TARGET_TMPDIR")
        .and_then(|path| Path::new(path).parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| framework_root().join("target"))
}

fn available_port() -> u16 {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_dev(project: &Path, port: u16) -> DevProcess {
    let stdout = project.with_extension("dev.stdout.log");
    let stderr = project.with_extension("dev.stderr.log");
    let stdout_file = fs::File::create(&stdout).unwrap();
    let stderr_file = fs::File::create(&stderr).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_pliego"))
        .args(["dev", &port.to_string()])
        .current_dir(project)
        .env("CARGO_TARGET_DIR", test_target_directory())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("start pliego dev");
    DevProcess {
        child,
        stdout,
        stderr,
    }
}

fn wait_for_server(process: &mut DevProcess, port: u16) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = process.child.try_wait().unwrap() {
            panic!("pliego dev exited with {status}\n{}", process.diagnostics());
        }
        if TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "pliego dev did not open its port\n{}",
            process.diagnostics()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn long_poll(port: u16, since: u64) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .unwrap();
        write!(
            stream,
            "GET /_pliego/reload?since={since} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    })
}

fn get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn native_watcher_css_hmr_and_rebuild_explanation_work_end_to_end() {
    let project = temporary_directory();
    let framework = framework_root();
    let scaffold = pliego(
        &[
            "new",
            project.to_str().unwrap(),
            "--framework-path",
            framework.to_str().unwrap(),
        ],
        &std::env::temp_dir(),
    );
    assert!(
        scaffold.status.success(),
        "{}",
        String::from_utf8_lossy(&scaffold.stderr)
    );

    let port = available_port();
    let mut dev = start_dev(&project, port);
    wait_for_server(&mut dev, port);
    let pending = long_poll(port, 0);
    std::thread::sleep(Duration::from_millis(250));

    let stylesheet = project.join("assets/site.css");
    let mut css = fs::read_to_string(&stylesheet).unwrap();
    css.push_str("\n/* native HMR acceptance */\n");
    fs::write(&stylesheet, css).unwrap();

    let response = pending.join().unwrap();
    assert!(response.contains("200 OK"), "{response}");
    assert!(response.contains("event: pliego"), "{response}");
    assert!(response.contains("\"kind\":\"css\""), "{response}");
    assert!(response.contains("/assets/site.css"), "{response}");

    let why = pliego(&["why-rebuilt"], &project);
    assert!(
        why.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&why.stderr),
        dev.diagnostics()
    );
    let why = String::from_utf8(why.stdout).unwrap();
    assert!(why.contains("Css"), "{why}");
    assert!(why.contains("assets/site.css"), "{why}");

    let pending = long_poll(port, 1);
    std::thread::sleep(Duration::from_millis(250));
    let main = project.join("src/main.rs");
    let source = fs::read_to_string(&main).unwrap().replace(
        "Your Rust site is running.",
        "Causal content HMR is running.",
    );
    fs::write(&main, source).unwrap();
    let response = pending.join().unwrap();
    assert!(response.contains("\"kind\":\"content\""), "{response}");
    assert!(response.contains("\"routes\":[\"/\""), "{response}");
    assert!(get(port, "/").contains("Causal content HMR is running."));

    let why = pliego(&["why-rebuilt"], &project);
    assert!(why.status.success());
    assert!(String::from_utf8(why.stdout).unwrap().contains("Content"));

    dev.stop();
    let _ = fs::remove_file(&dev.stdout);
    let _ = fs::remove_file(&dev.stderr);
    fs::remove_dir_all(project).unwrap();
}
