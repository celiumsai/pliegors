// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

fn pliego(arguments: &[&str], directory: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pliego"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("run pliego test binary")
}

fn pliego_with_home(
    arguments: &[&str],
    directory: &std::path::Path,
    home: &std::path::Path,
    proxy: Option<&str>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pliego"));
    command
        .args(arguments)
        .current_dir(directory)
        .env("PLIEGO_HOME", home);
    if let Some(proxy) = proxy {
        command
            .env("HTTP_PROXY", proxy)
            .env("HTTPS_PROXY", proxy)
            .env("ALL_PROXY", proxy)
            .env("NO_PROXY", "");
    }
    command.output().expect("run pliego with isolated home")
}

fn temporary_directory(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "pliego-cli-{label}-{}-{}",
        std::process::id(),
        NEXT_TEST.fetch_add(1, Ordering::Relaxed)
    ))
}

fn framework_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("CLI crate lives below framework root")
        .to_path_buf()
}

fn opensdk_fixture(root: &std::path::Path, plane: &str) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let (entry_name, entry_kind, bytes, world, determinism, capabilities) = match plane {
        "build" => (
            "component.wasm",
            "wasm-component",
            vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00],
            Some("pliego:build/transformer@0.1.0"),
            "pure",
            Vec::<&str>::new(),
        ),
        "browser" => (
            "component.js",
            "browser-esm",
            b"customElements.define('pliego-test', class extends HTMLElement {});".to_vec(),
            None,
            "native-trusted",
            vec!["dom"],
        ),
        other => panic!("unknown fixture plane {other}"),
    };
    fs::write(root.join(entry_name), &bytes).unwrap();
    let mut entry = serde_json::json!({
        "kind": entry_kind,
        "path": entry_name,
    });
    if let Some(world) = world {
        entry["world"] = Value::String(world.to_owned());
    }
    if plane == "browser" {
        entry["customElement"] = Value::String("pliego-test".to_owned());
    }
    let manifest = serde_json::json!({
        "schema": "dev.pliegors.sdk-extension/v1",
        "apiVersion": "0.1.0-preview.1",
        "hostVersion": ">=0.1.0-preview.1, <0.2.0",
        "plane": plane,
        "identity": {
            "namespace": "celiums",
            "name": format!("{plane}-fixture"),
            "version": "0.1.0",
            "digest": format!("sha256:{:x}", Sha256::digest(&bytes)),
        },
        "entry": entry,
        "determinism": determinism,
        "imports": [],
        "exports": [],
        "capabilities": capabilities,
        "requiredFeatures": [],
        "optionalFeatures": [],
        "budgets": {
            "cpuMs": 100,
            "wallTimeMs": 500,
            "memoryBytes": 16 * 1024 * 1024,
            "outputBytes": 1024 * 1024,
        },
        "lifecycle": {
            "init": true,
            "update": false,
            "suspend": false,
            "resume": false,
            "dispose": true,
            "hmr": false,
        },
    });
    let path = root.join("pliego-extension.json");
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    path
}

#[test]
fn unknown_command_is_usage_even_outside_a_project() {
    let output = pliego(&["unknown-command"], &std::env::temp_dir());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("PLG-ARG-001"));
    assert!(stderr.contains("unknown command"));
}

#[test]
fn sdk_check_and_test_admit_exact_component_bytes_outside_a_project() {
    let root = temporary_directory("sdk-component");
    let manifest = opensdk_fixture(&root, "build");
    let check = pliego(
        &[
            "sdk",
            "check",
            manifest.to_str().unwrap(),
            "--format",
            "json",
        ],
        &root,
    );
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let report: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(report["contract"], "dev.pliegors.sdk-conformance/v1");
    assert_eq!(report["level"], "admission");
    assert_eq!(report["result"], "pass");
    assert_eq!(
        report["admission"]["grantedCapabilities"],
        serde_json::json!([])
    );

    let test = pliego(
        &["sdk", "test", manifest.to_str().unwrap(), "--format=json"],
        &root,
    );
    assert!(
        test.status.success(),
        "{}",
        String::from_utf8_lossy(&test.stderr)
    );
    let report: Value = serde_json::from_slice(&test.stdout).unwrap();
    assert_eq!(report["level"], "component-instantiation");
    assert_eq!(
        report["componentHost"]["schema"],
        "dev.pliegors.component-host/v1"
    );
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check == "deterministic-admission")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sdk_capabilities_and_digests_fail_closed_before_execution() {
    let root = temporary_directory("sdk-policy");
    let manifest = opensdk_fixture(&root, "browser");
    let denied = pliego(&["sdk", "check", manifest.to_str().unwrap()], &root);
    assert_eq!(denied.status.code(), Some(9));
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(stderr.contains("PLG-SDK-001"));
    assert!(stderr.contains("capability `dom` was not granted"));

    let granted = pliego(
        &["sdk", "check", manifest.to_str().unwrap(), "--grant", "dom"],
        &root,
    );
    assert!(
        granted.status.success(),
        "{}",
        String::from_utf8_lossy(&granted.stderr)
    );
    fs::write(root.join("component.js"), b"tampered").unwrap();
    let tampered = pliego(&["sdk", "check", manifest.to_str().unwrap()], &root);
    assert_eq!(tampered.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("digest mismatch"));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn sdk_entry_cannot_escape_through_an_intermediate_symlink() {
    use std::os::unix::fs::symlink;

    let root = temporary_directory("sdk-linked-ancestor");
    let outside = temporary_directory("sdk-linked-outside");
    let manifest_path = opensdk_fixture(&root, "build");
    fs::create_dir_all(&outside).unwrap();
    fs::copy(root.join("component.wasm"), outside.join("component.wasm")).unwrap();
    symlink(&outside, root.join("linked")).unwrap();
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["entry"]["path"] = Value::String("linked/component.wasm".to_owned());
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let result = pliego(&["sdk", "check", manifest_path.to_str().unwrap()], &root);
    assert_eq!(result.status.code(), Some(9));
    assert!(String::from_utf8_lossy(&result.stderr).contains("non-symlink"));

    fs::remove_file(root.join("linked")).unwrap();
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn sdk_compatibility_is_machine_readable_and_bound_to_the_public_source() {
    let output = pliego(
        &["sdk", "compatibility", "--format", "json"],
        &std::env::temp_dir(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let matrix: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(matrix["contract"], "dev.pliegors.sdk-compatibility/v1");
    assert_eq!(matrix["protocolVersion"], "0.1.0-preview.1");
    assert_eq!(matrix["source"], "celiumsai/pliegors");
    assert_eq!(matrix["deprecations"], serde_json::json!([]));
}

#[test]
fn malformed_css_delegation_is_usage_even_outside_a_project() {
    for arguments in [&["css"][..], &["css", "lint"][..]] {
        let output = pliego(arguments, &std::env::temp_dir());
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("PLG-ARG-001"));
        assert!(stderr.contains("pliego css check"));
    }
}

#[test]
fn json_diagnostics_have_a_stable_machine_contract() {
    let output = pliego(
        &["unknown-command", "--diagnostic-format", "json"],
        &std::env::temp_dir(),
    );
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stderr).expect("JSON diagnostic");
    assert_eq!(value["code"], "PLG-ARG-001");
    assert_eq!(value["exit_code"], 2);
    assert_eq!(value["category"], "usage");
    assert!(value["help"].as_str().is_some_and(|help| !help.is_empty()));
    assert!(value["spans"].is_array());
    assert!(
        value["fixes"]
            .as_array()
            .is_some_and(|fixes| !fixes.is_empty())
    );
    assert_eq!(value["fixes"][0]["applicability"], "manual");
}

#[test]
fn template_catalog_is_available_without_a_project() {
    let output = pliego(&["templates"], &std::env::temp_dir());
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("default"));
    assert!(stdout.contains("(default)"));
    assert!(stdout.contains("minimal"));
    assert!(stdout.contains("editorial"));
    assert!(stdout.contains("cinematic"));
}

#[test]
fn doctor_has_a_versioned_machine_contract_outside_a_project() {
    let directory = temporary_directory("doctor-global");
    fs::create_dir_all(&directory).unwrap();
    let output = pliego(&["doctor", "--format", "json"], &directory);
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("doctor JSON report");
    assert_eq!(value["reportVersion"], "1.0.0");
    assert_eq!(value["cliVersion"], env!("CARGO_PKG_VERSION"));
    assert!(value["checks"].as_array().is_some_and(|checks| {
        checks.iter().any(|check| check["id"] == "PLG-DOC-002")
            && checks.iter().any(|check| check["id"] == "PLG-DOC-005")
    }));
    assert_eq!(value["project"], Value::Null);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn malformed_doctor_options_are_usage_errors() {
    let output = pliego(&["doctor", "--format", "yaml"], &std::env::temp_dir());
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("PLG-ARG-001"));
}

#[test]
fn voluntary_telemetry_is_disabled_local_bounded_and_deletable() {
    let root = temporary_directory("telemetry-contract");
    let home = root.join("home");
    let disabled_project = root.join("disabled-project");
    let project = root.join("project");
    let export = root.join("voluntary-report.json");
    fs::create_dir_all(&root).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());

    let status = pliego_with_home(
        &["telemetry", "status", "--format", "json"],
        &root,
        &home,
        Some(&proxy),
    );
    assert!(status.status.success());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["enabled"], false);
    assert_eq!(status["localEventCount"], 0);
    assert_eq!(status["networkSubmission"], "none");
    assert!(!home.exists(), "status must not create telemetry storage");

    let disabled_new = pliego_with_home(
        &[
            "new",
            disabled_project.to_str().unwrap(),
            "--framework-path",
            framework_root().to_str().unwrap(),
        ],
        &root,
        &home,
        Some(&proxy),
    );
    assert!(disabled_new.status.success());
    assert!(
        !home.exists(),
        "disabled funnel commands must not create telemetry storage"
    );

    let enabled = pliego_with_home(&["telemetry", "enable"], &root, &home, Some(&proxy));
    assert!(enabled.status.success());
    let created = pliego_with_home(
        &[
            "new",
            project.to_str().unwrap(),
            "--framework-path",
            framework_root().to_str().unwrap(),
        ],
        &root,
        &home,
        Some(&proxy),
    );
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );

    let preview = pliego_with_home(
        &["telemetry", "preview", "--format", "json"],
        &root,
        &home,
        Some(&proxy),
    );
    assert!(preview.status.success());
    let report: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(report["contract"], "dev.pliegors.telemetry-report/v1");
    assert_eq!(report["events"][0]["event"], "install");
    assert_eq!(report["events"][1]["event"], "new");
    assert_eq!(report["events"].as_array().unwrap().len(), 2);
    let preview_bytes = preview.stdout;
    let text = String::from_utf8(preview_bytes.clone())
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in ["project", "argument", "error", "environment", "email"] {
        assert!(
            !text.contains(forbidden),
            "telemetry report contains {forbidden}"
        );
    }

    let exported = pliego_with_home(
        &["telemetry", "export", "--output", export.to_str().unwrap()],
        &root,
        &home,
        Some(&proxy),
    );
    assert!(exported.status.success());
    assert_eq!(fs::read(&export).unwrap(), preview_bytes);
    let overwrite = pliego_with_home(
        &["telemetry", "export", "--output", export.to_str().unwrap()],
        &root,
        &home,
        Some(&proxy),
    );
    assert_eq!(overwrite.status.code(), Some(8));
    assert!(String::from_utf8_lossy(&overwrite.stderr).contains("PLG-TEL-001"));

    let disabled = pliego_with_home(
        &["telemetry", "disable", "--delete-local"],
        &root,
        &home,
        Some(&proxy),
    );
    assert!(disabled.status.success());
    assert!(!home.join("telemetry").exists());
    thread::sleep(Duration::from_millis(50));
    assert!(
        listener
            .accept()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock),
        "telemetry controls unexpectedly connected to the proxy"
    );
    fs::remove_dir_all(root).unwrap();
}

fn write_trust_fixture(destination: &std::path::Path, version: &str) {
    fs::create_dir_all(destination).unwrap();
    fs::write(
        destination.join("pliego.toml"),
        "[project]\nid = \"trust-fixture\"\nname = \"Trust Fixture\"\nsite_package = \"trust-fixture\"\noutput = \"target/site\"\n",
    )
    .unwrap();
    fs::write(
        destination.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"pliego-ssg\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"0000000000000000000000000000000000000000000000000000000000000000\"\n"
        ),
    )
    .unwrap();
}

#[test]
fn report_bundle_is_deterministic_bounded_and_excludes_project_secrets() {
    let destination = temporary_directory("report-bundle");
    write_trust_fixture(&destination, env!("CARGO_PKG_VERSION"));
    fs::create_dir_all(destination.join("src")).unwrap();
    fs::write(
        destination.join("src/private.rs"),
        "const SECRET: &str = \"PLIEGO_TEST_SECRET_6f32\";",
    )
    .unwrap();
    fs::write(destination.join(".env"), "TOKEN=PLIEGO_TEST_SECRET_6f32").unwrap();
    let first = destination.join("first.tar");
    let second = destination.join("second.tar");
    for output in [&first, &second] {
        let result = pliego(
            &["report", "--bundle", "--output", output.to_str().unwrap()],
            &destination,
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    let bytes = fs::read(&first).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("PLIEGO_TEST_SECRET_6f32"));
    let mut archive = tar::Archive::new(bytes.as_slice());
    let mut entries = Vec::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().replace('\\', "/");
        let mut body = Vec::new();
        entry.read_to_end(&mut body).unwrap();
        entries.push((path, body));
    }
    assert!(entries.iter().any(|(path, _)| path == "MANIFEST.json"));
    assert!(entries.iter().any(|(path, _)| path == "report/doctor.json"));
    assert!(entries.iter().any(|(path, _)| path == "report/pliego.toml"));
    assert!(
        !entries
            .iter()
            .any(|(path, _)| path.contains("private.rs") || path.contains(".env"))
    );
    fs::remove_dir_all(destination).unwrap();
}

#[test]
fn upgrade_check_is_read_only_and_reports_exact_alignment() {
    let destination = temporary_directory("upgrade-check");
    write_trust_fixture(&destination, env!("CARGO_PKG_VERSION"));
    let manifest = fs::read(destination.join("pliego.toml")).unwrap();
    let lock = fs::read(destination.join("Cargo.lock")).unwrap();
    let output = pliego(&["upgrade", "--check", "--format", "json"], &destination);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "compatible");
    assert_eq!(fs::read(destination.join("pliego.toml")).unwrap(), manifest);
    assert_eq!(fs::read(destination.join("Cargo.lock")).unwrap(), lock);
    fs::remove_dir_all(destination).unwrap();
}

#[test]
fn upgrade_check_blocks_a_target_cli_that_is_not_running() {
    let destination = temporary_directory("upgrade-target");
    write_trust_fixture(&destination, env!("CARGO_PKG_VERSION"));
    let target = if env!("CARGO_PKG_VERSION") == "999.0.0" {
        "998.0.0"
    } else {
        "999.0.0"
    };
    let output = pliego(
        &["upgrade", "--check", "--target", target, "--format", "json"],
        &destination,
    );
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "blocked");
    fs::remove_dir_all(destination).unwrap();
}

#[test]
fn new_without_template_builds_the_official_first_use_contract() {
    let destination = temporary_directory("default-scaffold");
    let framework = framework_root();
    let output = pliego(
        &[
            "new",
            destination.to_str().unwrap(),
            "--name",
            "First Fold",
            "--framework-path",
            framework.to_str().unwrap(),
        ],
        &std::env::temp_dir(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("PLIEGO template: default"));
    let main = fs::read_to_string(destination.join("src/main.rs")).unwrap();
    assert!(main.contains("First Fold"));
    assert!(!main.contains("__NAME__"));
    for path in [
        "README.md",
        "LICENSE",
        "src/domain.rs",
        "assets/favicon.svg",
        "assets/site.webmanifest",
        "assets/robots.txt",
    ] {
        assert!(destination.join(path).is_file(), "missing {path}");
    }
    let check = pliego(&["check"], &destination);
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let replay = Command::new("cargo")
        .args(["test", "--locked"])
        .current_dir(&destination)
        .output()
        .expect("run starter replay tests");
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let build = pliego(&["build"], &destination);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(destination.join("target/site/pliego.graph.json").is_file());
    let why = pliego(&["why", "artifact", "/"], &destination);
    assert!(why.status.success());
    let why = String::from_utf8(why.stdout).unwrap();
    assert!(why.contains("src/domain.rs"));
    assert!(why.contains("src/main.rs"));
    fs::remove_dir_all(&destination).expect("remove owned temporary scaffold");
}

#[test]
fn unknown_template_is_a_usage_error() {
    let destination = temporary_directory("unknown-template");
    let output = pliego(
        &[
            "new",
            destination.to_str().unwrap(),
            "--template",
            "missing",
        ],
        &std::env::temp_dir(),
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(!destination.exists());
}

#[test]
fn scaffold_is_transactional_and_json_tokens_are_escaped() {
    let destination = temporary_directory("scaffold");
    let framework = framework_root();
    let output = pliego(
        &[
            "new",
            destination.to_str().unwrap(),
            "--template",
            "minimal",
            "--name",
            "Mario's \"Proof\"",
            "--framework-path",
            framework.to_str().unwrap(),
        ],
        &std::env::temp_dir(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join("Cargo.toml").is_file());
    assert!(destination.join("pliego.toml").is_file());
    assert!(destination.join("src/main.rs").is_file());
    let manifest: Value =
        serde_json::from_slice(&fs::read(destination.join("assets/site.webmanifest")).unwrap())
            .expect("rendered web manifest remains JSON");
    assert_eq!(manifest["name"], "Mario's \"Proof\"");
    fs::remove_dir_all(&destination).expect("remove owned temporary scaffold");
}

#[test]
fn occupied_destination_is_never_overwritten() {
    let destination = temporary_directory("occupied");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "owned by user").unwrap();
    let output = pliego(
        &["new", destination.to_str().unwrap()],
        &std::env::temp_dir(),
    );
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "owned by user"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("PLG-NEW-001"));
    fs::remove_dir_all(&destination).expect("remove owned temporary destination");
}

fn pliego_with_env(
    arguments: &[&str],
    directory: &std::path::Path,
    name: &str,
    value: &str,
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pliego"))
        .args(arguments)
        .current_dir(directory)
        .env(name, value)
        .output()
        .expect("run pliego test binary with environment override")
}

#[test]
fn check_rejects_build_affecting_environment_overrides_before_cargo_runs() {
    let destination = temporary_directory("environment-override");
    fs::create_dir_all(&destination).unwrap();
    fs::write(
        destination.join("pliego.toml"),
        "[project]\nid = \"environment-proof\"\nname = \"Environment Proof\"\nsite_package = \"proof\"\noutput = \"target/site\"\n",
    )
    .unwrap();
    fs::write(
        destination.join("Cargo.toml"),
        "[package]\nname = \"proof\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir_all(destination.join("src")).unwrap();
    fs::write(destination.join("src/main.rs"), "fn main() {}\n").unwrap();

    let output = pliego_with_env(&["check"], &destination, "RUSTC_BOOTSTRAP", "pliego-proof");
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("PLG-ENV-001"), "{stderr}");
    assert!(stderr.contains("RUSTC_BOOTSTRAP"), "{stderr}");
    assert!(
        !destination.join("Cargo.lock").exists(),
        "environment rejection must happen before Cargo materializes a lockfile"
    );
    fs::remove_dir_all(&destination).expect("remove environment override fixture");
}

#[test]
fn cargo_target_env_cannot_write_inside_the_site_output() {
    let destination = temporary_directory("target-env-overlap");
    let nested = destination.join("src/nested");
    let output_root = destination.join("target/site");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(&output_root).unwrap();
    fs::write(
        destination.join("pliego.toml"),
        "[project]\nid = \"target-env-proof\"\nname = \"Target Env Proof\"\nsite_package = \"proof\"\noutput = \"target/site\"\n",
    )
    .unwrap();
    fs::write(
        destination.join("Cargo.toml"),
        "[package]\nname = \"proof\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(destination.join("src/main.rs"), "fn main() {}\n").unwrap();
    let sentinel = output_root.join("sentinel.txt");
    fs::write(&sentinel, b"must survive").unwrap();

    let output = pliego_with_env(&["build"], &nested, "CARGO_TARGET_DIR", "target/site");
    assert_eq!(output.status.code(), Some(5));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cargo target directory"), "{stderr}");
    assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
    assert!(!output_root.join("pliego.build.json").exists());
    fs::remove_dir_all(&destination).expect("remove target env overlap fixture");
}

#[test]
fn cargo_config_target_alias_cannot_overlap_portable_output() {
    let destination = temporary_directory("target-config-alias");
    let nested = destination.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(destination.join(".cargo")).unwrap();
    fs::write(
        destination.join("pliego.toml"),
        "[project]\nid = \"target-config-proof\"\nname = \"Target Config Proof\"\nsite_package = \"proof\"\noutput = \"target/site\"\n",
    )
    .unwrap();
    fs::write(
        destination.join("Cargo.toml"),
        "[package]\nname = \"proof\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(destination.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        destination.join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"target/SITE/cache\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_pliego"))
        .arg("check")
        .current_dir(&nested)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("run pliego with Cargo target directory selected from project config");
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cargo target directory"), "{stderr}");
    assert!(!destination.join("target/SITE/cache").exists());
    fs::remove_dir_all(&destination).expect("remove target config alias fixture");
}

#[test]
fn cargo_config_build_target_cannot_be_a_pliego_output_child() {
    let destination = temporary_directory("build-target-layout");
    let nested = destination.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(destination.join(".cargo")).unwrap();
    fs::write(
        destination.join("pliego.toml"),
        "[project]\nid = \"build-target-proof\"\nname = \"Build Target Proof\"\nsite_package = \"proof\"\noutput = \"target/x86_64-pc-windows-msvc/site\"\n",
    )
    .unwrap();
    fs::write(
        destination.join("Cargo.toml"),
        "[package]\nname = \"proof\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(destination.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        destination.join(".cargo/config.toml"),
        "[build]\ntarget = \"x86_64-pc-windows-msvc\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_pliego"))
        .arg("check")
        .current_dir(&nested)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("run pliego with a configured Cargo build target");
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cargo-owned target layout"), "{stderr}");
    fs::remove_dir_all(&destination).expect("remove configured build target fixture");
}

#[test]
fn project_commands_started_in_a_subdirectory_ignore_nested_toolchain_files() {
    let destination = temporary_directory("root-toolchain");
    let nested = destination.join("src/nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        destination.join("pliego.toml"),
        "[project]\nid = \"toolchain-proof\"\nname = \"Toolchain Proof\"\nsite_package = \"proof\"\noutput = \"target/site\"\n",
    )
    .unwrap();
    fs::write(
        destination.join("Cargo.toml"),
        "[package]\nname = \"proof\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(destination.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        nested.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = 7\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_pliego"))
        .arg("check")
        .current_dir(&nested)
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .expect("run pliego without the test harness toolchain override");
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pliego-ssg"),
        "check did not progress through root-scoped toolchain capture: {stderr}"
    );
    assert!(
        destination.join("Cargo.lock").is_file(),
        "root-scoped Cargo metadata did not run"
    );
    fs::remove_dir_all(&destination).expect("remove root toolchain fixture");
}

#[test]
fn portable_generated_path_aliases_fail_from_project_subdirectories() {
    let cases = [
        ("uppercase-target", "TARGET/site", None),
        ("windows-reserved", "target/CON", None),
        ("trailing-dot", "target/site.", None),
        ("reserved-state-alias", "target/.PLIEGO/site", None),
        ("case-prefix", "target/Site", Some("target/site/client")),
        (
            "unicode-fold-prefix",
            "target/Stra\u{df}e",
            Some("target/STRASSE/client"),
        ),
        ("non-nfc", "target/cafe\u{301}", None),
    ];

    for (label, output_path, bindgen_path) in cases {
        let destination = temporary_directory(label);
        let nested = destination.join("src/nested");
        fs::create_dir_all(&nested).unwrap();
        let client = bindgen_path.map_or_else(String::new, |path| {
            format!(
                "\n[client]\npackage = \"proof-client\"\nwasm_name = \"proof_client\"\nbindgen_output = \"{path}\"\n"
            )
        });
        fs::write(
            destination.join("pliego.toml"),
            format!(
                "[project]\nid = \"path-proof\"\nname = \"Path Proof\"\nsite_package = \"proof\"\noutput = \"{output_path}\"\n{client}"
            ),
        )
        .unwrap();

        let result = pliego(&["check"], &nested);
        assert_eq!(
            result.status.code(),
            Some(3),
            "portable alias {label:?} reached Cargo: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("PLG-PRJ-001"), "{label}: {stderr}");
        assert!(!destination.join("Cargo.lock").exists());
        fs::remove_dir_all(&destination).expect("remove portable path fixture");
    }
}

#[test]
fn first_use_build_is_reproducible_and_inspect_never_repairs_the_lock() {
    let destination = temporary_directory("first-build");
    let framework = framework_root();
    let scaffold = pliego(
        &[
            "new",
            destination.to_str().unwrap(),
            "--template",
            "minimal",
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
    assert!(!destination.join("Cargo.lock").exists());

    let first = pliego(&["build"], &destination);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(destination.join("Cargo.lock").is_file());
    let ledger = destination.join("target/site/pliego.build.json");
    let first_ledger = fs::read(&ledger).unwrap();
    let parsed: Value = serde_json::from_slice(&first_ledger).unwrap();
    assert_eq!(parsed["reportVersion"], "2.0.0");
    assert!(
        parsed["receipt"]["context"]["ownership"]["projectId"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    let ledger_text = String::from_utf8(first_ledger.clone()).unwrap();
    assert!(!ledger_text.contains(&destination.display().to_string()));
    assert!(!ledger_text.contains(&framework.display().to_string()));
    assert!(
        parsed["receipt"]["context"]["materials"]
            .as_array()
            .is_some_and(|materials| materials.iter().all(|material| {
                material.get("root").is_none()
                    && material.get("files").is_none()
                    && material["sha256"].as_str().is_some()
            }))
    );

    let inspect = pliego(&["inspect"], &destination);
    assert!(inspect.status.success());
    assert!(String::from_utf8_lossy(&inspect.stdout).contains("VERIFIED"));

    let second = pliego(&["build"], &destination);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(fs::read(&ledger).unwrap(), first_ledger);

    let lockfile = destination.join("Cargo.lock");
    let lock_bytes = fs::read(&lockfile).unwrap();
    fs::remove_file(&lockfile).unwrap();
    let missing_lock = pliego(&["inspect"], &destination);
    assert!(!missing_lock.status.success());
    assert!(
        !lockfile.exists(),
        "inspect must not materialize a missing Cargo.lock"
    );

    fs::write(&lockfile, &lock_bytes).unwrap();
    let cargo_manifest = destination.join("Cargo.toml");
    let manifest = fs::read_to_string(&cargo_manifest).unwrap();
    assert!(manifest.contains("version = \"0.1.0\""));
    fs::write(
        &cargo_manifest,
        manifest.replacen("version = \"0.1.0\"", "version = \"0.1.1\"", 1),
    )
    .unwrap();
    let stale_lock = pliego(&["inspect"], &destination);
    assert!(!stale_lock.status.success());
    assert_eq!(
        fs::read(&lockfile).unwrap(),
        lock_bytes,
        "inspect must not refresh a stale Cargo.lock"
    );
    fs::remove_dir_all(&destination).expect("remove owned first-use project");
}
