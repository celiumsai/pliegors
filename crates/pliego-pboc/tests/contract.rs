// SPDX-License-Identifier: Apache-2.0

use pliego_pboc::{
    ArtifactRole, BuildIdentity, CacheDomain, CachePolicy, CacheRevalidation,
    CompatibilityIdentity, DeploymentTarget, FeatureRequirement, FrameworkIdentity, HostKind,
    HostProfile, PbocArtifact, PbocAsset, PbocFunction, PbocManifest, PbocRoute, PbocRouter,
    ProvenanceIdentity, RenderMode, RouteKind, TelemetryHook, decode_manifest, feature,
    verify_bundle, verify_rollback_transition, verify_rolling_transition,
};
use std::fs;
use tempfile::tempdir;

const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn manifest(root: &std::path::Path) -> PbocManifest {
    let files = [
        (
            "artifacts/cloudflare/worker.wasm",
            b"cloudflare-module".as_slice(),
            ArtifactRole::CloudflareModule,
            "application/wasm",
        ),
        (
            "artifacts/native/server",
            b"native-executable".as_slice(),
            ArtifactRole::NativeExecutable,
            "application/octet-stream",
        ),
        (
            "public/index.html",
            b"<!doctype html><title>PBOC</title>".as_slice(),
            ArtifactRole::StaticAsset,
            "text/html; charset=utf-8",
        ),
        (
            "supply/provenance.intoto.jsonl",
            b"{\"predicateType\":\"https://slsa.dev/provenance/v1\"}\n".as_slice(),
            ArtifactRole::Provenance,
            "application/jsonl",
        ),
        (
            "supply/sbom.spdx.json",
            b"{\"spdxVersion\":\"SPDX-2.3\"}".as_slice(),
            ArtifactRole::Sbom,
            "application/spdx+json",
        ),
    ];
    for (path, bytes, _, _) in &files {
        let path = root.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(path.parent().expect("fixture path has parent")).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let artifacts = files
        .iter()
        .map(|(path, bytes, role, media_type)| PbocArtifact {
            path: (*path).to_owned(),
            bytes: bytes.len() as u64,
            sha256: sha(bytes),
            role: *role,
            media_type: (*media_type).to_owned(),
        })
        .collect();
    let capabilities = vec![
        FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1),
        FeatureRequirement::required(feature::CACHE_PUBLIC, 1),
        FeatureRequirement::required(feature::DEPLOYMENT_ROLLBACK, 1),
        FeatureRequirement::required(feature::DEPLOYMENT_ROLLING, 1),
        FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
        FeatureRequirement::required(feature::SECRETS_REFERENCES, 1),
        FeatureRequirement::required(feature::TELEMETRY_RECEIPTS, 1),
    ];
    PbocManifest {
        schema: "dev.pliegors.pboc/v1alpha1".to_owned(),
        framework: FrameworkIdentity {
            name: "pliegors".to_owned(),
            version: "0.3.0-alpha.1".to_owned(),
            source_revision: REVISION.to_owned(),
        },
        build: BuildIdentity {
            application_id: "provider-tck".to_owned(),
            release_id: "provider-tck-r1".to_owned(),
            route_graph_sha256: "b".repeat(64),
            runtime_contract_sha256: "c".repeat(64),
            artifact_ledger_sha256: "d".repeat(64),
            provenance: ProvenanceIdentity {
                sbom_path: "supply/sbom.spdx.json".to_owned(),
                provenance_path: "supply/provenance.intoto.jsonl".to_owned(),
                source_revision: REVISION.to_owned(),
            },
        },
        compatibility: CompatibilityIdentity {
            epoch: 1,
            sequence: 1,
            state_schema: "state-v1".to_owned(),
            previous_release_id: None,
            rollback_safe: true,
        },
        capabilities,
        artifacts,
        targets: vec![
            DeploymentTarget {
                id: "cloudflare-workers".to_owned(),
                host_kind: HostKind::CloudflareWorkers,
                artifact_paths: vec!["artifacts/cloudflare/worker.wasm".to_owned()],
                required_features: vec![FeatureRequirement::required(feature::HTTP_COMPLETE, 1)],
                optional_features: vec![FeatureRequirement::optional(
                    feature::HTTP_STREAM_ORDERED,
                    1,
                )],
            },
            DeploymentTarget {
                id: "native-linux-oci".to_owned(),
                host_kind: HostKind::NativeOci,
                artifact_paths: vec!["artifacts/native/server".to_owned()],
                required_features: vec![FeatureRequirement::required(feature::HTTP_COMPLETE, 1)],
                optional_features: vec![FeatureRequirement::optional(
                    feature::HTTP_STREAM_BOUNDARY,
                    1,
                )],
            },
        ],
        assets: vec![PbocAsset {
            request_path: "/".to_owned(),
            artifact_path: "public/index.html".to_owned(),
            cache_policy_id: "public-immutable".to_owned(),
            immutable: true,
        }],
        routes: vec![
            PbocRoute {
                id: "home".to_owned(),
                method: "GET".to_owned(),
                pattern: "/".to_owned(),
                kind: RouteKind::Static,
                asset_path: Some("public/index.html".to_owned()),
                function_id: None,
                render_mode: RenderMode::Complete,
                cache_policy_id: Some("public-immutable".to_owned()),
                required_features: vec![FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1)],
            },
            PbocRoute {
                id: "hello".to_owned(),
                method: "GET".to_owned(),
                pattern: "/api/hello".to_owned(),
                kind: RouteKind::Dynamic,
                asset_path: None,
                function_id: Some("hello".to_owned()),
                render_mode: RenderMode::Complete,
                cache_policy_id: Some("public-response".to_owned()),
                required_features: vec![FeatureRequirement::required(feature::HTTP_COMPLETE, 1)],
            },
        ],
        functions: vec![PbocFunction {
            id: "hello".to_owned(),
            entrypoint: "hello".to_owned(),
            render_modes: vec![RenderMode::Complete],
            max_response_bytes: 65_536,
            secret_references: Vec::new(),
            permission_ids: Vec::new(),
        }],
        cache_policies: vec![
            CachePolicy {
                id: "public-immutable".to_owned(),
                domain: CacheDomain::Public,
                revalidation: CacheRevalidation::Immutable,
                max_age_seconds: Some(31_536_000),
                stale_while_revalidate_seconds: None,
                vary_headers: Vec::new(),
                tags: vec!["assets".to_owned()],
            },
            CachePolicy {
                id: "public-response".to_owned(),
                domain: CacheDomain::Public,
                revalidation: CacheRevalidation::TimeBound,
                max_age_seconds: Some(30),
                stale_while_revalidate_seconds: Some(60),
                vary_headers: vec!["accept-language".to_owned()],
                tags: vec!["hello".to_owned()],
            },
        ],
        permissions: Vec::new(),
        secret_references: Vec::new(),
        telemetry_hooks: vec![TelemetryHook {
            id: "request-receipt".to_owned(),
            signal: "receipt".to_owned(),
            required: false,
            redacted_fields: vec!["authorization".to_owned(), "cookie".to_owned()],
        }],
    }
}

fn host(kind: HostKind, target: &str) -> HostProfile {
    HostProfile {
        host_id: "test.host".to_owned(),
        host_version: "1.0.0".to_owned(),
        target_id: target.to_owned(),
        host_kind: kind,
        features: vec![
            FeatureRequirement::required(feature::ASSETS_IMMUTABLE, 1),
            FeatureRequirement::required(feature::CACHE_PUBLIC, 1),
            FeatureRequirement::required(feature::DEPLOYMENT_ROLLBACK, 1),
            FeatureRequirement::required(feature::DEPLOYMENT_ROLLING, 1),
            FeatureRequirement::required(feature::HTTP_COMPLETE, 1),
            FeatureRequirement::required(feature::SECRETS_REFERENCES, 1),
            FeatureRequirement::required(feature::TELEMETRY_RECEIPTS, 1),
        ],
        max_artifact_bytes: 1024 * 1024,
        max_bundle_bytes: 10 * 1024 * 1024,
    }
}

#[test]
fn canonical_round_trip_and_both_hosts_admit_the_same_manifest() {
    let root = tempdir().unwrap();
    let manifest = manifest(root.path());
    let bytes = manifest.canonical_bytes().unwrap();
    assert_eq!(decode_manifest(&bytes).unwrap(), manifest);
    let native = manifest
        .admit(&host(HostKind::NativeOci, "native-linux-oci"))
        .unwrap();
    let cloudflare = manifest
        .admit(&host(HostKind::CloudflareWorkers, "cloudflare-workers"))
        .unwrap();
    assert_eq!(native.manifest_sha256, cloudflare.manifest_sha256);
    assert_eq!(native.required_secret_references, Vec::<String>::new());
    assert_eq!(
        cloudflare.unsupported_optional_features,
        vec!["http.stream.ordered@1"]
    );
}

#[test]
fn missing_required_feature_is_rejected_before_upload() {
    let root = tempdir().unwrap();
    let manifest = manifest(root.path());
    let mut host = host(HostKind::CloudflareWorkers, "cloudflare-workers");
    host.features
        .retain(|feature| feature.id != feature::HTTP_COMPLETE);
    let error = manifest.admit(&host).unwrap_err().to_string();
    assert!(error.contains("http.complete@1"));
}

#[test]
fn exact_bundle_is_verified_and_tampering_fails() {
    let root = tempdir().unwrap();
    let manifest = manifest(root.path());
    let receipt = verify_bundle(root.path(), &manifest).unwrap();
    assert_eq!(receipt.artifact_count, 5);
    fs::write(root.path().join("public/index.html"), b"tampered").unwrap();
    assert!(verify_bundle(root.path(), &manifest).is_err());
}

#[test]
fn unknown_json_fields_and_secret_like_purpose_are_rejected() {
    let root = tempdir().unwrap();
    let base_manifest = manifest(root.path());
    let mut value = serde_json::to_value(&base_manifest).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("providerToken".to_owned(), serde_json::json!("secret"));
    assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());

    let mut manifest = manifest(root.path());
    manifest
        .secret_references
        .push(pliego_pboc::SecretReference {
            id: "api-key".to_owned(),
            purpose: "TOKEN=leak".to_owned(),
            required: true,
        });
    assert!(
        manifest
            .validate()
            .unwrap_err()
            .to_string()
            .contains("appears to contain a value")
    );
}

#[test]
fn sealed_router_preserves_dispatch_and_method_errors() {
    let root = tempdir().unwrap();
    let manifest = manifest(root.path());
    let router = PbocRouter::new(&manifest).unwrap();
    assert_eq!(
        router.resolve("GET", "/api/hello").unwrap().route.id,
        "hello"
    );
    let method_error = router.resolve("POST", "/api/hello").unwrap_err();
    assert_eq!(method_error.status_code(), 405);
    assert_eq!(method_error.allow_header().as_deref(), Some("GET"));
    assert_eq!(
        router.resolve("GET", "/missing").unwrap_err().status_code(),
        404
    );
}

#[test]
fn rolling_and_rollback_require_an_exact_compatible_release_chain() {
    let root = tempdir().unwrap();
    let active = manifest(root.path());
    let mut candidate = active.clone();
    candidate.build.release_id = "provider-tck-r2".to_owned();
    candidate.compatibility.sequence = 2;
    candidate.compatibility.previous_release_id = Some(active.build.release_id.clone());

    let rolling = verify_rolling_transition(&active, &candidate).unwrap();
    assert_eq!(rolling.from_release_id, "provider-tck-r1");
    assert_eq!(rolling.to_release_id, "provider-tck-r2");
    let rollback = verify_rollback_transition(&candidate, &active).unwrap();
    assert_eq!(rollback.from_sequence, 2);
    assert_eq!(rollback.to_sequence, 1);

    let mut incompatible = candidate.clone();
    incompatible.compatibility.state_schema = "state-v2".to_owned();
    assert_eq!(
        verify_rolling_transition(&active, &incompatible)
            .unwrap_err()
            .code(),
        "PLG-PBOC-103"
    );
    let mut unsafe_active = candidate;
    unsafe_active.compatibility.rollback_safe = false;
    assert_eq!(
        verify_rollback_transition(&unsafe_active, &active)
            .unwrap_err()
            .code(),
        "PLG-PBOC-106"
    );
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
