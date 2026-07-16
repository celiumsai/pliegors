// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Deterministic full-document and static-route generation for PliegoRS.

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};
use fs2::FileExt;
use pliego_artifact::{
    ArtifactError, ArtifactReceipt, BuildContext, BuildInvocation, OutputFile, OutputNamespace,
    PortablePath, PreviousOwnership, VerifiedBuild, encode_build_report,
    invocation_from_environment, sha256_bytes, verify_build_context_with_materials,
    verify_build_report,
};
pub use pliego_artifact::{BUILD_REPORT_VERSION, BuildReport};
#[cfg(test)]
use pliego_artifact::{FrameworkEvidence, Ownership, capture_build_context};
use pliego_dom::{View, render_html};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static BUILD_SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
std::thread_local! {
    static FAIL_STAGE_PUBLISH_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FAIL_BACKUP_OPEN_ONCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Debug)]
pub enum BuildError {
    InvalidPath(String),
    DuplicateRoute(String),
    DuplicateAsset(String),
    Artifact(ArtifactError),
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
            Self::Artifact(source) => write!(formatter, "{source}"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json(source) => write!(formatter, "cannot serialize build report: {source}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<ArtifactError> for BuildError {
    fn from(source: ArtifactError) -> Self {
        Self::Artifact(source)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Head {
    title: String,
    description: Option<String>,
    canonical: Option<String>,
    icons: Vec<String>,
    manifest: Option<String>,
    apple_touch_icon: Option<String>,
    alternates: Vec<(String, String)>,
    stylesheet_preloads: Vec<String>,
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
            stylesheet_preloads: Vec::new(),
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

    /// Preload a stylesheet that is also applied by this [`Head`].
    ///
    /// Preload selection remains an explicit application delivery decision. Rendering fails when
    /// the same URL was not added with [`Head::stylesheet`] or when a preload is duplicated.
    #[must_use]
    pub fn preload_stylesheet(mut self, href: impl Into<String>) -> Self {
        self.stylesheet_preloads.push(href.into());
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
        for stylesheet in &self.head.stylesheet_preloads {
            output.push_str("<link rel=\"preload\" as=\"style\" href=\"");
            output.push_str(&escape_attribute(stylesheet));
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
        if let Some(invocation) = invocation_from_environment()? {
            return self.build_with_invocation(output_dir, invocation);
        }
        #[cfg(test)]
        {
            let project_root = std::env::current_dir().map_err(|source| BuildError::Io {
                path: PathBuf::from("."),
                source,
            })?;
            let context = direct_build_context(output_dir)?;
            self.build_with_context_at(output_dir, context, Some(&project_root), &[])
        }
        #[cfg(not(test))]
        Err(BuildError::InvalidPath(
            "verified publication requires `pliego build`; direct Site::build cannot bind the complete Cargo input graph"
                .to_owned(),
        ))
    }

    fn build_with_invocation(
        &self,
        output_dir: &Path,
        invocation: BuildInvocation,
    ) -> Result<BuildReport, BuildError> {
        validate_authorized_output(
            output_dir,
            &invocation.project_root,
            &invocation.output_path,
        )?;
        self.build_with_context_at(
            output_dir,
            invocation.context,
            Some(&invocation.project_root),
            &invocation.material_specs,
        )
    }

    #[cfg(test)]
    fn build_with_context(
        &self,
        output_dir: impl AsRef<Path>,
        project_root: impl AsRef<Path>,
        context: BuildContext,
    ) -> Result<BuildReport, BuildError> {
        self.build_with_context_at(
            output_dir.as_ref(),
            context,
            Some(project_root.as_ref()),
            &[],
        )
    }

    fn build_with_context_at(
        &self,
        output_dir: &Path,
        context: BuildContext,
        input_root: Option<&Path>,
        material_specs: &[pliego_artifact::InputMaterialSpec],
    ) -> Result<BuildReport, BuildError> {
        validate_output_target(output_dir)?;
        let mut namespace = OutputNamespace::new();
        namespace.insert_str(pliego_artifact::BUILD_LEDGER_NAME, "framework ledger")?;
        let mut files = BTreeMap::<String, PendingOutput>::new();
        let mut routes = BTreeSet::new();

        for page in &self.pages {
            validate_route(&page.route)?;
            if !routes.insert(page.route.clone()) {
                return Err(BuildError::DuplicateRoute(page.route.clone()));
            }
            let output_path = route_output_path(&page.route);
            let output_path =
                namespace.insert_str(&output_path, format!("route {}", page.route))?;
            files.insert(
                output_path.as_str().to_owned(),
                PendingOutput {
                    bytes: page.render()?.into_bytes(),
                    kind: "route",
                    producer: page.route.clone(),
                },
            );
        }
        for asset in &self.assets {
            let output_path = namespace.insert_str(&asset.path, format!("asset {}", asset.path))?;
            files.insert(
                output_path.as_str().to_owned(),
                PendingOutput {
                    bytes: asset.bytes.clone(),
                    kind: "asset",
                    producer: asset.path.clone(),
                },
            );
        }

        validate_pending_output_sizes(files.values().map(|output| output.bytes.len()))?;
        let emitted = files
            .iter()
            .map(|(relative, output)| {
                OutputFile::new(
                    relative.clone(),
                    output.kind,
                    output.producer.clone(),
                    &output.bytes,
                )
            })
            .collect::<Vec<_>>();
        let mut receipt = ArtifactReceipt::from_context_and_files(context, emitted)?;
        let preflight_report = BuildReport::new(receipt.clone())?;
        encode_build_report(&preflight_report)?;
        let parent_path = output_dir.parent().unwrap_or_else(|| Path::new("."));
        let output_name = output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("site");
        let publication_parent = open_or_create_directory_nofollow(parent_path)?;
        let _publication_lock =
            PublicationLock::acquire(&publication_parent, parent_path, output_name)?;
        let prior = if entry_exists_nofollow(&publication_parent, output_name, parent_path)? {
            Some(validate_replaceable_output(
                &publication_parent,
                parent_path,
                output_name,
                output_dir,
                &receipt.context.ownership.project_id,
            )?)
        } else {
            None
        };
        let previous_ownership = prior.as_ref().map(|prior| PreviousOwnership {
            project_id: prior
                .verified
                .report
                .receipt
                .context
                .ownership
                .project_id
                .clone(),
            site_package: prior
                .verified
                .report
                .receipt
                .context
                .ownership
                .site_package
                .clone(),
            receipt_sha256: prior.verified.report.receipt_sha256.clone(),
        });
        receipt.previous_ownership = previous_ownership;
        let report = BuildReport::new(receipt)?;
        let ledger = encode_build_report(&report)?;
        if let Some(input_root) = input_root {
            verify_build_context_with_materials(
                input_root,
                material_specs,
                &report.receipt.context,
            )
            .map_err(|error| {
                BuildError::InvalidPath(format!("build inputs changed before publication: {error}"))
            })?;
        }
        if prior.as_ref().is_some_and(|current_prior| {
            report
                .receipt
                .same_artifact_core(&current_prior.verified.report.receipt)
        }) {
            return Ok(prior
                .as_ref()
                .expect("prior is present after is_some_and")
                .verified
                .report
                .clone());
        }

        let mut stage =
            StageDirectory::create(&publication_parent, parent_path.to_owned(), output_name)?;
        for (relative, output) in files {
            stage.write_new_file(&relative, &output.bytes)?;
        }
        stage.write_new_file(pliego_artifact::BUILD_LEDGER_NAME, &ledger)?;
        stage.ensure_named()?;
        verify_build_report(&stage.path())?;
        stage.ensure_named()?;
        if let Some(input_root) = input_root {
            verify_build_context_with_materials(
                input_root,
                material_specs,
                &report.receipt.context,
            )
            .map_err(|error| {
                BuildError::InvalidPath(format!("build inputs changed before publication: {error}"))
            })?;
        }

        if let Some(expected_prior) = prior {
            let current_prior = validate_replaceable_output(
                &publication_parent,
                parent_path,
                output_name,
                output_dir,
                &report.receipt.context.ownership.project_id,
            )?;
            if current_prior.identity != expected_prior.identity
                || current_prior.verified.report.receipt_sha256
                    != expected_prior.verified.report.receipt_sha256
            {
                return Err(BuildError::InvalidPath(format!(
                    "output changed during build: {}",
                    output_dir.display()
                )));
            }
            let backup_name =
                allocate_private_name(&publication_parent, parent_path, output_name, "backup")?;
            publication_parent
                .rename(output_name, &publication_parent, &backup_name)
                .map_err(|source| BuildError::Io {
                    path: output_dir.to_owned(),
                    source,
                })?;
            let backup = parent_path.join(&backup_name);
            #[cfg(test)]
            let backup_open = FAIL_BACKUP_OPEN_ONCE.with(|fail| {
                if fail.replace(false) {
                    Err(std::io::Error::other("injected backup reopen failure"))
                } else {
                    publication_parent.open_dir_nofollow(&backup_name)
                }
            });
            #[cfg(not(test))]
            let backup_open = publication_parent.open_dir_nofollow(&backup_name);
            let backup_dir = match backup_open {
                Ok(directory) => directory,
                Err(open_source) => {
                    if let Err(rollback_source) =
                        publication_parent.rename(&backup_name, &publication_parent, output_name)
                    {
                        return Err(BuildError::Io {
                            path: backup,
                            source: rollback_source,
                        });
                    }
                    return Err(BuildError::Io {
                        path: backup,
                        source: open_source,
                    });
                }
            };
            let mut backup_cleanup = OpenDirectoryCleanup::new(backup_dir);
            if let Err(publish_error) = stage.publish_as(output_name) {
                backup_cleanup.disarm();
                let rollback =
                    publication_parent.rename(&backup_name, &publication_parent, output_name);
                if let Err(rollback_source) = rollback {
                    return Err(BuildError::Io {
                        path: backup,
                        source: rollback_source,
                    });
                }
                return Err(publish_error);
            }
            backup_cleanup
                .remove_now()
                .map_err(|source| BuildError::Io {
                    path: backup,
                    source,
                })?;
        } else {
            if entry_exists_nofollow(&publication_parent, output_name, parent_path)? {
                return Err(BuildError::InvalidPath(format!(
                    "output appeared during build: {}",
                    output_dir.display()
                )));
            }
            stage.publish_as(output_name)?;
        }
        Ok(report)
    }
}

struct PendingOutput {
    bytes: Vec<u8>,
    kind: &'static str,
    producer: String,
}

fn validate_pending_output_sizes(sizes: impl IntoIterator<Item = usize>) -> Result<(), BuildError> {
    let mut total = 0_u64;
    for size in sizes {
        let size = u64::try_from(size).map_err(|_| {
            ArtifactError::InvalidReceipt("output size cannot be represented as u64".to_owned())
        })?;
        if size > pliego_artifact::MAX_OUTPUT_FILE_BYTES {
            return Err(ArtifactError::InvalidReceipt(format!(
                "output exceeds the per-file limit of {} bytes",
                pliego_artifact::MAX_OUTPUT_FILE_BYTES
            ))
            .into());
        }
        total = total
            .checked_add(size)
            .filter(|total| *total <= pliego_artifact::MAX_OUTPUT_TOTAL_BYTES)
            .ok_or_else(|| {
                ArtifactError::InvalidReceipt(format!(
                    "output set exceeds the aggregate limit of {} bytes",
                    pliego_artifact::MAX_OUTPUT_TOTAL_BYTES
                ))
            })?;
    }
    Ok(())
}

fn validate_output_target(output: &Path) -> Result<(), BuildError> {
    if output.as_os_str().is_empty() || output.file_name().and_then(|name| name.to_str()).is_none()
    {
        return Err(BuildError::InvalidPath(output.display().to_string()));
    }
    Ok(())
}

fn validate_authorized_output(
    requested: &Path,
    project_root: &Path,
    authorized: &str,
) -> Result<(), BuildError> {
    let authorized = PortablePath::parse(authorized)?;
    let requested_relative = if requested.is_absolute() {
        requested.strip_prefix(project_root).map_err(|_| {
            BuildError::InvalidPath(
                "requested output is outside the project root authorized by pliego.toml".to_owned(),
            )
        })?
    } else {
        let current = std::env::current_dir()
            .and_then(|current| current.canonicalize())
            .map_err(|source| BuildError::Io {
                path: PathBuf::from("."),
                source,
            })?;
        if current != project_root {
            return Err(BuildError::InvalidPath(
                "relative output requires the site process to run from its canonical project root"
                    .to_owned(),
            ));
        }
        requested
    };
    let requested = portable_path_from_native(requested_relative)?;
    if requested != authorized {
        return Err(BuildError::InvalidPath(format!(
            "requested output {requested:?} does not match pliego.toml output {authorized:?}"
        )));
    }
    Ok(())
}

fn portable_path_from_native(path: &Path) -> Result<PortablePath, BuildError> {
    let mut components = Vec::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err(BuildError::InvalidPath(path.display().to_string()));
        };
        let component = component
            .to_str()
            .ok_or_else(|| BuildError::InvalidPath(path.display().to_string()))?;
        components.push(component);
    }
    let raw = components.join("/");
    let portable = PortablePath::parse(&raw)?;
    if portable.as_str() != raw {
        return Err(BuildError::InvalidPath(format!(
            "requested output is not canonical NFC: {raw:?}"
        )));
    }
    Ok(portable)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

struct ReplaceableOutput {
    verified: VerifiedBuild,
    identity: FileIdentity,
}

fn file_identity(metadata: &cap_std::fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: MetadataExt::dev(metadata),
        inode: MetadataExt::ino(metadata),
    }
}

fn validate_replaceable_output(
    parent: &Dir,
    parent_path: &Path,
    output_name: &str,
    output: &Path,
    expected_project_id: &str,
) -> Result<ReplaceableOutput, BuildError> {
    let opened = parent
        .open_dir_nofollow(output_name)
        .map_err(|source| BuildError::Io {
            path: parent_path.join(output_name),
            source,
        })?;
    let metadata = opened.dir_metadata().map_err(|source| BuildError::Io {
        path: parent_path.join(output_name),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(BuildError::InvalidPath(format!(
            "refusing to replace non-directory or linked output {}",
            output.display()
        )));
    }
    let identity = file_identity(&metadata);
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
    let verified = verify_build_report(&canonical).map_err(|error| {
        BuildError::InvalidPath(format!(
            "refusing to replace unverified output {}: {error}",
            output.display()
        ))
    })?;
    if verified.report.receipt.context.ownership.project_id != expected_project_id {
        return Err(BuildError::InvalidPath(format!(
            "output {} belongs to project {:?}, not {:?}",
            output.display(),
            verified.report.receipt.context.ownership.project_id,
            expected_project_id
        )));
    }
    let reopened = parent
        .open_dir_nofollow(output_name)
        .map_err(|source| BuildError::Io {
            path: parent_path.join(output_name),
            source,
        })?;
    let reopened_identity =
        file_identity(&reopened.dir_metadata().map_err(|source| BuildError::Io {
            path: parent_path.join(output_name),
            source,
        })?);
    if reopened_identity != identity {
        return Err(BuildError::InvalidPath(format!(
            "output changed while it was being verified: {}",
            output.display()
        )));
    }
    Ok(ReplaceableOutput { verified, identity })
}

#[cfg(test)]
fn direct_build_context(output: &Path) -> Result<BuildContext, BuildError> {
    let root = std::env::current_dir().map_err(|source| BuildError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    let configuration = ["pliego.toml", "Cargo.toml", "Cargo.lock"]
        .into_iter()
        .filter(|path| root.join(path).is_file())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if configuration.is_empty() {
        return Err(BuildError::InvalidPath(format!(
            "direct builds require pliego.toml or Cargo.toml below {}",
            root.display()
        )));
    }
    let mut excluded = Vec::new();
    if let Ok(relative) = output.strip_prefix(&root) {
        let relative = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(component) => component.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        if !relative.is_empty() {
            excluded.push(relative);
        }
    }
    capture_build_context(
        &root,
        Ownership {
            project_id: "direct-build".to_owned(),
            site_package: "direct-build".to_owned(),
        },
        FrameworkEvidence {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            source_revision: "test-source".to_owned(),
        },
        &configuration,
        &excluded,
    )
    .map_err(BuildError::from)
}

fn open_or_create_directory_nofollow(path: &Path) -> Result<Dir, BuildError> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| BuildError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => anchor.push(prefix.as_os_str()),
            Component::RootDir => anchor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(BuildError::InvalidPath(format!(
                    "publication parent contains parent traversal: {}",
                    path.display()
                )));
            }
            Component::Normal(name) => names.push(name.to_os_string()),
        }
    }
    if anchor.as_os_str().is_empty() {
        return Err(BuildError::InvalidPath(format!(
            "publication parent has no filesystem root: {}",
            path.display()
        )));
    }
    let mut directory =
        Dir::open_ambient_dir(&anchor, ambient_authority()).map_err(|source| BuildError::Io {
            path: anchor.clone(),
            source,
        })?;
    let mut traversed = anchor;
    for name in names {
        traversed.push(&name);
        directory = match directory.open_dir_nofollow(&name) {
            Ok(next) => next,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                match directory.create_dir(&name) {
                    Ok(()) => {}
                    Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(source) => {
                        return Err(BuildError::Io {
                            path: traversed.clone(),
                            source,
                        });
                    }
                }
                directory.open_dir_nofollow(&name).map_err(|source| {
                    if directory.symlink_metadata(&name).is_ok_and(|metadata| {
                        metadata.file_type().is_symlink() || !metadata.is_dir()
                    }) {
                        BuildError::InvalidPath(format!(
                            "output ancestor must be a real directory: {}",
                            traversed.display()
                        ))
                    } else {
                        BuildError::Io {
                            path: traversed.clone(),
                            source,
                        }
                    }
                })?
            }
            Err(source) => {
                if directory
                    .symlink_metadata(&name)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
                {
                    return Err(BuildError::InvalidPath(format!(
                        "output ancestor must be a real directory: {}",
                        traversed.display()
                    )));
                }
                return Err(BuildError::Io {
                    path: traversed.clone(),
                    source,
                });
            }
        };
    }
    Ok(directory)
}

fn entry_exists_nofollow(parent: &Dir, name: &str, parent_path: &Path) -> Result<bool, BuildError> {
    match parent.symlink_metadata(name) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(BuildError::Io {
            path: parent_path.join(name),
            source,
        }),
    }
}

struct StageDirectory {
    parent: Dir,
    parent_path: PathBuf,
    name: String,
    directory: Option<Dir>,
    directories: BTreeMap<String, FileIdentity>,
    armed: bool,
}

impl StageDirectory {
    fn create(parent: &Dir, parent_path: PathBuf, output_name: &str) -> Result<Self, BuildError> {
        let retained_parent = parent.try_clone().map_err(|source| BuildError::Io {
            path: parent_path.clone(),
            source,
        })?;
        let token = private_output_token(output_name)?;
        for _ in 0..64 {
            let name = format!(
                ".pliego-{token}-stage-{}-{}",
                std::process::id(),
                BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            match parent.create_dir(&name) {
                Ok(()) => {
                    let directory = parent.open_dir_nofollow(&name).map_err(|source| {
                        let _ = parent.remove_dir_all(&name);
                        BuildError::Io {
                            path: parent_path.join(&name),
                            source,
                        }
                    })?;
                    return Ok(Self {
                        parent: retained_parent,
                        parent_path,
                        name,
                        directory: Some(directory),
                        directories: BTreeMap::new(),
                        armed: true,
                    });
                }
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(BuildError::Io {
                        path: parent_path.join(name),
                        source,
                    });
                }
            }
        }
        Err(BuildError::InvalidPath(format!(
            "cannot allocate a private staging directory below {}",
            parent_path.display()
        )))
    }

    fn path(&self) -> PathBuf {
        self.parent_path.join(&self.name)
    }

    fn directory(&self) -> Result<&Dir, BuildError> {
        self.directory.as_ref().ok_or_else(|| {
            BuildError::InvalidPath(format!(
                "staging directory {} is closed",
                self.path().display()
            ))
        })
    }

    fn ensure_named(&self) -> Result<(), BuildError> {
        let expected =
            file_identity(
                &self
                    .directory()?
                    .dir_metadata()
                    .map_err(|source| BuildError::Io {
                        path: self.path(),
                        source,
                    })?,
            );
        let named = self
            .parent
            .open_dir_nofollow(&self.name)
            .map_err(|source| BuildError::Io {
                path: self.path(),
                source,
            })?;
        let actual = file_identity(&named.dir_metadata().map_err(|source| BuildError::Io {
            path: self.path(),
            source,
        })?);
        if actual != expected {
            return Err(BuildError::InvalidPath(format!(
                "staging directory name changed during publication: {}",
                self.path().display()
            )));
        }
        Ok(())
    }

    fn write_new_file(&mut self, relative: &str, bytes: &[u8]) -> Result<(), BuildError> {
        let portable = pliego_artifact::PortablePath::parse(relative)?;
        if portable.as_str() != relative {
            return Err(BuildError::InvalidPath(format!(
                "output path is not in canonical portable form: {relative}"
            )));
        }
        let mut components = portable.as_str().split('/').collect::<Vec<_>>();
        let leaf = components
            .pop()
            .ok_or_else(|| BuildError::InvalidPath(relative.to_owned()))?;
        let mut directory = self
            .directory()?
            .try_clone()
            .map_err(|source| BuildError::Io {
                path: self.path(),
                source,
            })?;
        let mut prefix = String::new();
        for component in components {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            let next =
                if let Some(expected) = self.directories.get(&prefix).copied() {
                    let next = directory.open_dir_nofollow(component).map_err(|source| {
                        BuildError::Io {
                            path: self.path().join(&prefix),
                            source,
                        }
                    })?;
                    let actual =
                        file_identity(&next.dir_metadata().map_err(|source| BuildError::Io {
                            path: self.path().join(&prefix),
                            source,
                        })?);
                    if actual != expected {
                        return Err(BuildError::InvalidPath(format!(
                            "staging directory changed during publication: {}",
                            self.path().join(&prefix).display()
                        )));
                    }
                    next
                } else {
                    directory
                        .create_dir(component)
                        .map_err(|source| BuildError::Io {
                            path: self.path().join(&prefix),
                            source,
                        })?;
                    let next = directory.open_dir_nofollow(component).map_err(|source| {
                        BuildError::Io {
                            path: self.path().join(&prefix),
                            source,
                        }
                    })?;
                    let identity =
                        file_identity(&next.dir_metadata().map_err(|source| BuildError::Io {
                            path: self.path().join(&prefix),
                            source,
                        })?);
                    self.directories.insert(prefix.clone(), identity);
                    next
                };
            directory = next;
        }

        let destination = self.path().join(portable.as_str());
        let mut options = CapOpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = directory
            .open_with(leaf, &options)
            .map_err(|source| BuildError::Io {
                path: destination.clone(),
                source,
            })?;
        let before = file.metadata().map_err(|source| BuildError::Io {
            path: destination.clone(),
            source,
        })?;
        if !before.is_file() || MetadataExt::nlink(&before) != 1 {
            return Err(BuildError::InvalidPath(format!(
                "new output must be a singly linked regular file: {}",
                destination.display()
            )));
        }
        file.write_all(bytes).map_err(|source| BuildError::Io {
            path: destination.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| BuildError::Io {
            path: destination.clone(),
            source,
        })?;
        let after = file.metadata().map_err(|source| BuildError::Io {
            path: destination.clone(),
            source,
        })?;
        if !after.is_file() || MetadataExt::nlink(&after) != 1 || after.len() != bytes.len() as u64
        {
            return Err(BuildError::InvalidPath(format!(
                "output changed while it was written: {}",
                destination.display()
            )));
        }
        Ok(())
    }

    fn publish_as(&mut self, output_name: &str) -> Result<(), BuildError> {
        self.ensure_named()?;
        let expected =
            file_identity(
                &self
                    .directory()?
                    .dir_metadata()
                    .map_err(|source| BuildError::Io {
                        path: self.path(),
                        source,
                    })?,
            );
        self.directory.take();
        #[cfg(test)]
        let rename_result = FAIL_STAGE_PUBLISH_ONCE.with(|fail| {
            if fail.replace(false) {
                Err(std::io::Error::other("injected stage publication failure"))
            } else {
                self.parent.rename(&self.name, &self.parent, output_name)
            }
        });
        #[cfg(not(test))]
        let rename_result = self.parent.rename(&self.name, &self.parent, output_name);
        match rename_result {
            Ok(()) => {
                self.armed = false;
                Ok(())
            }
            Err(source) => {
                if let Ok(directory) = self.parent.open_dir_nofollow(&self.name) {
                    if directory
                        .dir_metadata()
                        .map(|metadata| file_identity(&metadata) == expected)
                        .unwrap_or(false)
                    {
                        self.directory = Some(directory);
                    }
                }
                Err(BuildError::Io {
                    path: self.parent_path.join(output_name),
                    source,
                })
            }
        }
    }
}

impl Drop for StageDirectory {
    fn drop(&mut self) {
        if self.armed {
            if let Some(directory) = self.directory.take() {
                let _ = directory.remove_open_dir_all();
            }
        }
    }
}

struct OpenDirectoryCleanup {
    directory: Option<Dir>,
}

impl OpenDirectoryCleanup {
    fn new(directory: Dir) -> Self {
        Self {
            directory: Some(directory),
        }
    }

    fn disarm(&mut self) {
        self.directory.take();
    }

    fn remove_now(&mut self) -> std::io::Result<()> {
        if let Some(directory) = self.directory.take() {
            directory.remove_open_dir_all()
        } else {
            Ok(())
        }
    }
}

impl Drop for OpenDirectoryCleanup {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = directory.remove_open_dir_all();
        }
    }
}

struct PublicationLock {
    file: File,
}

impl PublicationLock {
    fn acquire(parent: &Dir, parent_path: &Path, output_name: &str) -> Result<Self, BuildError> {
        let name = publication_lock_name(output_name)?;
        let path = parent_path.join(&name);
        let mut options = CapOpenOptions::new();
        options
            .create(true)
            .read(true)
            .write(true)
            .follow(FollowSymlinks::No);
        let file = parent
            .open_with(&name, &options)
            .map_err(|source| BuildError::Io {
                path: path.clone(),
                source,
            })?;
        let metadata = file.metadata().map_err(|source| BuildError::Io {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() || MetadataExt::nlink(&metadata) != 1 {
            return Err(BuildError::InvalidPath(format!(
                "invalid publication lock {}",
                path.display()
            )));
        }
        let file = file.into_std();
        file.try_lock_exclusive().map_err(|source| {
            if is_lock_contention(&source) {
                BuildError::InvalidPath(format!(
                    "another build owns publication lock {}",
                    path.display()
                ))
            } else {
                BuildError::Io { path, source }
            }
        })?;
        Ok(Self { file })
    }
}

fn is_lock_contention(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33))
}

impl Drop for PublicationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn allocate_private_name(
    parent: &Dir,
    parent_path: &Path,
    output_name: &str,
    purpose: &str,
) -> Result<String, BuildError> {
    let token = private_output_token(output_name)?;
    for _ in 0..64 {
        let name = format!(
            ".pliego-{token}-{purpose}-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        match parent.symlink_metadata(&name) {
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(name),
            Ok(_) => continue,
            Err(source) => {
                return Err(BuildError::Io {
                    path: parent_path.join(name),
                    source,
                });
            }
        }
    }
    Err(BuildError::InvalidPath(format!(
        "cannot allocate a private {purpose} path below {}",
        parent_path.display()
    )))
}

fn publication_lock_name(output_name: &str) -> Result<String, BuildError> {
    Ok(format!(
        ".pliego-{}.lock",
        private_output_token(output_name)?
    ))
}

fn private_output_token(output_name: &str) -> Result<String, BuildError> {
    let portable = PortablePath::parse(output_name)?;
    if portable.as_str() != output_name || output_name.contains('/') {
        return Err(BuildError::InvalidPath(format!(
            "output leaf is not a canonical portable name: {output_name:?}"
        )));
    }
    Ok(sha256_bytes(portable.collision_key().as_bytes()))
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
        .chain(
            head.stylesheet_preloads
                .iter()
                .map(|value| ("stylesheet-preload", value)),
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
    let linked_stylesheets = head.stylesheets.iter().collect::<BTreeSet<_>>();
    let mut seen_preloads = BTreeSet::new();
    for preload in &head.stylesheet_preloads {
        if !linked_stylesheets.contains(preload) {
            return Err(BuildError::InvalidPath(format!(
                "stylesheet preload has no matching stylesheet: {preload}"
            )));
        }
        if !seen_preloads.insert(preload) {
            return Err(BuildError::InvalidPath(format!(
                "duplicate stylesheet preload: {preload}"
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

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_dom::{IntoView, el};
    use std::fs;

    fn test_project(parent: &Path, project_id: &str) -> (PathBuf, BuildContext) {
        let project = parent.join(format!("project-{project_id}"));
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("pliego.toml"),
            format!("[project]\nid = \"{project_id}\"\n"),
        )
        .unwrap();
        fs::write(project.join("src/main.rs"), b"fn main() {}\n").unwrap();
        let context = capture_build_context(
            &project,
            Ownership {
                project_id: project_id.to_owned(),
                site_package: project_id.to_owned(),
            },
            FrameworkEvidence {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                source_revision: "test-source".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &[],
        )
        .unwrap();
        (project, context)
    }

    fn is_private_build_directory(name: &str) -> bool {
        name.starts_with(".pliego-") && (name.contains("-stage-") || name.contains("-backup-"))
    }

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
    fn stylesheet_preloads_are_explicit_unique_and_early() {
        let page = Page::new(
            "/preload",
            Head::new("Preload")
                .inline_script("document.documentElement.dataset.theme='dark'")
                .stylesheet("/theme.css")
                .preload_stylesheet("/theme.css"),
            el("main").into_view(),
        );
        let html = page.render().unwrap();
        let preload = r#"<link rel="preload" as="style" href="/theme.css">"#;
        let stylesheet = r#"<link rel="stylesheet" href="/theme.css">"#;
        assert_eq!(html.matches(preload).count(), 1);
        assert_eq!(html.matches(stylesheet).count(), 1);
        assert!(html.find(preload).unwrap() < html.find("dataset.theme").unwrap());
        assert!(html.find(preload).unwrap() < html.find(stylesheet).unwrap());
    }

    #[test]
    fn stylesheet_preloads_require_one_matching_stylesheet() {
        let orphan = Page::new(
            "/orphan",
            Head::new("Orphan").preload_stylesheet("/theme.css"),
            el("main").into_view(),
        );
        assert!(matches!(orphan.render(), Err(BuildError::InvalidPath(_))));

        let duplicate = Page::new(
            "/duplicate",
            Head::new("Duplicate")
                .stylesheet("/theme.css")
                .preload_stylesheet("/theme.css")
                .preload_stylesheet("/theme.css"),
            el("main").into_view(),
        );
        assert!(matches!(
            duplicate.render(),
            Err(BuildError::InvalidPath(_))
        ));
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
        let second = site.build(&root).unwrap();
        assert_eq!(first, second);
        fs::write(root.join("stale.txt"), b"stale").unwrap();
        assert!(site.build(&root).is_err());
        assert!(root.join("stale.txt").exists());
        assert!(
            Site::new()
                .asset(Asset::new("../secret", vec![]))
                .build(&root)
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn valid_invocation_cannot_publish_to_an_unauthorized_output() {
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-output-binding-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let project = parent.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("pliego.toml"),
            "[project]\nid = \"output-binding\"\noutput = \"target/site\"\n",
        )
        .unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        let context = capture_build_context(
            &project,
            Ownership {
                project_id: "output-binding".to_owned(),
                site_package: "output-binding".to_owned(),
            },
            FrameworkEvidence {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                source_revision: "test-source".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &["target/site".to_owned()],
        )
        .unwrap();
        let invocation = BuildInvocation {
            context,
            project_root: project.canonicalize().unwrap(),
            output_path: "target/site".to_owned(),
            material_specs: Vec::new(),
        };
        let sidecar = parent.join("invocation.json");
        pliego_artifact::write_build_invocation(&sidecar, &invocation).unwrap();
        let invocation = pliego_artifact::read_build_invocation(&sidecar).unwrap();
        let unauthorized = invocation.project_root.join("target/other");

        let error = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build_with_invocation(&unauthorized, invocation)
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match pliego.toml output"),
            "unexpected diagnostic: {error}"
        );
        assert!(
            !project.join("target").exists(),
            "authorization must fail before creating publication parents"
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn output_size_preflight_rejects_before_publication_parent_creation() {
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-size-preflight-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let oversized = usize::try_from(pliego_artifact::MAX_OUTPUT_FILE_BYTES + 1).unwrap();
        assert!(validate_pending_output_sizes([oversized]).is_err());
        let at_limit = usize::try_from(pliego_artifact::MAX_OUTPUT_FILE_BYTES).unwrap();
        assert!(validate_pending_output_sizes(std::iter::repeat_n(at_limit, 9)).is_err());
        assert!(
            !parent.exists(),
            "size preflight must not create publication parents"
        );
    }

    #[test]
    fn oversized_ledger_fails_before_publication_parent_or_stage_creation() {
        let fixture = std::env::temp_dir().join(format!(
            "pliego-ssg-ledger-fixture-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let (_, mut context) = test_project(&fixture, "oversized-ledger");
        context.framework.source_revision = "x".repeat(pliego_artifact::MAX_LEDGER_BYTES as usize);
        let publication_parent = std::env::temp_dir().join(format!(
            "pliego-ssg-ledger-publication-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let output = publication_parent.join("site");

        let error = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build_with_context_at(&output, context, None, &[])
            .unwrap_err();

        assert!(
            error.to_string().contains("build report exceeds"),
            "unexpected oversized-ledger diagnostic: {error}"
        );
        assert!(
            !publication_parent.exists(),
            "ledger preflight created publication filesystem state"
        );
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn private_publication_names_are_bounded_for_maximum_output_leafs() {
        let output_name = "x".repeat(255);
        assert!(PortablePath::parse(&output_name).is_ok());
        assert_eq!(
            private_output_token("Site").unwrap(),
            private_output_token("site").unwrap(),
            "portable aliases must share one publication lock"
        );
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-private-name-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&parent).unwrap();
        let directory = open_or_create_directory_nofollow(&parent).unwrap();

        let lock_name = publication_lock_name(&output_name).unwrap();
        assert!(lock_name.len() <= 255);
        let lock = PublicationLock::acquire(&directory, &parent, &output_name).unwrap();
        drop(lock);

        let stage = StageDirectory::create(&directory, parent.clone(), &output_name).unwrap();
        assert!(stage.name.len() <= 255);
        drop(stage);

        let backup = allocate_private_name(&directory, &parent, &output_name, "backup").unwrap();
        assert!(backup.len() <= 255);
        let _ = fs::remove_dir_all(parent);
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
    fn build_never_replaces_output_owned_by_another_project() {
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-wrong-owner-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let (project, first_context) = test_project(&parent, "first-owner");
        let output = parent.join("site");
        let site = Site::new().page(Page::new("/", Head::new("Owned"), el("main").into_view()));
        site.build_with_context(&output, &project, first_context)
            .unwrap();
        let receipt_before = fs::read(output.join(pliego_artifact::BUILD_LEDGER_NAME)).unwrap();
        let (_, second_context) = test_project(&parent, "second-owner");

        let result =
            site.build_with_context(&output, parent.join("project-second-owner"), second_context);
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("belongs to project") && error.contains("first-owner"),
            "unexpected ownership diagnostic: {error}"
        );
        assert_eq!(
            fs::read(output.join(pliego_artifact::BUILD_LEDGER_NAME)).unwrap(),
            receipt_before
        );
        assert!(output.join("index.html").is_file());
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn changed_build_links_real_previous_ownership_and_identical_rebuild_is_a_noop() {
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-lineage-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let (project, context) = test_project(&parent, "lineage-owner");
        let output = parent.join("site");
        let first_site = Site::new()
            .page(Page::new("/", Head::new("First"), el("main").into_view()))
            .asset(Asset::new("assets/site.css", b"body{color:black}".to_vec()));
        let first = first_site
            .build_with_context(&output, &project, context.clone())
            .unwrap();
        assert!(first.receipt.previous_ownership.is_none());

        let changed_site = Site::new()
            .page(Page::new("/", Head::new("Changed"), el("main").into_view()))
            .asset(Asset::new("assets/site.css", b"body{color:white}".to_vec()));
        let changed = changed_site
            .build_with_context(&output, &project, context.clone())
            .unwrap();
        let previous = changed.receipt.previous_ownership.as_ref().unwrap();
        assert_eq!(previous.project_id, "lineage-owner");
        assert_eq!(previous.site_package, "lineage-owner");
        assert_eq!(previous.receipt_sha256, first.receipt_sha256);
        let changed_ledger = fs::read(output.join(pliego_artifact::BUILD_LEDGER_NAME)).unwrap();

        let identical = changed_site
            .build_with_context(&output, &project, context)
            .unwrap();
        assert_eq!(identical, changed);
        assert_eq!(
            fs::read(output.join(pliego_artifact::BUILD_LEDGER_NAME)).unwrap(),
            changed_ledger
        );
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn publication_lock_rejects_a_second_concurrent_publisher_deterministically() {
        use std::sync::mpsc;

        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-concurrent-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&parent).unwrap();
        let first_parent = parent.clone();
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = std::thread::spawn(move || {
            let first_directory = open_or_create_directory_nofollow(&first_parent).unwrap();
            let _lock = PublicationLock::acquire(&first_directory, &first_parent, "site").unwrap();
            locked_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        locked_rx.recv().unwrap();

        let parent_directory = open_or_create_directory_nofollow(&parent).unwrap();
        let second = match PublicationLock::acquire(&parent_directory, &parent, "site") {
            Ok(_) => panic!("second concurrent publisher acquired the same lock"),
            Err(error) => error,
        };
        assert!(
            second
                .to_string()
                .contains("another build owns publication lock"),
            "unexpected lock diagnostic: {second}"
        );
        release_tx.send(()).unwrap();
        first.join().unwrap();

        let reacquired = PublicationLock::acquire(&parent_directory, &parent, "site");
        assert!(reacquired.is_ok(), "released lock could not be reacquired");
        drop(reacquired);
        let _ = fs::remove_dir_all(parent);
    }

    #[cfg(unix)]
    #[test]
    fn stage_writer_rejects_a_linked_directory_without_touching_outside_data() {
        use std::os::unix::fs::symlink;

        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-stage-link-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let outside = std::env::temp_dir().join(format!(
            "pliego-ssg-stage-link-outside-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&parent).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("site.css");
        fs::write(&sentinel, b"must survive").unwrap();
        let parent_directory = open_or_create_directory_nofollow(&parent).unwrap();
        let mut stage = StageDirectory::create(&parent_directory, parent.clone(), "site").unwrap();
        symlink(&outside, stage.path().join("assets")).unwrap();

        let result = stage.write_new_file("assets/site.css", b"must not escape");
        assert!(result.is_err(), "linked stage directory was accepted");
        drop(stage);
        assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
        assert_eq!(
            fs::read_dir(&parent).unwrap().count(),
            0,
            "failed stage was not removed through its open handle"
        );
        let _ = fs::remove_dir_all(parent);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn stage_writer_rejects_a_preexisting_hardlink_without_truncating_it() {
        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-stage-hardlink-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let outside = std::env::temp_dir().join(format!(
            "pliego-ssg-stage-hardlink-sentinel-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&parent).unwrap();
        fs::write(&outside, b"must survive").unwrap();
        let parent_directory = open_or_create_directory_nofollow(&parent).unwrap();
        let mut stage = StageDirectory::create(&parent_directory, parent.clone(), "site").unwrap();
        fs::hard_link(&outside, stage.path().join("index.html")).unwrap();

        let result = stage.write_new_file("index.html", b"must not truncate");
        assert!(result.is_err(), "preexisting hardlink was accepted");
        drop(stage);
        assert_eq!(fs::read(&outside).unwrap(), b"must survive");
        let _ = fs::remove_dir_all(parent);
        let _ = fs::remove_file(outside);
    }

    #[cfg(unix)]
    #[test]
    fn publication_lock_rejects_a_preexisting_symlink_without_opening_its_target() {
        use std::os::unix::fs::symlink;

        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-lock-link-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let outside = std::env::temp_dir().join(format!(
            "pliego-ssg-lock-link-sentinel-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&parent).unwrap();
        fs::write(&outside, b"must survive").unwrap();
        symlink(
            &outside,
            parent.join(publication_lock_name("site").unwrap()),
        )
        .unwrap();
        let parent_directory = open_or_create_directory_nofollow(&parent).unwrap();

        let result = PublicationLock::acquire(&parent_directory, &parent, "site");
        assert!(result.is_err(), "linked publication lock was accepted");
        assert_eq!(fs::read(&outside).unwrap(), b"must survive");
        let _ = fs::remove_dir_all(parent);
        let _ = fs::remove_file(outside);
    }

    #[cfg(unix)]
    #[test]
    fn build_rejects_a_symlinked_output_ancestor() {
        use std::os::unix::fs::symlink;

        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-linked-ancestor-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let real_parent = parent.join("real");
        fs::create_dir_all(&real_parent).unwrap();
        symlink(&real_parent, parent.join("alias")).unwrap();
        let (project, context) = test_project(&parent, "linked-ancestor");
        let output = parent.join("alias/site");

        let result = Site::new()
            .page(Page::new("/", Head::new("No"), el("main").into_view()))
            .build_with_context(&output, &project, context);
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("output ancestor must be a real directory"),
            "unexpected linked-ancestor diagnostic: {error}"
        );
        assert!(!real_parent.join("site").exists());
        let _ = fs::remove_dir_all(parent);
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
        file.set_len(pliego_artifact::MAX_LEDGER_BYTES + 1).unwrap();
        let sentinel = root.join("sentinel.txt");
        fs::write(&sentinel, b"must survive").unwrap();

        let result = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build(&root);
        assert!(matches!(result, Err(BuildError::InvalidPath(_))));
        assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
        assert_eq!(
            fs::metadata(&marker).unwrap().len(),
            pliego_artifact::MAX_LEDGER_BYTES + 1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn forged_legacy_marker_never_claims_an_existing_output() {
        let root = std::env::temp_dir().join(format!(
            "pliego-ssg-forged-ledger-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("pliego.build.json"),
            br#"{"reportVersion":"1.0.0","files":[]}"#,
        )
        .unwrap();
        let sentinel = root.join("customer-data.txt");
        fs::write(&sentinel, b"must survive").unwrap();

        let result = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build(&root);
        assert!(matches!(result, Err(BuildError::InvalidPath(_))));
        assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drifted_or_extended_owned_output_is_not_replaced() {
        for case in ["modified", "extra"] {
            let root = std::env::temp_dir().join(format!(
                "pliego-ssg-drift-{case}-{}-{}",
                std::process::id(),
                BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let site = Site::new()
                .page(Page::new("/", Head::new("Home"), el("main").into_view()))
                .asset(Asset::new("assets/site.css", b"body{}".to_vec()));
            site.build(&root).unwrap();

            let protected = if case == "modified" {
                let path = root.join("index.html");
                fs::write(&path, b"tampered").unwrap();
                path
            } else {
                let path = root.join("unexpected.txt");
                fs::write(&path, b"customer data").unwrap();
                path
            };
            let before = fs::read(&protected).unwrap();
            let result = site.build(&root);
            assert!(
                matches!(result, Err(BuildError::InvalidPath(_))),
                "drifted output was replaced for case {case}"
            );
            assert_eq!(fs::read(&protected).unwrap(), before);
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn changed_inputs_discard_stage_before_publication() {
        let project = std::env::temp_dir().join(format!(
            "pliego-ssg-input-drift-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("pliego.toml"), b"project = true").unwrap();
        fs::write(project.join("src/main.rs"), b"fn main() {}").unwrap();
        let context = capture_build_context(
            &project,
            Ownership {
                project_id: "input-drift".to_owned(),
                site_package: "input-drift".to_owned(),
            },
            FrameworkEvidence {
                version: "0.0.1".to_owned(),
                source_revision: "test".to_owned(),
            },
            &["pliego.toml".to_owned()],
            &["target/site".to_owned()],
        )
        .unwrap();
        fs::write(project.join("src/main.rs"), b"fn main() { panic!() }").unwrap();
        let output = project.join("target/site");
        let result = Site::new()
            .page(Page::new("/", Head::new("Home"), el("main").into_view()))
            .build_with_context_at(&output, context, Some(&project), &[]);
        assert!(result.is_err());
        assert!(!output.exists());
        assert!(
            fs::read_dir(project.join("target"))
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !is_private_build_directory(&entry.file_name().to_string_lossy()))
        );
        let _ = fs::remove_dir_all(project);
    }

    #[test]
    fn failed_stage_publication_restores_the_previous_output_byte_for_byte() {
        fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
            fn walk(root: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
                for entry in fs::read_dir(current).unwrap() {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    if entry.file_type().unwrap().is_dir() {
                        walk(root, &path, files);
                    } else {
                        let relative = path
                            .strip_prefix(root)
                            .unwrap()
                            .components()
                            .map(|component| component.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/");
                        files.insert(relative, fs::read(path).unwrap());
                    }
                }
            }

            let mut files = BTreeMap::new();
            walk(root, root, &mut files);
            files
        }

        let parent = std::env::temp_dir().join(format!(
            "pliego-ssg-real-rollback-{}-{}",
            std::process::id(),
            BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let (project, context) = test_project(&parent, "rollback-owner");
        let output = parent.join("site");
        Site::new()
            .page(Page::new(
                "/",
                Head::new("Stable"),
                el("main").child("stable").into_view(),
            ))
            .asset(Asset::new("assets/site.css", b"stable".to_vec()))
            .build_with_context(&output, &project, context.clone())
            .unwrap();
        let before = snapshot(&output);

        FAIL_STAGE_PUBLISH_ONCE.with(|fail| fail.set(true));
        let result = Site::new()
            .page(Page::new(
                "/",
                Head::new("Changed"),
                el("main").child("changed").into_view(),
            ))
            .asset(Asset::new("assets/site.css", b"changed".to_vec()))
            .build_with_context(&output, &project, context.clone());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("injected stage publication failure"),
            "fault injection did not reach the publish branch: {error}"
        );
        assert_eq!(snapshot(&output), before);
        verify_build_report(&output).unwrap();
        let leftovers = fs::read_dir(&parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_private_build_directory(name))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "rollback left private directories: {leftovers:?}"
        );

        FAIL_BACKUP_OPEN_ONCE.with(|fail| fail.set(true));
        let result = Site::new()
            .page(Page::new(
                "/",
                Head::new("Changed again"),
                el("main").child("changed again").into_view(),
            ))
            .asset(Asset::new("assets/site.css", b"changed again".to_vec()))
            .build_with_context(&output, &project, context);
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("injected backup reopen failure"),
            "fault injection did not reach the backup reopen branch: {error}"
        );
        assert_eq!(snapshot(&output), before);
        verify_build_report(&output).unwrap();
        let leftovers = fs::read_dir(&parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_private_build_directory(name))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "backup-open rollback left private directories: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(parent);
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
        assert!(reserved.is_err());

        let failed = Site::new()
            .asset(Asset::new("assets/collision", b"file".to_vec()))
            .asset(Asset::new("assets/collision/nested", b"nested".to_vec()))
            .build(&output);
        assert!(failed.is_err());
        assert_eq!(fs::read(output.join("index.html")).unwrap(), original);
        let leftovers = fs::read_dir(&parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_private_build_directory(name))
            .collect::<Vec<_>>();
        assert!(
            leftovers.is_empty(),
            "leftover build directories: {leftovers:?}"
        );
        let _ = fs::remove_dir_all(parent);
    }

    fn collision_output(label: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "pliego-ssg-{label}-{}-{}",
                std::process::id(),
                BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ))
            .join("site")
    }

    #[test]
    fn route_aliases_are_rejected_before_publication() {
        let output = collision_output("route-alias");
        let result = Site::new()
            .page(Page::new(
                "/guide",
                Head::new("Guide"),
                el("main").into_view(),
            ))
            .page(Page::new(
                "/guide/",
                Head::new("Guide alias"),
                el("main").into_view(),
            ))
            .build(&output);
        assert!(result.is_err());
        assert!(!output.exists());
    }

    #[test]
    fn route_case_and_unicode_aliases_share_one_portable_namespace() {
        for (label, first, second) in [
            ("case", "/Guide", "/guide"),
            ("nfc", "/caf\u{e9}", "/cafe\u{301}"),
            ("casefold", "/Stra\u{df}e", "/STRASSE"),
        ] {
            let output = collision_output(label);
            let result = Site::new()
                .page(Page::new(first, Head::new("First"), el("main").into_view()))
                .page(Page::new(
                    second,
                    Head::new("Second"),
                    el("main").into_view(),
                ))
                .build(&output);
            assert!(
                result.is_err(),
                "portable alias was accepted: {first:?} versus {second:?}"
            );
            assert!(!output.exists());
        }
    }

    #[test]
    fn routes_and_assets_reject_exact_and_file_directory_collisions() {
        for (label, asset) in [("exact", "guide/index.html"), ("ancestor-file", "guide")] {
            let output = collision_output(label);
            let result = Site::new()
                .page(Page::new(
                    "/guide",
                    Head::new("Guide"),
                    el("main").into_view(),
                ))
                .asset(Asset::new(asset, b"collision".to_vec()))
                .build(&output);
            assert!(
                result.is_err(),
                "route/asset collision was accepted for {asset:?}"
            );
            assert!(!output.exists());
        }
    }

    #[test]
    fn directory_spelling_and_portability_are_validated_before_staging() {
        let output = collision_output("directory-case");
        let result = Site::new()
            .asset(Asset::new("Assets/a.css", b"a".to_vec()))
            .asset(Asset::new("assets/b.css", b"b".to_vec()))
            .build(&output);
        assert!(result.is_err());
        assert!(!output.exists());

        for path in ["CON", "assets/aux.txt", "assets/LPT1.css", "file.", "file "] {
            let output = collision_output("nonportable");
            let result = Site::new()
                .asset(Asset::new(path, Vec::new()))
                .build(&output);
            assert!(result.is_err(), "non-portable path was accepted: {path:?}");
            assert!(!output.exists());
        }
    }
}
