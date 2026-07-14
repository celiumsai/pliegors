use pliego_content::{
    CollectionSpec, ContentFormat, ContentLimits, DEFAULT_MAX_DEPTH, DEFAULT_MAX_ENTRIES,
    DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_TOTAL_BYTES, DiagnosticCode, DiagnosticFormat, Frontmatter,
    LoadOptions, MarkdownPolicy,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct Metadata {
    title: String,
    order: u32,
    #[serde(default)]
    tags: Vec<String>,
}

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pliego-content-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create temporary fixture root");
        Self { root }
    }

    fn write(&self, relative: &str, content: impl AsRef<[u8]>) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, content).expect("write fixture");
    }

    fn remove(&self, relative: &str) {
        fs::remove_file(self.root.join(relative)).expect("remove fixture");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn ids<T>(collection: &pliego_content::Collection<T>) -> Vec<&str> {
    collection.iter().map(|entry| entry.id().as_str()).collect()
}

fn options_with(limits: ContentLimits) -> LoadOptions {
    LoadOptions::new().limits(limits)
}

#[test]
fn secure_limits_are_the_backward_compatible_default() {
    let defaults = ContentLimits::default();
    assert_eq!(defaults.selected_max_depth(), DEFAULT_MAX_DEPTH);
    assert_eq!(defaults.selected_max_entries(), DEFAULT_MAX_ENTRIES);
    assert_eq!(defaults.selected_max_file_bytes(), DEFAULT_MAX_FILE_BYTES);
    assert_eq!(defaults.selected_max_total_bytes(), DEFAULT_MAX_TOTAL_BYTES);
    assert_eq!(LoadOptions::default(), LoadOptions::new());
    assert_eq!(LoadOptions::new().selected_limits(), defaults);
}

#[test]
fn rejects_excessive_depth_before_reaching_an_invalid_source() {
    let tree = TempTree::new("depth-limit");
    tree.write("one/two/invalid.json", "not JSON");
    let limits = ContentLimits::new().max_depth(1);

    let error = CollectionSpec::new(&tree.root)
        .with_options(options_with(limits))
        .load::<Metadata>()
        .expect_err("depth quota");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::DepthLimitExceeded
    );
    assert_eq!(error.diagnostics()[0].relative_path(), Some("one/two"));
    assert_eq!(
        error.diagnostics()[0].message(),
        "collection depth 2 exceeds configured maximum 1"
    );
}

#[test]
fn caps_discovery_entries_before_unbounded_enumeration_or_parsing() {
    let tree = TempTree::new("entry-limit");
    tree.write("a.json", "invalid");
    tree.write("b.json", "invalid");
    tree.write("ignored.txt", "also consumes traversal work");
    let limits = ContentLimits::new().max_entries(2);

    let error = CollectionSpec::new(&tree.root)
        .with_options(options_with(limits))
        .load::<Metadata>()
        .expect_err("entry quota");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::EntryLimitExceeded
    );
    assert_eq!(error.diagnostics()[0].path(), tree.root);
    assert_eq!(
        error.diagnostics()[0].message(),
        "collection traversal exceeds configured maximum of 2 filesystem entries"
    );
}

#[test]
fn rejects_oversized_file_from_metadata_before_allocating_or_parsing() {
    let tree = TempTree::new("file-byte-limit");
    tree.write("large.json", vec![b'x'; 65]);
    let limits = ContentLimits::new().max_file_bytes(64);

    let error = CollectionSpec::new(&tree.root)
        .with_options(options_with(limits))
        .load::<Metadata>()
        .expect_err("per-file quota");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::FileByteLimitExceeded
    );
    assert_eq!(error.diagnostics()[0].relative_path(), Some("large.json"));
    assert_eq!(
        error.diagnostics()[0].message(),
        "content source is 65 bytes; configured per-file maximum is 64 bytes"
    );
}

#[test]
fn rejects_aggregate_bytes_deterministically_before_parsing_any_source() {
    let tree = TempTree::new("total-byte-limit");
    tree.write("a.json", vec![b'a'; 20]);
    tree.write("b.json", vec![b'b'; 20]);
    let limits = ContentLimits::new().max_file_bytes(20).max_total_bytes(39);

    let error = CollectionSpec::new(&tree.root)
        .with_options(options_with(limits))
        .load::<Metadata>()
        .expect_err("aggregate quota");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::TotalByteLimitExceeded
    );
    assert_eq!(error.diagnostics()[0].relative_path(), Some("b.json"));
    assert_eq!(
        error.diagnostics()[0].message(),
        "content sources exceed configured aggregate maximum of 39 bytes"
    );
}

#[test]
fn loads_every_format_recursively_as_typed_deterministic_entries() {
    let tree = TempTree::new("formats");
    tree.write("z.json", br#"{"title":"JSON","order":4,"tags":["one"]}"#);
    tree.write("a.toml", "title = \"TOML\"\norder = 1\ntags = [\"two\"]\n");
    tree.write(
        "nested/y.md",
        "---\ntitle: YAML\norder: 3\ntags: [three]\n---\n# Hello\n\nA **bold** move.\n",
    );
    tree.write(
        "nested/b.markdown",
        "+++\ntitle = \"Markdown TOML\"\norder = 2\ntags = [\"four\"]\n+++\nBody.\n",
    );
    tree.write("README.txt", "ignored");

    let collection = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect("typed collection");
    assert_eq!(ids(&collection), ["a", "nested/b", "nested/y", "z"]);
    assert_eq!(collection.get("a").unwrap().format(), ContentFormat::Toml);
    assert_eq!(collection.get("z").unwrap().format(), ContentFormat::Json);
    assert_eq!(
        collection.get("nested/b").unwrap().frontmatter(),
        Some(Frontmatter::Toml)
    );
    let yaml = collection.get("nested/y").unwrap();
    assert_eq!(yaml.frontmatter(), Some(Frontmatter::Yaml));
    assert_eq!(yaml.data().title, "YAML");
    assert_eq!(yaml.relative_path(), "nested/y.md");
    assert_eq!(
        yaml.markdown()
            .unwrap()
            .render_html(MarkdownPolicy::Safe)
            .unwrap(),
        "<h1>Hello</h1>\n<p>A <strong>bold</strong> move.</p>\n"
    );
    assert!(collection.get("missing").is_none());
}

#[test]
fn aggregates_parse_and_frontmatter_diagnostics_in_path_order() {
    let tree = TempTree::new("diagnostics");
    tree.write("a.json", "{");
    tree.write("b.toml", "title = [");
    tree.write("c.md", "plain Markdown");
    tree.write("d.md", "---\ntitle: Open\n");
    tree.write("e.md", "---\ntitle: [\n---\nBody\n");

    let error = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect_err("five invalid sources");
    let paths: Vec<_> = error
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.relative_path().unwrap())
        .collect();
    assert_eq!(paths, ["a.json", "b.toml", "c.md", "d.md", "e.md"]);
    assert_eq!(
        error.diagnostics()[0].format(),
        Some(DiagnosticFormat::Json)
    );
    assert_eq!(
        error.diagnostics()[1].format(),
        Some(DiagnosticFormat::Toml)
    );
    assert_eq!(
        error.diagnostics()[2].code(),
        DiagnosticCode::MissingFrontmatter
    );
    assert_eq!(
        error.diagnostics()[3].code(),
        DiagnosticCode::UnterminatedFrontmatter
    );
    assert_eq!(
        error.diagnostics()[4].format(),
        Some(DiagnosticFormat::YamlFrontmatter)
    );
    assert!(error.diagnostics().iter().all(|item| item.span().is_some()));
    assert_eq!(error.snapshot(), error.snapshot());
}

#[test]
fn rejects_exact_and_case_insensitive_id_collisions_before_parsing() {
    let tree = TempTree::new("collisions");
    tree.write("Post.md", "not valid");
    tree.write("post.toml", "also invalid");
    tree.write("same.json", "invalid");
    tree.write("same.markdown", "invalid");

    let error = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect_err("colliding IDs");
    assert_eq!(error.diagnostics().len(), 2);
    assert!(
        error
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code() == DiagnosticCode::IdCollision)
    );
    assert!(
        error
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.related_path().is_some())
    );
}

#[test]
fn safe_markdown_rejects_raw_html_and_untrusted_uri_schemes() {
    let tree = TempTree::new("markdown-policy");
    tree.write(
        "html.md",
        "+++\ntitle = \"HTML\"\norder = 1\n+++\n<script>alert(1)</script>\n",
    );
    tree.write(
        "uri.md",
        "+++\ntitle = \"URI\"\norder = 2\n+++\n[open](javascript:alert(1))\n",
    );

    let error = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect_err("safe policy");
    assert_eq!(error.diagnostics().len(), 2);
    assert!(
        error
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code() == DiagnosticCode::UnsafeMarkdown)
    );

    let trusted = CollectionSpec::new(&tree.root)
        .with_options(LoadOptions::new().markdown_policy(MarkdownPolicy::Trusted))
        .load::<Metadata>()
        .expect("explicit trusted policy");
    let html = trusted.get("html").unwrap().markdown().unwrap();
    assert!(html.render_html(MarkdownPolicy::Safe).is_err());
    assert!(
        html.render_html(MarkdownPolicy::Trusted)
            .unwrap()
            .contains("<script>alert(1)</script>")
    );
}

#[test]
fn snapshots_report_sorted_added_changed_and_removed_ids() {
    let tree = TempTree::new("snapshot");
    tree.write("a.json", br#"{"title":"A","order":1}"#);
    tree.write("b.json", br#"{"title":"B","order":2}"#);
    let before = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .unwrap()
        .snapshot();

    tree.write("a.json", br#"{"title":"A changed","order":1}"#);
    tree.remove("b.json");
    tree.write("c.json", br#"{"title":"C","order":3}"#);
    let after = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .unwrap()
        .snapshot();
    let diff = before.diff(&after);

    assert_eq!(
        diff.added()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["c"]
    );
    assert_eq!(
        diff.changed()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(
        diff.removed()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["b"]
    );
    assert!(!diff.is_empty());
    assert!(after.diff(&after).is_empty());
}

#[test]
fn fingerprints_normalize_crlf_but_include_markdown_policy() {
    let tree = TempTree::new("fingerprint-contract");
    tree.write(
        "post.md",
        "+++\r\ntitle = \"Post\"\r\norder = 1\r\n+++\r\nBody.\r\n",
    );
    let crlf = CollectionSpec::new(&tree.root).load::<Metadata>().unwrap();
    let crlf_fingerprint = crlf.get("post").unwrap().fingerprint().clone();
    assert_eq!(
        crlf.get("post").unwrap().markdown().unwrap().source(),
        "Body.\n"
    );

    tree.write("post.md", "+++\ntitle = \"Post\"\norder = 1\n+++\nBody.\n");
    let lf = CollectionSpec::new(&tree.root).load::<Metadata>().unwrap();
    assert_eq!(lf.get("post").unwrap().fingerprint(), &crlf_fingerprint);

    let trusted = CollectionSpec::new(&tree.root)
        .with_options(LoadOptions::new().markdown_policy(MarkdownPolicy::Trusted))
        .load::<Metadata>()
        .unwrap();
    assert_ne!(
        trusted.get("post").unwrap().fingerprint(),
        &crlf_fingerprint
    );
}

#[test]
fn rejects_bom_non_utf8_and_non_ascii_portable_paths() {
    let tree = TempTree::new("encoding-paths");
    tree.write(
        "bom.md",
        "\u{feff}+++\ntitle = \"BOM\"\norder = 1\n+++\nBody\n",
    );
    tree.write("invalid.json", [0xff, 0xfe, 0xfd]);
    tree.write(
        "café.md",
        "+++\ntitle = \"Unicode path\"\norder = 2\n+++\nBody\n",
    );

    let error = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect_err("portable diagnostics");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::InvalidPath);

    tree.remove("café.md");
    let error = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect_err("encoding diagnostics");
    assert_eq!(error.diagnostics().len(), 2);
    assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::BomRejected);
    assert_eq!(error.diagnostics()[1].code(), DiagnosticCode::InvalidUtf8);
}

#[test]
fn missing_or_non_directory_roots_report_the_requested_path() {
    let tree = TempTree::new("roots");
    let missing = tree.root.join("missing");
    let error = CollectionSpec::new(&missing)
        .load::<Metadata>()
        .expect_err("missing root");
    assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::Io);
    assert_eq!(error.diagnostics()[0].path(), missing);

    tree.write("file", "not a directory");
    let file = tree.root.join("file");
    let error = CollectionSpec::new(&file)
        .load::<Metadata>()
        .expect_err("file root");
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::RootNotDirectory
    );
}

#[cfg(any(unix, windows))]
#[test]
fn rejects_directory_links_without_following_them() {
    let tree = TempTree::new("symlink");
    tree.write(
        "target/entry.md",
        "+++\ntitle = \"Target\"\norder = 1\n+++\nBody\n",
    );
    let target = tree.root.join("target");
    let link = tree.root.join("linked");
    create_directory_link(&target, &link).expect("create fixture directory link");

    let error = CollectionSpec::new(&tree.root)
        .load::<Metadata>()
        .expect_err("symlink rejected");
    assert_eq!(error.diagnostics().len(), 1);
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::SymlinkRejected
    );
    assert_eq!(error.diagnostics()[0].relative_path(), Some("linked"));
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(1314) => {
            let status = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(format!(
                    "mklink /J exited with {status}"
                )))
            }
        }
        Err(error) => Err(error),
    }
}

#[test]
fn reference_yaml_corpus_has_stable_order_and_digest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/content/reference");
    let expected = [
        ("houses", vec!["field-room", "north-studio"]),
        ("journal", vec!["content-as-data", "deterministic-builds"]),
        ("rituals", vec!["clear-table", "first-light"]),
    ];

    let mut contract = String::new();
    for (name, expected_ids) in expected {
        let collection = CollectionSpec::new(root.join(name))
            .load::<serde_json::Value>()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(ids(&collection), expected_ids);
        assert!(
            collection
                .iter()
                .all(|entry| entry.frontmatter() == Some(Frontmatter::Yaml))
        );
        for (id, fingerprint) in collection.snapshot().iter() {
            contract.push_str(name);
            contract.push('/');
            contract.push_str(id.as_str());
            contract.push(':');
            contract.push_str(fingerprint.as_str());
            contract.push('\n');
        }
    }
    let digest = format!("{:x}", Sha256::digest(contract.as_bytes()));
    assert_eq!(
        digest,
        "95e1ea69cc8267bfeab7fc77b0c8d5bcdd420cba4a7f14f2e7cb1f55f3d89e52"
    );
}
