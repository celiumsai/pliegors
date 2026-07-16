// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

use pliego_artifact::{
    BUILD_GRAPH_NAME, BuildGraph, BuildReport, SourceDependencies, decode_build_graph,
    validate_build_graph_against_report, verify_build_report,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

const REBUILD_RECORD_VERSION: &str = "pliego-rebuild/1";
const REBUILD_RECORD_PATH: &str = "target/.pliego/last-rebuild.json";
const MAX_REBUILD_RECORD_BYTES: usize = 1024 * 1024;
const MAX_REBUILD_ITEMS: usize = 100_000;

#[derive(Clone, Debug)]
pub(crate) struct VerifiedGraph {
    pub graph: BuildGraph,
    pub report: BuildReport,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HmrKind {
    None,
    Css,
    Content,
    Adapter,
    Reload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HmrUpdate {
    pub kind: HmrKind,
    pub paths: Vec<String>,
    pub routes: Vec<String>,
}

impl HmrUpdate {
    pub(crate) fn reload() -> Self {
        Self {
            kind: HmrKind::Reload,
            paths: Vec::new(),
            routes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RebuildRecord {
    pub record_version: String,
    pub generation: u64,
    pub changed_sources: Vec<String>,
    pub affected_routes: Vec<String>,
    pub affected_artifacts: Vec<String>,
    pub changed_artifacts: Vec<String>,
    pub hmr: HmrUpdate,
    pub receipt_before: Option<String>,
    pub receipt_after: String,
}

pub(crate) fn load_verified_graph(output_root: &Path) -> Result<VerifiedGraph, String> {
    let verified = verify_build_report(output_root).map_err(|error| error.to_string())?;
    let graph_path = output_root.join(BUILD_GRAPH_NAME);
    let bytes = fs::read(&graph_path)
        .map_err(|error| format!("cannot read {}: {error}", graph_path.display()))?;
    let graph = decode_build_graph(&bytes).map_err(|error| error.to_string())?;
    validate_build_graph_against_report(&graph, &verified.report)
        .map_err(|error| error.to_string())?;
    Ok(VerifiedGraph {
        graph,
        report: verified.report,
    })
}

pub(crate) fn explain_rebuild(
    generation: u64,
    changed_sources: BTreeSet<String>,
    before: Option<&VerifiedGraph>,
    after: &VerifiedGraph,
) -> RebuildRecord {
    let known_sources = before
        .into_iter()
        .flat_map(|build| build.graph.sources.iter())
        .chain(after.graph.sources.iter())
        .map(|source| source.path.as_str())
        .collect::<BTreeSet<_>>();
    let global_change = changed_sources
        .iter()
        .any(|path| !known_sources.contains(path.as_str()));

    let mut affected_routes = BTreeSet::new();
    let mut affected_artifacts = BTreeSet::new();
    for graph in before.into_iter().chain(std::iter::once(after)) {
        for route in &graph.graph.routes {
            if global_change || dependencies_touch(&route.sources, &changed_sources) {
                affected_routes.insert(route.route.clone());
                affected_artifacts.extend(route.artifacts.iter().cloned());
            }
        }
        for artifact in &graph.graph.artifacts {
            if global_change || dependencies_touch(&artifact.sources, &changed_sources) {
                affected_artifacts.insert(artifact.path.clone());
            }
        }
    }

    let before_artifacts = before
        .map(|build| artifact_hashes(&build.graph))
        .unwrap_or_default();
    let after_artifacts = artifact_hashes(&after.graph);
    let changed_artifacts = before_artifacts
        .keys()
        .chain(after_artifacts.keys())
        .filter(|path| before_artifacts.get(*path) != after_artifacts.get(*path))
        .cloned()
        .collect::<BTreeSet<_>>();
    let hmr = classify_hmr(&changed_artifacts, &affected_routes, &after.graph);

    RebuildRecord {
        record_version: REBUILD_RECORD_VERSION.to_owned(),
        generation,
        changed_sources: changed_sources.into_iter().collect(),
        affected_routes: affected_routes.into_iter().collect(),
        affected_artifacts: affected_artifacts.into_iter().collect(),
        changed_artifacts: changed_artifacts.into_iter().collect(),
        hmr,
        receipt_before: before.map(|build| build.report.receipt_sha256.clone()),
        receipt_after: after.report.receipt_sha256.clone(),
    }
}

fn dependencies_touch(
    dependencies: &SourceDependencies,
    changed_sources: &BTreeSet<String>,
) -> bool {
    match dependencies {
        SourceDependencies::AllSources => !changed_sources.is_empty(),
        SourceDependencies::Explicit { paths } => paths
            .iter()
            .any(|path| changed_sources.contains(path.as_str())),
    }
}

fn artifact_hashes(graph: &BuildGraph) -> BTreeMap<String, String> {
    graph
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.clone(), artifact.sha256.clone()))
        .collect()
}

fn classify_hmr(
    changed: &BTreeSet<String>,
    affected_routes: &BTreeSet<String>,
    graph: &BuildGraph,
) -> HmrUpdate {
    if changed.is_empty() {
        return HmrUpdate {
            kind: HmrKind::None,
            paths: Vec::new(),
            routes: affected_routes.iter().cloned().collect(),
        };
    }
    let nodes = changed
        .iter()
        .filter_map(|path| {
            graph
                .artifacts
                .iter()
                .find(|artifact| artifact.path == *path)
        })
        .collect::<Vec<_>>();
    let kind = if nodes.len() != changed.len() {
        HmrKind::Reload
    } else if nodes.iter().all(|artifact| artifact.path.ends_with(".css")) {
        HmrKind::Css
    } else if nodes.iter().all(|artifact| artifact.kind == "route") {
        HmrKind::Content
    } else if nodes.iter().all(|artifact| {
        [".js", ".mjs", ".wasm"]
            .iter()
            .any(|extension| artifact.path.ends_with(extension))
    }) {
        HmrKind::Adapter
    } else {
        HmrKind::Reload
    };
    HmrUpdate {
        kind,
        paths: changed.iter().map(|path| format!("/{path}")).collect(),
        routes: affected_routes.iter().cloned().collect(),
    }
}

pub(crate) fn write_rebuild_record(root: &Path, record: &RebuildRecord) -> Result<(), String> {
    validate_rebuild_record(record)?;
    let mut bytes = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_REBUILD_RECORD_BYTES {
        return Err(format!(
            "rebuild record exceeds {MAX_REBUILD_RECORD_BYTES} bytes"
        ));
    }
    let path = root.join(REBUILD_RECORD_PATH);
    let parent = path
        .parent()
        .ok_or_else(|| "rebuild record has no parent directory".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let temporary = parent.join(format!("last-rebuild-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    let result = (|| {
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("{}: {error}", temporary.display()))?;
        drop(file);
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "refusing to replace non-regular rebuild record {}",
                    path.display()
                ));
            }
            fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        fs::rename(&temporary, &path)
            .map_err(|error| format!("{} -> {}: {error}", temporary.display(), path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn read_rebuild_record(root: &Path) -> Result<RebuildRecord, String> {
    let path = root.join(REBUILD_RECORD_PATH);
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.len() > MAX_REBUILD_RECORD_BYTES as u64 {
        return Err(format!(
            "rebuild record exceeds {MAX_REBUILD_RECORD_BYTES} bytes"
        ));
    }
    let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let record: RebuildRecord =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    validate_rebuild_record(&record)?;
    Ok(record)
}

fn validate_rebuild_record(record: &RebuildRecord) -> Result<(), String> {
    if record.record_version != REBUILD_RECORD_VERSION || record.receipt_after.len() != 64 {
        return Err("unsupported or invalid rebuild record".to_owned());
    }
    for items in [
        &record.changed_sources,
        &record.affected_routes,
        &record.affected_artifacts,
        &record.changed_artifacts,
        &record.hmr.paths,
        &record.hmr.routes,
    ] {
        if items.len() > MAX_REBUILD_ITEMS
            || items.windows(2).any(|pair| pair[0] >= pair[1])
            || items.iter().any(|item| item.len() > 4096)
        {
            return Err("rebuild record contains an invalid path set".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn explain_artifact(graph: &BuildGraph, query: &str) -> Result<String, String> {
    let query = query.trim();
    let artifact = if query.starts_with('/') {
        graph
            .routes
            .iter()
            .find(|route| route.route == query)
            .and_then(|route| route.artifacts.first())
            .and_then(|path| {
                graph
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.path == *path)
            })
            .or_else(|| {
                let path = query.trim_start_matches('/');
                graph
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.path == path)
            })
    } else {
        graph
            .artifacts
            .iter()
            .find(|artifact| artifact.path == query)
    }
    .ok_or_else(|| {
        format!("artifact or route {query:?} is not present in the current build graph")
    })?;

    let causal = match &artifact.sources {
        SourceDependencies::AllSources => {
            format!(
                "all {} captured project sources (conservative edge)",
                graph.sources.len()
            )
        }
        SourceDependencies::Explicit { paths } => paths.join(", "),
    };
    let route = artifact
        .route
        .as_deref()
        .map(|route| format!(" via route {route}"))
        .unwrap_or_default();
    Ok(format!(
        "{}{} <- {}\nproducer: {}\nsha256: {}",
        artifact.path, route, causal, artifact.producer, artifact.sha256
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_artifact::{
        ArtifactReceipt, BuildContext, GraphArtifact, GraphRoute, GraphSource, OutputSet,
        Ownership, ReplacementPolicy,
    };

    fn graph(css: &[u8], html: &[u8]) -> VerifiedGraph {
        let source = GraphSource {
            path: "src/main.rs".to_owned(),
            sha256: pliego_artifact::sha256_bytes(b"source"),
        };
        let dependencies = SourceDependencies::Explicit {
            paths: vec![source.path.clone()],
        };
        let graph = BuildGraph {
            graph_version: pliego_artifact::BUILD_GRAPH_VERSION.to_owned(),
            project_id: "dev-test".to_owned(),
            source_set_sha256: "1".repeat(64),
            sources: vec![source],
            routes: vec![GraphRoute {
                route: "/".to_owned(),
                sources: dependencies.clone(),
                artifacts: vec!["index.html".to_owned()],
            }],
            artifacts: vec![
                GraphArtifact {
                    path: "assets/site.css".to_owned(),
                    kind: "asset".to_owned(),
                    producer: "assets/site.css".to_owned(),
                    route: None,
                    sources: dependencies.clone(),
                    sha256: pliego_artifact::sha256_bytes(css),
                },
                GraphArtifact {
                    path: "index.html".to_owned(),
                    kind: "route".to_owned(),
                    producer: "/".to_owned(),
                    route: Some("/".to_owned()),
                    sources: dependencies,
                    sha256: pliego_artifact::sha256_bytes(html),
                },
            ],
        };
        let context = BuildContext {
            ownership: Ownership {
                project_id: "dev-test".to_owned(),
                site_package: "dev-test".to_owned(),
            },
            framework: pliego_artifact::FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test".to_owned(),
            },
            toolchain: Vec::new(),
            configuration: Vec::new(),
            sources: vec![pliego_artifact::EvidenceFile {
                path: "src/main.rs".to_owned(),
                bytes: 6,
                sha256: pliego_artifact::sha256_bytes(b"source"),
            }],
            materials: Vec::new(),
            source_set_sha256: "1".repeat(64),
            excluded_paths: Vec::new(),
        };
        VerifiedGraph {
            graph,
            report: BuildReport {
                report_version: "test".to_owned(),
                receipt_sha256: pliego_artifact::sha256_bytes(html),
                receipt: ArtifactReceipt {
                    receipt_version: "test".to_owned(),
                    namespace_version: "test".to_owned(),
                    context,
                    replacement_policy: ReplacementPolicy {
                        required_previous_project_id: "dev-test".to_owned(),
                    },
                    previous_ownership: None,
                    outputs: OutputSet {
                        files: Vec::new(),
                        file_count: 0,
                        total_bytes: 0,
                        sha256: pliego_artifact::sha256_bytes(css),
                    },
                },
            },
        }
    }

    #[test]
    fn rebuild_explanation_selects_precise_hmr_kind() {
        let before = graph(b"a", b"home");
        let after = graph(b"b", b"home");
        let record = explain_rebuild(
            7,
            BTreeSet::from(["src/main.rs".to_owned()]),
            Some(&before),
            &after,
        );
        assert_eq!(record.hmr.kind, HmrKind::Css);
        assert_eq!(record.changed_artifacts, ["assets/site.css"]);
        assert_eq!(record.affected_routes, ["/"]);
    }

    #[test]
    fn unknown_build_input_forces_conservative_invalidation() {
        let before = graph(b"a", b"home");
        let after = graph(b"a", b"changed");
        let record = explain_rebuild(
            8,
            BTreeSet::from(["Cargo.toml".to_owned()]),
            Some(&before),
            &after,
        );
        assert_eq!(record.hmr.kind, HmrKind::Content);
        assert_eq!(record.affected_routes, ["/"]);
        assert_eq!(record.affected_artifacts.len(), 2);
    }
}
