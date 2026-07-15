// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "pliego-r1-{label}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).expect("create fixture directory");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn project_context(root: &Path, client_package: Option<&str>) -> Context {
    Context {
        root: root.canonicalize().expect("canonical project root"),
        manifest: ProjectManifest {
            project: Project {
                id: "proof-site".to_owned(),
                name: "Proof Site".to_owned(),
                site_package: "site".to_owned(),
                output: PathBuf::from("target/site"),
            },
            client: client_package.map(|package| Client {
                package: package.to_owned(),
                wasm_name: "proof_client".to_owned(),
                bindgen_output: PathBuf::from("target/client/pkg"),
            }),
        },
    }
}

fn package(name: &str, id: &str, root: &Path) -> CargoPackage {
    fs::create_dir_all(root.join("src")).expect("create package source directory");
    if !root.join("Cargo.toml").exists() {
        fs::write(
            root.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        )
        .expect("write package manifest");
    }
    if !root.join("src/lib.rs").exists() {
        fs::write(root.join("src/lib.rs"), "pub const VALUE: u8 = 1;\n")
            .expect("write package source");
    }
    CargoPackage {
        name: name.to_owned(),
        version: "0.1.0".to_owned(),
        id: id.to_owned(),
        source: None,
        manifest_path: root.join("Cargo.toml"),
        targets: Vec::new(),
    }
}

fn node(id: &str, dependencies: &[&str]) -> CargoResolveNode {
    CargoResolveNode {
        id: id.to_owned(),
        dependencies: dependencies
            .iter()
            .map(|dependency| (*dependency).to_owned())
            .collect(),
    }
}

fn metadata(
    workspace_root: &Path,
    packages: Vec<CargoPackage>,
    nodes: Vec<CargoResolveNode>,
) -> CargoMetadata {
    CargoMetadata {
        packages,
        workspace_root: workspace_root.to_owned(),
        target_directory: workspace_root.join("target"),
        resolve: Some(CargoResolve { nodes }),
    }
}

fn seed_project(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("create project source directory");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"site\"\nversion = \"0.1.0\"\n",
    )
    .expect("write project manifest");
    fs::write(
        root.join("pliego.toml"),
        "[project]\nid = \"proof-site\"\nname = \"Proof\"\nsite_package = \"site\"\noutput = \"target/site\"\n",
    )
    .expect("write Pliego manifest");
    fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write project source");
}

fn seed_workspace(root: &Path) {
    fs::create_dir_all(root.join(".cargo")).expect("create Cargo configuration directory");
    fs::write(root.join("Cargo.lock"), "version = 4\n").expect("write workspace lockfile");
    fs::write(root.join("Cargo.toml"), "[workspace]\nresolver = \"3\"\n")
        .expect("write workspace manifest");
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nincremental = true\n",
    )
    .expect("write workspace Cargo configuration");
}

fn capture_for(context: &Context, specs: &[InputMaterialSpec]) -> pliego_artifact::BuildContext {
    capture_build_context_with_materials(
        &context.root,
        Ownership {
            project_id: context.manifest.project.id.clone(),
            site_package: context.manifest.project.site_package.clone(),
        },
        FrameworkEvidence {
            version: "test".to_owned(),
            source_revision: "test-revision".to_owned(),
        },
        &["Cargo.toml".to_owned(), "pliego.toml".to_owned()],
        &["target/site".to_owned()],
        specs,
    )
    .expect("capture fixture context")
}

fn capture_selection(
    context: &Context,
    selection: &CargoInputSelection,
) -> pliego_artifact::BuildContext {
    capture_build_context_with_materials(
        &context.root,
        Ownership {
            project_id: context.manifest.project.id.clone(),
            site_package: context.manifest.project.site_package.clone(),
        },
        FrameworkEvidence {
            version: "test".to_owned(),
            source_revision: "test-revision".to_owned(),
        },
        &selection.project_configuration,
        &["target/site".to_owned()],
        &selection.materials,
    )
    .expect("capture selected fixture context")
}

fn material_ids(specs: &[InputMaterialSpec]) -> BTreeSet<&str> {
    specs.iter().map(|spec| spec.id.as_str()).collect()
}

#[test]
fn private_build_invocation_binds_the_manifest_output_path() {
    let fixture = Fixture::new("invocation-output");
    let project = fixture.directory("project");
    seed_project(&project);
    let context = project_context(&project, None);
    let build_context = capture_for(&context, &[]);

    let path = write_project_build_context(&context, &build_context, &[])
        .expect("write validated private build invocation");
    let invocation: serde_json::Value =
        serde_json::from_slice(&fs::read(path).expect("read invocation"))
            .expect("parse invocation JSON");
    assert_eq!(invocation["outputPath"], "target/site");
}

#[test]
fn cargo_target_directory_is_disjoint_or_uses_only_safe_default_children() {
    let fixture = Fixture::new("cargo-target-layout");
    let project = fixture.directory("project");
    seed_project(&project);
    let mut context = project_context(&project, None);
    let mut cargo = metadata(&project, Vec::new(), Vec::new());

    validate_cargo_target_directory(&context, &cargo).expect("default target layout is valid");

    for relative in [
        "target/site",
        "target/site/cargo",
        "target/SITE/cache",
        "target/.PLIEGO/cache",
    ] {
        cargo.target_directory = project.join(relative);
        assert!(
            validate_cargo_target_directory(&context, &cargo).is_err(),
            "overlapping Cargo target directory was accepted: {relative}"
        );
    }

    cargo.target_directory = project.join("target/cargo");
    validate_cargo_target_directory(&context, &cargo)
        .expect("disjoint alternate Cargo target directory is valid");

    cargo.target_directory = project.join("target");
    for generated in [
        "target/debug/site",
        "target/release/site",
        "target/wasm32-unknown-unknown/site",
        "target/x86_64-pc-windows-msvc/site",
        "target/aarch64-unknown-linux-gnu/site",
    ] {
        context.manifest.project.output = PathBuf::from(generated);
        assert!(
            validate_cargo_target_directory(&context, &cargo).is_err(),
            "Cargo-owned default target child was accepted: {generated}"
        );
    }

    fs::create_dir_all(project.join(".cargo")).expect("create Cargo config directory");
    fs::write(
        project.join(".cargo/config.toml"),
        "[build]\ntarget = \"targets/custom.json\"\n",
    )
    .expect("write custom Cargo build target");
    context.manifest.project.output = PathBuf::from("target/custom/site");
    assert!(
        validate_cargo_target_directory(&context, &cargo).is_err(),
        "custom Cargo build target output was accepted as a Pliego generated path"
    );
}

#[test]
fn parent_workspace_lock_and_cargo_configuration_are_evidence() {
    let fixture = Fixture::new("parent-workspace");
    let workspace = fixture.directory("workspace");
    let project = fixture.directory("workspace/site");
    seed_workspace(&workspace);
    seed_project(&project);
    let context = project_context(&project, None);
    let metadata = metadata(
        &workspace,
        vec![package("site", "site-id", &project)],
        vec![node("site-id", &[])],
    );
    let specs = cargo_input_materials(&context, &metadata).expect("resolve input materials");
    let expected = capture_for(&context, &specs);

    fs::write(workspace.join("Cargo.lock"), "version = 4\n# changed\n")
        .expect("change parent lockfile");
    assert!(verify_build_context_with_materials(&project, &specs, &expected).is_err());

    fs::write(workspace.join("Cargo.lock"), "version = 4\n").expect("restore parent lockfile");
    verify_build_context_with_materials(&project, &specs, &expected)
        .expect("restored parent lockfile verifies");

    fs::write(
        workspace.join(".cargo/config.toml"),
        "[build]\nincremental = false\n",
    )
    .expect("change parent Cargo configuration");
    assert!(verify_build_context_with_materials(&project, &specs, &expected).is_err());
}

#[test]
fn transitive_external_path_dependency_is_part_of_the_receipt_context() {
    let fixture = Fixture::new("transitive-path");
    let project = fixture.directory("project");
    let direct = fixture.directory("deps/direct");
    let transitive = fixture.directory("deps/transitive");
    seed_workspace(&project);
    seed_project(&project);
    let context = project_context(&project, None);
    let metadata = metadata(
        &project,
        vec![
            package("site", "site-id", &project),
            package("direct", "direct-id", &direct),
            package("transitive", "transitive-id", &transitive),
        ],
        vec![
            node("site-id", &["direct-id"]),
            node("direct-id", &["transitive-id"]),
            node("transitive-id", &[]),
        ],
    );
    let specs = cargo_input_materials(&context, &metadata).expect("resolve transitive materials");
    assert!(material_ids(&specs).contains("cargo-path/transitive@0.1.0"));
    let expected = capture_for(&context, &specs);

    fs::write(transitive.join("src/lib.rs"), "pub const VALUE: u8 = 2;\n")
        .expect("change transitive source");
    assert!(verify_build_context_with_materials(&project, &specs, &expected).is_err());
}

#[test]
fn client_package_contributes_its_exclusive_dependency_closure() {
    let fixture = Fixture::new("client-closure");
    let project = fixture.directory("project");
    let site = fixture.directory("project/site");
    let client = fixture.directory("project/client");
    let site_dependency = fixture.directory("deps/site-only");
    let client_dependency = fixture.directory("deps/client-only");
    let unreachable = fixture.directory("deps/unreachable");
    seed_workspace(&project);
    seed_project(&project);
    let context = project_context(&project, Some("client"));
    let metadata = metadata(
        &project,
        vec![
            package("site", "site-id", &site),
            package("client", "client-id", &client),
            package("site-only", "site-dep-id", &site_dependency),
            package("client-only", "client-dep-id", &client_dependency),
            package("unreachable", "unreachable-id", &unreachable),
        ],
        vec![
            node("site-id", &["site-dep-id"]),
            node("client-id", &["client-dep-id"]),
            node("site-dep-id", &[]),
            node("client-dep-id", &[]),
            node("unreachable-id", &[]),
        ],
    );

    let reachable = reachable_package_ids(&context, &metadata).expect("resolve client closure");
    assert!(reachable.contains("site-id"));
    assert!(reachable.contains("site-dep-id"));
    assert!(reachable.contains("client-id"));
    assert!(reachable.contains("client-dep-id"));
    assert!(!reachable.contains("unreachable-id"));

    let specs = cargo_input_materials(&context, &metadata).expect("capture client materials");
    let ids = material_ids(&specs);
    assert!(ids.contains("cargo-path/site-only@0.1.0"));
    assert!(ids.contains("cargo-path/client-only@0.1.0"));
    assert!(!ids.contains("cargo-path/unreachable@0.1.0"));
}

#[test]
fn unreachable_workspace_member_does_not_change_evidence() {
    let fixture = Fixture::new("unreachable-member");
    let workspace = fixture.directory("workspace");
    let project = fixture.directory("workspace/site");
    let unreachable = fixture.directory("workspace/unreachable");
    seed_workspace(&workspace);
    seed_project(&project);
    let context = project_context(&project, None);
    let metadata = metadata(
        &workspace,
        vec![
            package("site", "site-id", &project),
            package("unreachable", "unreachable-id", &unreachable),
        ],
        vec![node("site-id", &[]), node("unreachable-id", &[])],
    );
    let specs = cargo_input_materials(&context, &metadata).expect("resolve workspace materials");
    assert!(!material_ids(&specs).contains("cargo-path/unreachable@0.1.0"));
    let expected = capture_for(&context, &specs);

    fs::write(
        unreachable.join("src/lib.rs"),
        "pub const UNREACHABLE: u8 = 99;\n",
    )
    .expect("change unreachable workspace member");
    verify_build_context_with_materials(&project, &specs, &expected)
        .expect("unreachable member must not affect evidence");
}

#[test]
fn duplicate_and_overlapping_local_package_roots_fail_closed() {
    let fixture = Fixture::new("overlap");
    let project = fixture.directory("project");
    let shared = fixture.directory("deps/shared");
    seed_workspace(&project);
    seed_project(&project);
    let context = project_context(&project, None);
    let duplicate = metadata(
        &project,
        vec![
            package("site", "site-id", &project),
            package("first", "first-id", &shared),
            package("second", "second-id", &shared),
        ],
        vec![
            node("site-id", &["first-id", "second-id"]),
            node("first-id", &[]),
            node("second-id", &[]),
        ],
    );
    let error = cargo_input_materials(&context, &duplicate).expect_err("duplicate root rejected");
    assert!(
        error.contains("overlaps another build input root"),
        "{error}"
    );

    let parent = fixture.directory("deps/parent");
    let child = fixture.directory("deps/parent/child");
    let overlapping = metadata(
        &project,
        vec![
            package("site", "site-id", &project),
            package("parent", "parent-id", &parent),
            package("child", "child-id", &child),
        ],
        vec![
            node("site-id", &["parent-id", "child-id"]),
            node("parent-id", &[]),
            node("child-id", &[]),
        ],
    );
    let error =
        cargo_input_materials(&context, &overlapping).expect_err("overlapping roots rejected");
    assert!(
        error.contains("overlaps another build input root"),
        "{error}"
    );
}

#[test]
fn framework_evidence_tracks_the_resolved_local_pliego_ssg_package() {
    let fixture = Fixture::new("framework-evidence");
    let project = fixture.directory("project");
    let framework = fixture.directory("framework/pliego-ssg");
    seed_workspace(&project);
    seed_project(&project);
    let context = project_context(&project, None);
    let site = package("site", "site-id", &project);
    let mut ssg = package("pliego-ssg", "ssg-id", &framework);
    ssg.version = "9.8.7".to_owned();
    fs::write(
        project.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"pliego-ssg\"\nversion = \"9.8.7\"\n",
    )
    .expect("write resolved framework lock entry");
    let metadata = metadata(
        &project,
        vec![site, ssg],
        vec![node("site-id", &["ssg-id"]), node("ssg-id", &[])],
    );

    let (before, _) =
        capture_project_build_context(&context, &metadata).expect("capture framework evidence");
    assert_eq!(before.framework.version, "9.8.7");
    assert!(before.framework.source_revision.starts_with("sha256:"));
    assert_eq!(before.framework.source_revision.len(), "sha256:".len() + 64);

    fs::write(
        framework.join("src/lib.rs"),
        "pub const FRAMEWORK_VALUE: u8 = 2;\n",
    )
    .expect("change resolved framework source");
    let (after, _) = capture_project_build_context(&context, &metadata)
        .expect("recapture changed framework evidence");
    assert_ne!(
        before.framework.source_revision,
        after.framework.source_revision
    );
}

#[test]
fn framework_evidence_fails_when_metadata_and_lock_disagree() {
    let fixture = Fixture::new("framework-lock-disagreement");
    let project = fixture.directory("project");
    let framework = fixture.directory("framework/pliego-ssg");
    seed_workspace(&project);
    seed_project(&project);
    let context = project_context(&project, None);
    let site = package("site", "site-id", &project);
    let mut ssg = package("pliego-ssg", "ssg-id", &framework);
    ssg.version = "2.0.0".to_owned();
    fs::write(
        project.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"pliego-ssg\"\nversion = \"1.0.0\"\n",
    )
    .expect("write mismatched framework lock entry");
    let metadata = metadata(
        &project,
        vec![site, ssg],
        vec![node("site-id", &["ssg-id"]), node("ssg-id", &[])],
    );

    let error = capture_project_build_context(&context, &metadata)
        .expect_err("mismatched lock must fail closed");
    assert!(error.contains("absent from"), "{error}");
}

#[test]
fn effective_ancestor_toolchain_and_cargo_home_configs_are_bound() {
    let fixture = Fixture::new("effective-config-chain");
    let outer = fixture.directory("outer");
    let gap = fixture.directory("outer/gap");
    let project = fixture.directory("outer/gap/project");
    let cargo_home = fixture.directory("cargo-home");
    seed_workspace(&project);
    seed_project(&project);
    fs::remove_file(project.join(".cargo/config.toml")).expect("remove seeded local config");
    fs::write(
        project.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\n",
    )
    .expect("write local toolchain file");
    fs::write(
        outer.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\n",
    )
    .expect("write ancestor toolchain file");
    fs::write(
        cargo_home.join("config.toml"),
        "[build]\nincremental = true\n",
    )
    .expect("write Cargo home config");

    let context = project_context(&project, None);
    let metadata = metadata(
        &project,
        vec![package("site", "site-id", &project)],
        vec![node("site-id", &[])],
    );
    let selection = cargo_input_selection_with_home(&context, &metadata, &cargo_home)
        .expect("select effective configuration");
    assert!(
        selection
            .project_configuration
            .contains(&"rust-toolchain.toml".to_owned())
    );
    assert!(selection.materials.iter().any(|material| {
        material.kind == "cargo-ancestor-config"
            && material.root == outer.canonicalize().unwrap()
            && material
                .included_paths
                .contains(&"rust-toolchain.toml".to_owned())
    }));
    assert!(selection.materials.iter().any(|material| {
        material.id == "cargo-config/home"
            && material.root == cargo_home.canonicalize().unwrap()
            && material.included_paths == ["config.toml"]
    }));
    let expected = capture_selection(&context, &selection);

    fs::write(
        cargo_home.join("config.toml"),
        "[build]\nincremental = false\n",
    )
    .expect("change Cargo home config");
    assert!(
        verify_build_context_with_materials(&context.root, &selection.materials, &expected)
            .is_err()
    );
    fs::write(
        cargo_home.join("config.toml"),
        "[build]\nincremental = true\n",
    )
    .expect("restore Cargo home config");

    fs::create_dir_all(gap.join(".cargo")).expect("create intermediate Cargo config directory");
    fs::write(gap.join(".cargo/config.toml"), "[net]\nretry = 1\n")
        .expect("add intermediate config");
    let changed = cargo_input_selection_with_home(&context, &metadata, &cargo_home)
        .expect("reselect effective configuration");
    assert_ne!(
        changed.materials, selection.materials,
        "newly effective config must change the material topology"
    );
}

#[test]
fn ancestor_material_ids_do_not_depend_on_checkout_depth() {
    let fixture = Fixture::new("config-depth");
    let shallow_base = fixture.directory("shallow");
    let shallow_project = fixture.directory("shallow/project");
    let deep_base = fixture.directory("deep");
    let deep_project = fixture.directory("deep/a/b/c/project");
    let cargo_home = fixture.directory("cargo-home");
    for (base, project) in [
        (&shallow_base, &shallow_project),
        (&deep_base, &deep_project),
    ] {
        seed_workspace(project);
        seed_project(project);
        fs::remove_file(project.join(".cargo/config.toml")).expect("remove local config");
        fs::create_dir_all(base.join(".cargo")).expect("create ancestor config directory");
        fs::write(
            base.join(".cargo/config.toml"),
            "[build]\nincremental = true\n",
        )
        .expect("write ancestor config");
    }
    let shallow_context = project_context(&shallow_project, None);
    let deep_context = project_context(&deep_project, None);
    let shallow_metadata = metadata(
        &shallow_project,
        vec![package("site", "site-shallow", &shallow_project)],
        vec![node("site-shallow", &[])],
    );
    let deep_metadata = metadata(
        &deep_project,
        vec![package("site", "site-deep", &deep_project)],
        vec![node("site-deep", &[])],
    );
    let shallow = cargo_input_selection_with_home(&shallow_context, &shallow_metadata, &cargo_home)
        .expect("select shallow inputs");
    let deep = cargo_input_selection_with_home(&deep_context, &deep_metadata, &cargo_home)
        .expect("select deep inputs");
    let shape = |selection: &CargoInputSelection| {
        selection
            .materials
            .iter()
            .filter(|material| material.kind == "cargo-ancestor-config")
            .map(|material| (material.id.clone(), material.included_paths.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&shallow), shape(&deep));
}

#[test]
fn git_and_registry_framework_provenance_comes_from_the_resolved_lock() {
    let fixture = Fixture::new("resolved-framework-sources");
    let workspace = fixture.directory("workspace");
    seed_workspace(&workspace);
    let revision = "a".repeat(40);
    let git_source = format!("git+https://example.invalid/pliegors?rev=main#{revision}");
    let mut git = package("pliego-ssg", "git-ssg", &fixture.directory("git-ssg"));
    git.version = "1.2.3".to_owned();
    git.source = Some(git_source.clone());
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"pliego-ssg\"\nversion = \"1.2.3\"\nsource = \"{git_source}\"\n"
        ),
    )
    .expect("write Git lock entry");
    let git_metadata = metadata(&workspace, vec![git], Vec::new());
    let evidence = resolved_framework_evidence(&git_metadata, &git_metadata.packages[0], None)
        .expect("resolve Git provenance");
    assert_eq!(evidence.source_revision, revision);

    let registry_source = "registry+https://github.com/rust-lang/crates.io-index";
    let checksum = "b".repeat(64);
    let mut registry = package(
        "pliego-ssg",
        "registry-ssg",
        &fixture.directory("registry-ssg"),
    );
    registry.version = "4.5.6".to_owned();
    registry.source = Some(registry_source.to_owned());
    fs::write(
        workspace.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"pliego-ssg\"\nversion = \"4.5.6\"\nsource = \"{registry_source}\"\nchecksum = \"{checksum}\"\n"
        ),
    )
    .expect("write registry lock entry");
    let registry_metadata = metadata(&workspace, vec![registry], Vec::new());
    let evidence =
        resolved_framework_evidence(&registry_metadata, &registry_metadata.packages[0], None)
            .expect("resolve registry provenance");
    assert_eq!(evidence.source_revision, format!("sha256:{checksum}"));
}

#[test]
fn framework_provenance_requires_one_reachable_pliego_ssg() {
    let fixture = Fixture::new("framework-cardinality");
    let project = fixture.directory("project");
    seed_workspace(&project);
    seed_project(&project);
    let context = project_context(&project, None);
    let metadata = metadata(
        &project,
        vec![package("site", "site-id", &project)],
        vec![node("site-id", &[])],
    );
    let error = resolved_framework_package(&context, &metadata)
        .expect_err("missing framework must fail provenance");
    assert!(error.contains("does not contain pliego-ssg"), "{error}");
}
