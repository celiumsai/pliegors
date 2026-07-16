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
