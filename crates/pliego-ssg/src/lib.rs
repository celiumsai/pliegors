// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Deterministic full-document and static-route generation for PliegoRS.

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, OpenOptions};
use pliego_dom::{View, render_html};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const BUILD_REPORT_VERSION: &str = "1.0.0";
const MAX_BUILD_LEDGER_BYTES: u64 = 8 * 1024 * 1024;
static BUILD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum BuildError {
    InvalidPath(String),
    DuplicateRoute(String),
    DuplicateAsset(String),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json(serde_json::Error),
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(path) => write!(formatter, "invalid Pliego path: {path}"),
            Self::DuplicateRoute(route) => write!(formatter, "duplicate route: {route}"),
            Self::DuplicateAsset(path) => write!(formatter, "duplicate asset: {path}"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json(source) => write!(formatter, "cannot serialize build report: {source}"),
        }
    }
}

impl std::error::Error for BuildError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Head {
    title: String,
    description: Option<String>,
    canonical: Option<String>,
    icons: Vec<String>,
    manifest: Option<String>,
    apple_touch_icon: Option<String>,
    alternates: Vec<(String, String)>,
    stylesheets: Vec<String>,
    inline_scripts: Vec<String>,
    module_scripts: Vec<String>,
    redirect: Option<String>,
    meta: BTreeMap<String, String>,
    property_meta: BTreeMap<String, String>,
    json_ld: Vec<serde_json::Value>,
}

impl Head {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            canonical: None,
            icons: Vec::new(),
            manifest: None,
            apple_touch_icon: None,
            alternates: Vec::new(),
            stylesheets: Vec::new(),
            inline_scripts: Vec::new(),
            module_scripts: Vec::new(),
            redirect: None,
            meta: BTreeMap::new(),
            property_meta: BTreeMap::new(),
            json_ld: Vec::new(),
        }
    }

    #[must_use]
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }

    #[must_use]
    pub fn canonical(mut self, value: impl Into<String>) -> Self {
        self.canonical = Some(value.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, href: impl Into<String>) -> Self {
        self.icons.push(href.into());
        self
    }

    #[must_use]
    pub fn manifest(mut self, href: impl Into<String>) -> Self {
        self.manifest = Some(href.into());
        self
    }

    #[must_use]
    pub fn apple_touch_icon(mut self, href: impl Into<String>) -> Self {
        self.apple_touch_icon = Some(href.into());
        self
    }

    #[must_use]
    pub fn alternate(mut self, language: impl Into<String>, href: impl Into<String>) -> Self {
        self.alternates.push((language.into(), href.into()));
        self
    }

    #[must_use]
    pub fn stylesheet(mut self, href: impl Into<String>) -> Self {
        self.stylesheets.push(href.into());
        self
    }

    /// Add trusted, framework-authored JavaScript that must execute during head parsing.
    /// Closing script tags are neutralized when the document is rendered.
    #[must_use]
    pub fn inline_script(mut self, source: impl Into<String>) -> Self {
        self.inline_scripts.push(source.into());
        self
    }

    #[must_use]
    pub fn module_script(mut self, src: impl Into<String>) -> Self {
        self.module_scripts.push(src.into());
        self
    }

    #[must_use]
    pub fn redirect(mut self, location: impl Into<String>) -> Self {
        self.redirect = Some(location.into());
        self
    }

    #[must_use]
    pub fn meta(mut self, name: impl Into<String>, content: impl Into<String>) -> Self {
        self.meta.insert(name.into(), content.into());
        self
    }

    #[must_use]
    pub fn property_meta(
        mut self,
        property: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.property_meta.insert(property.into(), content.into());
        self
    }

    #[must_use]
    pub fn json_ld(mut self, value: serde_json::Value) -> Self {
        self.json_ld.push(value);
        self
    }
}

#[derive(Clone)]
pub struct Page {
    pub route: String,
    pub language: String,
    pub head: Head,
    pub body: View,
}

impl Page {
    pub fn new(route: impl Into<String>, head: Head, body: View) -> Self {
        Self {
            route: route.into(),
            language: "en".to_owned(),
            head,
            body,
        }
    }

    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub fn render(&self) -> Result<String, BuildError> {
        validate_route(&self.route)?;
        validate_head_urls(&self.head)?;
        let mut output = String::from("<!doctype html><html lang=\"");
        output.push_str(&escape_attribute(&self.language));
        output.push_str("\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
        output.push_str("<title>");
        output.push_str(&escape_text(&self.head.title));
        output.push_str("</title>");
        if let Some(description) = &self.head.description {
            push_meta(&mut output, "description", description);
        }
        for (name, content) in &self.head.meta {
            push_meta(&mut output, name, content);
        }
        for (property, content) in &self.head.property_meta {
            push_property_meta(&mut output, property, content);
        }
        if let Some(canonical) = &self.head.canonical {
            output.push_str("<link rel=\"canonical\" href=\"");
            output.push_str(&escape_attribute(canonical));
            output.push_str("\">");
        }
        for icon in &self.head.icons {
            output.push_str("<link rel=\"icon\" href=\"");
            output.push_str(&escape_attribute(icon));
            output.push_str("\">");
        }
        if let Some(manifest) = &self.head.manifest {
            output.push_str("<link rel=\"manifest\" href=\"");
            output.push_str(&escape_attribute(manifest));
            output.push_str("\">");
        }
        if let Some(icon) = &self.head.apple_touch_icon {
            output.push_str("<link rel=\"apple-touch-icon\" href=\"");
            output.push_str(&escape_attribute(icon));
            output.push_str("\">");
        }
        for (language, href) in &self.head.alternates {
            output.push_str("<link rel=\"alternate\" hreflang=\"");
            output.push_str(&escape_attribute(language));
            output.push_str("\" href=\"");
            output.push_str(&escape_attribute(href));
            output.push_str("\">");
        }
        for script in &self.head.inline_scripts {
            output.push_str("<script>");
            output.push_str(&escape_inline_script(script));
            output.push_str("</script>");
        }
        for stylesheet in &self.head.stylesheets {
            output.push_str("<link rel=\"stylesheet\" href=\"");
            output.push_str(&escape_attribute(stylesheet));
            output.push_str("\">");
        }
        for script in &self.head.module_scripts {
            output.push_str("<script type=\"module\" src=\"");
            output.push_str(&escape_attribute(script));
            output.push_str("\"></script>");
        }
        if let Some(location) = &self.head.redirect {
            output.push_str("<meta http-equiv=\"refresh\" content=\"0;url=");
            output.push_str(&escape_attribute(location));
            output.push_str("\">");
        }
        for value in &self.head.json_ld {
            output.push_str("<script type=\"application/ld+json\">");
            output.push_str(
                &serde_json::to_string(value)
                    .map_err(BuildError::Json)?
                    .replace('<', "\\u003c"),
            );
            output.push_str("</script>");
        }
        output.push_str("</head><body>");
        output.push_str(&render_html(&self.body));
        output.push_str("</body></html>\n");
        Ok(output)
    }
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl Asset {
    pub fn new(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
        }
    }
}

#[derive(Default)]
pub struct Site {
    pages: Vec<Page>,
    assets: Vec<Asset>,
}

impl Site {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn page(mut self, page: Page) -> Self {
        self.pages.push(page);
        self
    }

    #[must_use]
    pub fn asset(mut self, asset: Asset) -> Self {
        self.assets.push(asset);
        self
    }

    pub fn build(&self, output_dir: impl AsRef<Path>) -> Result<BuildReport, BuildError> {
        let output_dir = output_dir.as_ref();
        validate_output_target(output_dir)?;
        let mut files = BTreeMap::<String, Vec<u8>>::new();
        let mut routes = BTreeSet::new();

        for page in &self.pages {
            validate_route(&page.route)?;
            if !routes.insert(page.route.clone()) {
                return Err(BuildError::DuplicateRoute(page.route.clone()));
            }
            files.insert(route_output_path(&page.route), page.render()?.into_bytes());
        }
        for asset in &self.assets {
            validate_relative_path(&asset.path)?;
            if asset.path.eq_ignore_ascii_case("pliego.build.json") {
                return Err(BuildError::InvalidPath(
                    "pliego.build.json is reserved for the framework build ledger".to_owned(),
                ));
            }
            if files
                .insert(asset.path.clone(), asset.bytes.clone())
                .is_some()
            {
                return Err(BuildError::DuplicateAsset(asset.path.clone()));
            }
        }

        let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| BuildError::Io {
            path: parent.to_owned(),
            source,
        })?;
        let output_name = output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("site");
        let stage = create_stage_directory(parent, output_name)?;
        let mut stage_cleanup = DirectoryCleanup::new(stage.clone());
        let mut emitted = Vec::new();
        for (relative, bytes) in files {
            let destination = stage.join(&relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| BuildError::Io {
                    path: parent.to_owned(),
                    source,
                })?;
            }
            fs::write(&destination, &bytes).map_err(|source| BuildError::Io {
                path: destination,
                source,
            })?;
            emitted.push(EmittedFile {
                path: relative,
                bytes: bytes.len() as u64,
                sha256: sha256(&bytes),
            });
        }
        emitted.sort_by(|left, right| left.path.cmp(&right.path));
        let report = BuildReport {
            report_version: BUILD_REPORT_VERSION,
            files: emitted,
        };
        let mut report_bytes = serde_json::to_vec_pretty(&report).map_err(BuildError::Json)?;
        report_bytes.push(b'\n');
        let report_path = stage.join("pliego.build.json");
        fs::write(&report_path, report_bytes).map_err(|source| BuildError::Io {
            path: report_path,
            source,
        })?;
        if output_dir.exists() {
            validate_replaceable_output(output_dir)?;
            let backup = allocate_private_path(parent, output_name, "backup")?;
            fs::rename(output_dir, &backup).map_err(|source| BuildError::Io {
                path: output_dir.to_owned(),
                source,
            })?;
            let mut backup_cleanup = DirectoryCleanup::new(backup.clone());
            if let Err(source) = fs::rename(&stage, output_dir) {
                let rollback = fs::rename(&backup, output_dir);
                backup_cleanup.disarm();
                if let Err(rollback_source) = rollback {
                    return Err(BuildError::Io {
                        path: backup,
                        source: rollback_source,
                    });
                }
                return Err(BuildError::Io {
                    path: output_dir.to_owned(),
                    source,
                });
            }
            stage_cleanup.disarm();
            fs::remove_dir_all(&backup).map_err(|source| BuildError::Io {
                path: backup.clone(),
                source,
            })?;
            backup_cleanup.disarm();
        } else {
            fs::rename(&stage, output_dir).map_err(|source| BuildError::Io {
                path: output_dir.to_owned(),
                source,
            })?;
            stage_cleanup.disarm();
        }
        Ok(report)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmittedFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub report_version: &'static str,
    pub files: Vec<EmittedFile>,
}

fn validate_output_target(output: &Path) -> Result<(), BuildError> {
    if output.as_os_str().is_empty() || output.file_name().and_then(|name| name.to_str()).is_none()
    {
        return Err(BuildError::InvalidPath(output.display().to_string()));
    }
    if output.exists() {
        validate_replaceable_output(output)?;
    }
    Ok(())
}

fn validate_replaceable_output(output: &Path) -> Result<(), BuildError> {
    let metadata = fs::symlink_metadata(output).map_err(|source| BuildError::Io {
        path: output.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BuildError::InvalidPath(format!(
            "refusing to replace non-directory or linked output {}",
            output.display()
        )));
    }
    let canonical = output.canonicalize().map_err(|source| BuildError::Io {
        path: output.to_owned(),
        source,
    })?;
    let current = std::env::current_dir()
        .and_then(|path| path.canonicalize())
        .map_err(|source| BuildError::Io {
            path: PathBuf::from("."),
            source,
        })?;
    if current.starts_with(&canonical) {
        return Err(BuildError::InvalidPath(format!(
            "refusing to replace the current directory or one of its ancestors: {}",
            output.display()
        )));
    }
    let bytes = read_build_marker(&canonical, output)?;
    let report: serde_json::Value = serde_json::from_slice(&bytes).map_err(BuildError::Json)?;
    if report["reportVersion"].as_str() != Some(BUILD_REPORT_VERSION) || !report["files"].is_array()
    {
        return Err(BuildError::InvalidPath(format!(
            "unsupported Pliego build marker in {}",
            output.display()
        )));
    }
    Ok(())
}

fn read_build_marker(
    canonical_output: &Path,
    display_output: &Path,
) -> Result<Vec<u8>, BuildError> {
    let marker = display_output.join("pliego.build.json");
    let directory =
        Dir::open_ambient_dir(canonical_output, ambient_authority()).map_err(|source| {
            BuildError::Io {
                path: display_output.to_owned(),
                source,
            }
        })?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with("pliego.build.json", &options)
        .map_err(|_| {
            BuildError::InvalidPath(format!(
                "refusing to replace unowned output without pliego.build.json: {}",
                display_output.display()
            ))
        })?;
    let metadata = file.metadata().map_err(|source| BuildError::Io {
        path: marker.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(BuildError::InvalidPath(format!(
            "invalid Pliego build marker in {}",
            display_output.display()
        )));
    }
    if metadata.len() > MAX_BUILD_LEDGER_BYTES {
        return Err(BuildError::InvalidPath(format!(
            "Pliego build marker exceeds {MAX_BUILD_LEDGER_BYTES} bytes in {}",
            display_output.display()
        )));
    }

    let mut bytes = Vec::new();
    file.take(MAX_BUILD_LEDGER_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| BuildError::Io {
            path: marker,
            source,
        })?;
    if bytes.len() as u64 > MAX_BUILD_LEDGER_BYTES {
        return Err(BuildError::InvalidPath(format!(
            "Pliego build marker grew beyond {MAX_BUILD_LEDGER_BYTES} bytes while reading in {}",
            display_output.display()
        )));
    }
    Ok(bytes)
}

fn create_stage_directory(parent: &Path, output_name: &str) -> Result<PathBuf, BuildError> {
    for _ in 0..64 {
        let stage = parent.join(format!(
            ".{output_name}.pliego-stage-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(BuildError::Io {
                    path: stage,
                    source,
                });
            }
        }
    }
    Err(BuildError::InvalidPath(format!(
        "cannot allocate a private staging directory below {}",
        parent.display()
    )))
}

struct DirectoryCleanup {
    path: PathBuf,
    armed: bool,
}

impl DirectoryCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DirectoryCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn allocate_private_path(
    parent: &Path,
    output_name: &str,
    purpose: &str,
) -> Result<PathBuf, BuildError> {
    for _ in 0..64 {
        let path = parent.join(format!(
            ".{output_name}.pliego-{purpose}-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::symlink_metadata(&path) {
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(path),
            Ok(_) => continue,
            Err(source) => return Err(BuildError::Io { path, source }),
        }
    }
    Err(BuildError::InvalidPath(format!(
        "cannot allocate a private {purpose} path below {}",
        parent.display()
    )))
}

fn route_output_path(route: &str) -> String {
    if route == "/" {
        "index.html".to_owned()
    } else if route.ends_with(".html") {
        route.trim_start_matches('/').to_owned()
    } else {
        format!("{}/index.html", route.trim_matches('/'))
    }
}

fn validate_route(route: &str) -> Result<(), BuildError> {
    if !route.starts_with('/') || route.contains('?') || route.contains('#') || route.contains("//")
    {
        return Err(BuildError::InvalidPath(route.to_owned()));
    }
    validate_relative_path(route.trim_start_matches('/'))
}

fn validate_relative_path(path: &str) -> Result<(), BuildError> {
    if path.starts_with('/')
        || path.contains('\\')
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(BuildError::InvalidPath(path.to_owned()));
    }
    Ok(())
}

fn validate_head_urls(head: &Head) -> Result<(), BuildError> {
    for (field, value) in head
        .canonical
        .iter()
        .map(|value| ("canonical", value))
        .chain(head.icons.iter().map(|value| ("icon", value)))
        .chain(head.manifest.iter().map(|value| ("manifest", value)))
        .chain(
            head.apple_touch_icon
                .iter()
                .map(|value| ("apple-touch-icon", value)),
        )
        .chain(
            head.alternates
                .iter()
                .map(|(_, value)| ("alternate", value)),
        )
        .chain(head.stylesheets.iter().map(|value| ("stylesheet", value)))
        .chain(
            head.module_scripts
                .iter()
                .map(|value| ("module-script", value)),
        )
        .chain(head.redirect.iter().map(|value| ("redirect", value)))
    {
        if !safe_document_url(value) {
            return Err(BuildError::InvalidPath(format!(
                "unsafe {field} URL: {value}"
            )));
        }
    }
    Ok(())
}

fn safe_document_url(value: &str) -> bool {
    if value.is_empty()
        || value.trim() != value
        || value.contains(['\\', '\n', '\r', '\0'])
        || value.starts_with("//")
    {
        return false;
    }
    if value.starts_with('/') || value.starts_with("https://") || value.starts_with("http://") {
        return true;
    }
    !value.contains(':')
        && !Path::new(value).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn push_meta(output: &mut String, name: &str, content: &str) {
    output.push_str("<meta name=\"");
    output.push_str(&escape_attribute(name));
    output.push_str("\" content=\"");
    output.push_str(&escape_attribute(content));
    output.push_str("\">");
}

fn push_property_meta(output: &mut String, property: &str, content: &str) {
    output.push_str("<meta property=\"");
    output.push_str(&escape_attribute(property));
    output.push_str("\" content=\"");
    output.push_str(&escape_attribute(content));
    output.push_str("\">");
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

fn escape_inline_script(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut offset = 0;
    while offset < value.len() {
        let remaining = &value[offset..];
        if remaining.len() >= 8 && remaining.as_bytes()[..8].eq_ignore_ascii_case(b"</script") {
            output.push_str("<\\/");
            output.push_str(&remaining[2..8]);
            offset += 8;
            continue;
        }
        let character = remaining
            .chars()
            .next()
            .expect("offset remains on a UTF-8 boundary");
        output.push(character);
        offset += character.len_utf8();
    }
    output.replace("<!--", "<\\!--")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_dom::{IntoView, el};

    #[test]
    fn page_renders_a_complete_escaped_document() {
        let page = Page::new(
            "/",
            Head::new("PliegoRS & Reference").description("<quiet>"),
            el("main").child(el("h1").child("Reference")).into_view(),
        );
        let html = page.render().unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>PliegoRS &amp; Reference</title>"));
        assert!(html.contains("content=\"&lt;quiet&gt;\""));
        assert!(html.contains("<body><main><h1>Reference</h1></main></body>"));
    }

    #[test]
    fn module_scripts_are_explicit_and_escaped() {
        let page = Page::new(
            "/interactive",
            Head::new("Island").module_script("/assets/resume.js?x=1&y=2"),
            el("main").into_view(),
        );
        assert!(
            page.render()
                .unwrap()
                .contains(r#"<script type="module" src="/assets/resume.js?x=1&amp;y=2"></script>"#)
        );
    }

    #[test]
    fn active_document_urls_reject_executable_or_ambiguous_schemes() {
        for head in [
            Head::new("Unsafe").module_script("javascript:alert(1)"),
            Head::new("Unsafe").stylesheet("data:text/css,body{}"),
            Head::new("Unsafe").redirect("//untrusted.example/path"),
            Head::new("Unsafe").icon("../outside.svg"),
        ] {
            let page = Page::new("/unsafe", head, el("main").into_view());
            assert!(matches!(page.render(), Err(BuildError::InvalidPath(_))));
        }
        assert!(
            Page::new(
                "/safe",
                Head::new("Safe")
                    .stylesheet("https://fonts.example/style.css?family=Pliego")
                    .module_script("/assets/client.js"),
                el("main").into_view(),
            )
            .render()
            .is_ok()
        );
    }

    #[test]
    fn inline_scripts_run_before_styles_and_cannot_close_their_element() {
        let page = Page::new(
            "/theme",
            Head::new("Theme")
                .inline_script("document.documentElement.dataset.theme='dark';</ScRiPt><p>bad</p>")
                .stylesheet("/theme.css"),
            el("main").into_view(),
        );
        let html = page.render().unwrap();
        assert!(!html.contains("</ScRiPt><p>bad"));
        assert!(html.contains("<\\/ScRiPt><p>bad</p>"));
        assert!(html.find("dataset.theme").unwrap() < html.find("/theme.css").unwrap());
    }

    #[test]
    fn document_identity_links_are_explicit_and_escaped() {
        let page = Page::new(
            "/identity",
            Head::new("Identity")
                .icon("/favicon.svg?rev=pliego&rs=1")
                .manifest("/site.webmanifest")
                .apple_touch_icon("/apple-touch-icon.png")
                .alternate("es", "https://example.com/es/identity"),
            el("main").into_view(),
        );
        let html = page.render().unwrap();
        assert!(html.contains(r#"<link rel="icon" href="/favicon.svg?rev=pliego&amp;rs=1">"#));
        assert!(html.contains(r#"<link rel="manifest" href="/site.webmanifest">"#));
        assert!(html.contains(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#));
        assert!(html.contains(
            r#"<link rel="alternate" hreflang="es" href="https://example.com/es/identity">"#
        ));
    }

    #[test]
    fn open_graph_metadata_uses_property_attributes() {
        let page = Page::new(
            "/sharing",
            Head::new("Sharing").property_meta("og:title", "Pliego & Rust"),
            el("main").into_view(),
        );
        assert!(
            page.render()
                .unwrap()
                .contains(r#"<meta property="og:title" content="Pliego &amp; Rust">"#)
        );
    }

    #[test]
    fn structured_data_is_serialized_without_html_breakout() {
        let page = Page::new(
            "/structured",
            Head::new("Structured").json_ld(serde_json::json!({
                "@type": "WebSite",
                "name": "</script><script>alert(1)</script>"
            })),
            el("main").into_view(),
        );
        let html = page.render().unwrap();
        assert!(html.contains(r#"<script type="application/ld+json">"#));
        assert!(!html.contains("</script><script>alert"));
        assert!(html.contains(r#"\u003c/script>"#));
    }

    #[test]
    fn file_routes_and_redirects_emit_static_host_contracts() {
        let page = Page::new(
            "/404.html",
            Head::new("Not found").redirect("/"),
            el("main").into_view(),
        );
        assert_eq!(route_output_path(&page.route), "404.html");
        assert!(
            page.render()
                .unwrap()
                .contains(r#"<meta http-equiv="refresh" content="0;url=/">"#)
        );
    }

    #[test]
    fn build_is_stable_and_rejects_path_traversal() {
        let root = std::env::temp_dir().join(format!("pliego-ssg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let site = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .asset(Asset::new("assets/site.css", b"body{}".to_vec()));
        let first = site.build(&root).unwrap();
        fs::write(root.join("stale.txt"), b"stale").unwrap();
        let second = site.build(&root).unwrap();
        assert_eq!(first, second);
        assert!(!root.join("stale.txt").exists());
        assert!(matches!(
            Site::new()
                .asset(Asset::new("../secret", vec![]))
                .build(&root),
            Err(BuildError::InvalidPath(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_never_replaces_an_unowned_existing_directory() {
        let root = std::env::temp_dir().join(format!(
            "pliego-ssg-unowned-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let sentinel = root.join("source.rs");
        fs::write(&sentinel, b"must survive").unwrap();
        let result = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build(&root);
        assert!(matches!(result, Err(BuildError::InvalidPath(_))));
        assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_never_reads_or_replaces_an_oversized_ledger() {
        let root = std::env::temp_dir().join(format!(
            "pliego-ssg-large-ledger-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let marker = root.join("pliego.build.json");
        let file = fs::File::create(&marker).unwrap();
        file.set_len(MAX_BUILD_LEDGER_BYTES + 1).unwrap();
        let sentinel = root.join("sentinel.txt");
        fs::write(&sentinel, b"must survive").unwrap();

        let result = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build(&root);
        assert!(matches!(result, Err(BuildError::InvalidPath(_))));
        assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
        assert_eq!(
            fs::metadata(&marker).unwrap().len(),
            MAX_BUILD_LEDGER_BYTES + 1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn build_reserves_its_ledger_and_rolls_back_failed_staging() {
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-rollback-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let output = parent.join("site");
        fs::create_dir_all(&parent).unwrap();
        Site::new()
            .page(Page::new("/", Head::new("Stable"), el("main").into_view()))
            .build(&output)
            .unwrap();
        let original = fs::read(output.join("index.html")).unwrap();

        let reserved = Site::new()
            .asset(Asset::new("Pliego.Build.Json", b"forged".to_vec()))
            .build(&output);
        assert!(matches!(reserved, Err(BuildError::InvalidPath(_))));

        let failed = Site::new()
            .asset(Asset::new("assets/collision", b"file".to_vec()))
            .asset(Asset::new("assets/collision/nested", b"nested".to_vec()))
            .build(&output);
        assert!(matches!(failed, Err(BuildError::Io { .. })));
        assert_eq!(fs::read(output.join("index.html")).unwrap(), original);
        let leftovers = fs::read_dir(&parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.contains("pliego-stage") || name.contains("pliego-backup"))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "leftover build directories: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(parent);
    }
}
