// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

fn pliego(arguments: &[&str], directory: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pliego"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("run pliego test binary")
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

#[test]
fn unknown_command_is_usage_even_outside_a_project() {
    let output = pliego(&["unknown-command"], &std::env::temp_dir());
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("PLG-ARG-001"));
    assert!(stderr.contains("unknown command"));
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
        "assets/favicon.svg",
        "assets/site.webmanifest",
        "assets/robots.txt",
    ] {
        assert!(destination.join(path).is_file(), "missing {path}");
    }
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
