// SPDX-License-Identifier: Apache-2.0

use pliego_pboc::{
    ArtifactRole, BuildIdentity, CompatibilityIdentity, DeploymentTarget, FeatureRequirement,
    FrameworkIdentity, HostKind, PBOC_FILE_NAME, PbocArtifact, PbocAsset, PbocManifest,
    ProvenanceIdentity, feature, verify_bundle,
};
use provider_tck::{
    CLOUDFLARE_PBOC_WRAPPER, STATIC_BODY, STATIC_HEADERS, cache_policies, capabilities, functions,
    route_graph_sha256, routes, runtime_contract_sha256, telemetry_hooks,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() -> AppResult<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() < 6 || arguments.len() > 7 {
        return Err("usage: provider-tck-pack <native-binary> <cloudflare-dir> <output-root> <release-id> <sequence> <source-revision> [previous-release]".into());
    }
    let native = PathBuf::from(&arguments[0]);
    let cloudflare = PathBuf::from(&arguments[1]);
    let output = PathBuf::from(&arguments[2]);
    let release_id = &arguments[3];
    let sequence: u64 = arguments[4].parse()?;
    let revision = &arguments[5];
    let previous = arguments.get(6).cloned();
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("source revision must be 40 lowercase hexadecimal characters".into());
    }
    if output.exists() {
        fs::remove_dir_all(&output)?;
    }
    fs::create_dir_all(&output)?;

    // The artifact name follows the declared deployment target, not the host
    // that happens to run the packer.
    let native_name = "server";
    copy_file(&native, &output.join("artifacts/native").join(native_name))?;
    copy_tree(
        &cloudflare,
        &cloudflare,
        &output.join("artifacts/cloudflare"),
    )?;
    write(
        &output.join("artifacts/cloudflare/worker/pboc.mjs"),
        CLOUDFLARE_PBOC_WRAPPER.as_bytes(),
    )?;
    write(&output.join("public/asset.txt"), STATIC_BODY.as_bytes())?;
    write(&output.join("public/_headers"), STATIC_HEADERS.as_bytes())?;
    let sbom = serde_json::json!({
        "spdxVersion": "SPDX-2.3",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": format!("pliegors-provider-tck-{release_id}"),
        "dataLicense": "CC0-1.0",
        "documentNamespace": format!("https://pliegors.dev/sbom/{release_id}"),
        "creationInfo": { "creators": ["Organization: Celiums Solutions LLC"], "created": "2026-07-22T00:00:00Z" },
        "packages": [{ "SPDXID": "SPDXRef-Package", "name": "provider-tck", "versionInfo": env!("CARGO_PKG_VERSION"), "downloadLocation": "NOASSERTION", "filesAnalyzed": false }]
    });
    write(
        &output.join("supply/sbom.spdx.json"),
        &serde_json::to_vec(&sbom)?,
    )?;
    let provenance = serde_json::json!({
        "_type": "https://in-toto.io/Statement/v1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": [{ "name": "provider-tck", "digest": { "gitCommit": revision } }],
        "predicate": { "buildDefinition": { "buildType": "https://pliegors.dev/build/provider-tck/v1", "externalParameters": { "releaseId": release_id, "sequence": sequence } }, "runDetails": { "builder": { "id": "https://github.com/celiumsai/pliegors" } } }
    });
    write(
        &output.join("supply/provenance.intoto.json"),
        &serde_json::to_vec(&provenance)?,
    )?;

    let artifacts = capture_artifacts(&output)?;
    let artifact_ledger_sha256 = sha(&serde_json::to_vec(&artifacts)?);
    let cloudflare_paths = artifacts
        .iter()
        .filter(|artifact| artifact.path.starts_with("artifacts/cloudflare/"))
        .map(|artifact| artifact.path.clone())
        .collect::<Vec<_>>();
    let native_path = format!("artifacts/native/{native_name}");
    let manifest = PbocManifest {
        schema: pliego_pboc::PBOC_SCHEMA.to_owned(),
        framework: FrameworkIdentity {
            name: "pliegors".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            source_revision: revision.to_owned(),
        },
        build: BuildIdentity {
            application_id: "provider-tck".to_owned(),
            release_id: release_id.to_owned(),
            route_graph_sha256: route_graph_sha256(),
            runtime_contract_sha256: runtime_contract_sha256(),
            artifact_ledger_sha256,
            provenance: ProvenanceIdentity {
                sbom_path: "supply/sbom.spdx.json".to_owned(),
                provenance_path: "supply/provenance.intoto.json".to_owned(),
                source_revision: revision.to_owned(),
            },
        },
        compatibility: CompatibilityIdentity {
            epoch: 1,
            sequence,
            state_schema: "stateless-v1".to_owned(),
            previous_release_id: previous,
            rollback_safe: true,
        },
        capabilities: capabilities(),
        artifacts,
        targets: vec![
            DeploymentTarget {
                id: "cloudflare-workers".to_owned(),
                host_kind: HostKind::CloudflareWorkers,
                artifact_paths: cloudflare_paths,
                required_features: vec![
                    FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
                    FeatureRequirement::required(feature::HTTP_STREAM_ORDERED, 1),
                ],
                optional_features: Vec::new(),
            },
            DeploymentTarget {
                id: "native-linux-oci".to_owned(),
                host_kind: HostKind::NativeOci,
                artifact_paths: vec![native_path],
                required_features: vec![
                    FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
                    FeatureRequirement::required(feature::HTTP_STREAM_ORDERED, 1),
                ],
                optional_features: vec![FeatureRequirement::optional(
                    feature::HTTP_STREAM_BOUNDARY,
                    1,
                )],
            },
        ],
        assets: vec![PbocAsset {
            request_path: "/asset.txt".to_owned(),
            artifact_path: "public/asset.txt".to_owned(),
            cache_policy_id: "public-immutable".to_owned(),
            immutable: true,
        }],
        routes: routes(),
        functions: functions(),
        cache_policies: cache_policies(),
        permissions: Vec::new(),
        secret_references: Vec::new(),
        telemetry_hooks: telemetry_hooks(),
    };
    let bytes = manifest.canonical_bytes()?;
    write(&output.join(PBOC_FILE_NAME), &bytes)?;
    let verification = verify_bundle(&output, &manifest)?;
    println!("{}", serde_json::to_string(&verification)?);
    Ok(())
}

fn capture_artifacts(root: &Path) -> AppResult<Vec<PbocArtifact>> {
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    files
        .into_iter()
        .map(|relative| {
            let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
            let bytes = fs::read(&path)?;
            let (role, media_type) = if relative == "public/asset.txt" {
                (ArtifactRole::StaticAsset, "text/plain; charset=utf-8")
            } else if relative == "public/_headers" {
                (ArtifactRole::Configuration, "text/plain; charset=utf-8")
            } else if relative == "supply/sbom.spdx.json" {
                (ArtifactRole::Sbom, "application/spdx+json")
            } else if relative == "supply/provenance.intoto.json" {
                (ArtifactRole::Provenance, "application/vnd.in-toto+json")
            } else if relative.starts_with("artifacts/cloudflare/") {
                (ArtifactRole::CloudflareModule, media_type(&relative))
            } else {
                (ArtifactRole::NativeExecutable, "application/octet-stream")
            };
            Ok(PbocArtifact {
                path: relative,
                bytes: bytes.len() as u64,
                sha256: sha(&bytes),
                role,
                media_type: media_type.to_owned(),
            })
        })
        .collect()
}

fn collect(root: &Path, directory: &Path, output: &mut Vec<String>) -> AppResult<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, output)?;
        } else {
            output.push(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn copy_tree(root: &Path, directory: &Path, destination: &Path) -> AppResult<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path.strip_prefix(root)?;
        if relative == Path::new(".gitignore") {
            continue;
        }
        let target = destination.join(relative);
        if path.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(root, &path, destination)?;
        } else {
            copy_file(&path, &target)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> AppResult<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn sha(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn media_type(path: &str) -> &'static str {
    if path.ends_with(".wasm") {
        "application/wasm"
    } else if path.ends_with(".mjs") || path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".map") || path.ends_with(".json") {
        "application/json"
    } else {
        "application/octet-stream"
    }
}
