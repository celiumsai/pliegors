// SPDX-License-Identifier: Apache-2.0

//! Typed, deterministic, build-time content collections for PliegoRS.
//!
//! A collection recursively discovers JSON, TOML, and Markdown files without
//! following symbolic links. IDs are portable relative paths without the final
//! extension, entries are sorted by ID, and duplicate IDs are rejected using a
//! case-insensitive collision key.
//!
//! ```no_run
//! use pliego_content::{CollectionSpec, Frontmatter, MarkdownPolicy};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct Article {
//!     title: String,
//! }
//!
//! let collection = CollectionSpec::new("content/articles")
//!     .options(|options| options.markdown_policy(MarkdownPolicy::Safe))
//!     .load::<Article>()?;
//! let article = collection.get("journal/first-post").expect("known article");
//! assert_eq!(article.frontmatter(), Some(Frontmatter::Yaml));
//! let html = article.markdown().expect("Markdown entry")
//!     .render_html(MarkdownPolicy::Safe)?;
//! # let _ = (&article.data().title, html);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod diagnostic;
mod markdown;
mod snapshot;

pub use diagnostic::{ContentDiagnostic, DiagnosticCode, DiagnosticFormat, LoadError, SourceSpan};
pub use markdown::{
    MarkdownAst, MarkdownCodeBlock, MarkdownDocument, MarkdownEvent, MarkdownLinkKind,
    MarkdownPolicy, MarkdownRenderError, MarkdownSecurityIssue, MarkdownTag, MarkdownTagEnd,
    SpannedMarkdownEvent,
};
pub use snapshot::{CollectionSnapshot, ContentFingerprint, SnapshotDiff};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, ambient_authority};
use cap_std::fs::{Dir, OpenOptions};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::borrow::Borrow;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Read;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

/// Version mixed into every content fingerprint.
pub const FINGERPRINT_CONTRACT_VERSION: &str = "pliego-content-entry-v1";

/// Default maximum directory depth below the collection root.
pub const DEFAULT_MAX_DEPTH: usize = 32;
/// Default maximum number of filesystem entries visited during discovery.
pub const DEFAULT_MAX_ENTRIES: usize = 10_000;
/// Default maximum size of one content source (8 MiB).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Default maximum aggregate size of content sources (128 MiB).
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

/// Portable identity derived from a source path relative to its collection root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentId(String);

impl ContentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn collision_key(&self) -> String {
        self.0.to_lowercase()
    }
}

impl Borrow<str> for ContentId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// On-disk representation of an entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContentFormat {
    Json,
    Toml,
    Markdown,
}

/// Markdown metadata delimiter and parser.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Frontmatter {
    /// Canonical PliegoRS front matter (`+++`).
    Toml,
    /// Explicit compatibility front matter (`---`).
    Yaml,
}

/// Immutable typed content entry.
#[derive(Clone, Debug)]
pub struct Entry<T> {
    id: ContentId,
    source_path: PathBuf,
    relative_path: String,
    format: ContentFormat,
    frontmatter: Option<Frontmatter>,
    data: T,
    markdown: Option<MarkdownDocument>,
    fingerprint: ContentFingerprint,
}

impl<T> Entry<T> {
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    pub const fn format(&self) -> ContentFormat {
        self.format
    }

    pub const fn frontmatter(&self) -> Option<Frontmatter> {
        self.frontmatter
    }

    pub const fn data(&self) -> &T {
        &self.data
    }

    pub const fn markdown(&self) -> Option<&MarkdownDocument> {
        self.markdown.as_ref()
    }

    pub const fn fingerprint(&self) -> &ContentFingerprint {
        &self.fingerprint
    }

    pub fn into_data(self) -> T {
        self.data
    }
}

/// Deterministically ordered entries loaded from one directory tree.
#[derive(Clone, Debug)]
pub struct Collection<T> {
    root: PathBuf,
    entries: Vec<Entry<T>>,
}

impl<T> Collection<T> {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry<T>] {
        &self.entries
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Entry<T>> {
        self.entries.iter()
    }

    pub fn get(&self, id: &str) -> Option<&Entry<T>> {
        self.entries
            .binary_search_by(|entry| entry.id.as_str().cmp(id))
            .ok()
            .map(|index| &self.entries[index])
    }

    pub fn snapshot(&self) -> CollectionSnapshot {
        CollectionSnapshot::from_collection(self)
    }

    pub fn into_entries(self) -> Vec<Entry<T>> {
        self.entries
    }
}

impl<'a, T> IntoIterator for &'a Collection<T> {
    type Item = &'a Entry<T>;
    type IntoIter = std::slice::Iter<'a, Entry<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

impl<T> IntoIterator for Collection<T> {
    type Item = Entry<T>;
    type IntoIter = std::vec::IntoIter<Entry<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

/// Resource ceilings enforced while discovering and reading a collection.
///
/// Directory depth is relative to the collection root, so a limit of zero
/// permits regular files in the root but rejects child directories. The entry
/// budget counts every filesystem entry visited, including directories and
/// ignored files, because all of them consume discovery work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentLimits {
    max_depth: usize,
    max_entries: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
}

impl ContentLimits {
    /// Create the secure default limits.
    pub const fn new() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
        }
    }

    #[must_use]
    pub const fn max_depth(mut self, value: usize) -> Self {
        self.max_depth = value;
        self
    }

    #[must_use]
    pub const fn max_entries(mut self, value: usize) -> Self {
        self.max_entries = value;
        self
    }

    #[must_use]
    pub const fn max_file_bytes(mut self, value: u64) -> Self {
        self.max_file_bytes = value;
        self
    }

    #[must_use]
    pub const fn max_total_bytes(mut self, value: u64) -> Self {
        self.max_total_bytes = value;
        self
    }

    pub const fn selected_max_depth(self) -> usize {
        self.max_depth
    }

    pub const fn selected_max_entries(self) -> usize {
        self.max_entries
    }

    pub const fn selected_max_file_bytes(self) -> u64 {
        self.max_file_bytes
    }

    pub const fn selected_max_total_bytes(self) -> u64 {
        self.max_total_bytes
    }
}

impl Default for ContentLimits {
    fn default() -> Self {
        Self::new()
    }
}

/// Loader behavior shared by all entries in a collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadOptions {
    markdown_policy: MarkdownPolicy,
    limits: ContentLimits,
}

impl LoadOptions {
    pub const fn new() -> Self {
        Self {
            markdown_policy: MarkdownPolicy::Safe,
            limits: ContentLimits::new(),
        }
    }

    #[must_use]
    pub const fn markdown_policy(mut self, policy: MarkdownPolicy) -> Self {
        self.markdown_policy = policy;
        self
    }

    pub const fn selected_markdown_policy(self) -> MarkdownPolicy {
        self.markdown_policy
    }

    #[must_use]
    pub const fn limits(mut self, limits: ContentLimits) -> Self {
        self.limits = limits;
        self
    }

    pub const fn selected_limits(self) -> ContentLimits {
        self.limits
    }
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Reusable specification for a typed directory-backed collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionSpec {
    root: PathBuf,
    options: LoadOptions,
}

impl CollectionSpec {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            options: LoadOptions::new(),
        }
    }

    #[must_use]
    pub fn with_options(mut self, options: LoadOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn options(mut self, configure: impl FnOnce(LoadOptions) -> LoadOptions) -> Self {
        self.options = configure(self.options);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn load_options(&self) -> LoadOptions {
        self.options
    }

    pub fn load<T>(&self) -> Result<Collection<T>, LoadError>
    where
        T: DeserializeOwned,
    {
        load_collection_with_options(&self.root, self.options)
    }
}

/// Load a collection with safe Markdown defaults.
pub fn load_collection<T>(root: impl AsRef<Path>) -> Result<Collection<T>, LoadError>
where
    T: DeserializeOwned,
{
    load_collection_with_options(root, LoadOptions::new())
}

/// Load a collection with explicit options.
pub fn load_collection_with_options<T>(
    root: impl AsRef<Path>,
    options: LoadOptions,
) -> Result<Collection<T>, LoadError>
where
    T: DeserializeOwned,
{
    let root = root.as_ref().to_path_buf();
    let mut discovered = Vec::new();
    let mut diagnostics = Vec::new();
    let mut budget = DiscoveryBudget::new(options.limits);
    discover(
        &root,
        &root,
        0,
        &mut discovered,
        &mut diagnostics,
        &mut budget,
    );
    if !diagnostics.is_empty() {
        return Err(LoadError::new(diagnostics));
    }

    let mut sources = Vec::new();
    for path in discovered {
        let Some(format) = detect_format(&path) else {
            continue;
        };
        match SourceFile::new(&root, path, format) {
            Ok(source) => sources.push(source),
            Err(diagnostic) => diagnostics.push(*diagnostic),
        }
    }
    if !diagnostics.is_empty() {
        return Err(LoadError::new(diagnostics));
    }

    diagnostics.extend(collision_diagnostics(&sources));
    if !diagnostics.is_empty() {
        return Err(LoadError::new(diagnostics));
    }

    let confined_root = match open_confined_root(&root) {
        Ok(root) => root,
        Err(source) => return Err(LoadError::new(vec![io_diagnostic(&root, &root, source)])),
    };

    sources.sort_by(|left, right| left.id.cmp(&right.id));
    let mut entries = Vec::with_capacity(sources.len());
    let mut bytes_read = 0;
    for source in sources {
        match parse_source::<T>(&confined_root, &source, options, &mut bytes_read) {
            Ok(entry) => entries.push(entry),
            Err(diagnostic) => {
                let limit_reached = diagnostic.code().is_resource_limit();
                diagnostics.push(*diagnostic);
                if limit_reached {
                    break;
                }
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(LoadError::new(diagnostics));
    }
    Ok(Collection { root, entries })
}

#[derive(Clone, Copy, Debug)]
struct DiscoveryBudget {
    limits: ContentLimits,
    entries: usize,
    content_bytes: u64,
}

impl DiscoveryBudget {
    const fn new(limits: ContentLimits) -> Self {
        Self {
            limits,
            entries: 0,
            content_bytes: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct SourceFile {
    id: ContentId,
    path: PathBuf,
    relative_path: String,
    format: ContentFormat,
}

fn open_confined_root(root: &Path) -> std::io::Result<Dir> {
    let input = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    let mut absolute = PathBuf::new();
    for component in input.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                absolute.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !absolute.pop() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "collection root escapes the filesystem root",
                    ));
                }
            }
        }
    }
    let parent = absolute.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "collection root has no parent directory",
        )
    })?;
    let name = absolute.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "collection root has no final path component",
        )
    })?;
    let parent = Dir::open_ambient_dir(parent, ambient_authority())?;
    parent.open_dir_nofollow(name)
}

impl SourceFile {
    fn new(
        root: &Path,
        path: PathBuf,
        format: ContentFormat,
    ) -> Result<Self, Box<ContentDiagnostic>> {
        let relative = path.strip_prefix(root).map_err(|_| {
            ContentDiagnostic::new(
                DiagnosticCode::InvalidPath,
                path.clone(),
                None,
                Some(format.diagnostic_format()),
                None,
                "source escaped the collection root",
            )
        })?;
        let relative_path = normalized_relative_path(relative).map_err(|message| {
            ContentDiagnostic::new(
                DiagnosticCode::InvalidPath,
                path.clone(),
                None,
                Some(format.diagnostic_format()),
                None,
                message,
            )
        })?;
        let mut id_path = relative.to_path_buf();
        id_path.set_extension("");
        let id = normalized_relative_path(&id_path)
            .map(ContentId)
            .map_err(|message| {
                ContentDiagnostic::new(
                    DiagnosticCode::InvalidPath,
                    path.clone(),
                    Some(relative_path.clone()),
                    Some(format.diagnostic_format()),
                    None,
                    message,
                )
            })?;
        if id.as_str().is_empty() {
            return Err(ContentDiagnostic::new(
                DiagnosticCode::InvalidPath,
                path,
                Some(relative_path),
                Some(format.diagnostic_format()),
                None,
                "content ID is empty",
            )
            .into());
        }
        Ok(Self {
            id,
            path,
            relative_path,
            format,
        })
    }
}

impl ContentFormat {
    pub const fn as_str(self) -> &'static str {
        content_format_name(self)
    }

    const fn diagnostic_format(self) -> DiagnosticFormat {
        match self {
            Self::Json => DiagnosticFormat::Json,
            Self::Toml => DiagnosticFormat::Toml,
            Self::Markdown => DiagnosticFormat::Markdown,
        }
    }
}

fn discover(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<ContentDiagnostic>,
    budget: &mut DiscoveryBudget,
) -> bool {
    if depth > budget.limits.max_depth {
        diagnostics.push(simple_path_diagnostic(
            root,
            directory,
            DiagnosticCode::DepthLimitExceeded,
            format!(
                "collection depth {depth} exceeds configured maximum {}",
                budget.limits.max_depth
            ),
        ));
        return false;
    }
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(source) => {
            diagnostics.push(io_diagnostic(root, directory, source));
            return true;
        }
    };
    if is_link(&metadata) {
        diagnostics.push(simple_path_diagnostic(
            root,
            directory,
            DiagnosticCode::SymlinkRejected,
            "symbolic links are not allowed in content collections",
        ));
        return true;
    }
    if !metadata.is_dir() {
        diagnostics.push(simple_path_diagnostic(
            root,
            directory,
            DiagnosticCode::RootNotDirectory,
            "collection root is not a directory",
        ));
        return true;
    }

    let read_dir = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(source) => {
            diagnostics.push(io_diagnostic(root, directory, source));
            return true;
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        if budget.entries >= budget.limits.max_entries {
            diagnostics.push(simple_path_diagnostic(
                root,
                directory,
                DiagnosticCode::EntryLimitExceeded,
                format!(
                    "collection traversal exceeds configured maximum of {} filesystem entries",
                    budget.limits.max_entries
                ),
            ));
            return false;
        }
        budget.entries += 1;
        match entry {
            Ok(entry) => entries.push(entry),
            Err(source) => diagnostics.push(io_diagnostic(root, directory, source)),
        }
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(source) => {
                diagnostics.push(io_diagnostic(root, &path, source));
                continue;
            }
        };
        if is_link(&metadata) {
            diagnostics.push(simple_path_diagnostic(
                root,
                &path,
                DiagnosticCode::SymlinkRejected,
                "symbolic links are not allowed in content collections",
            ));
        } else if metadata.is_dir() {
            if !discover(
                root,
                &path,
                depth.saturating_add(1),
                files,
                diagnostics,
                budget,
            ) {
                return false;
            }
        } else if metadata.is_file() {
            if detect_format(&path).is_some() {
                let file_bytes = metadata.len();
                if file_bytes > budget.limits.max_file_bytes {
                    diagnostics.push(simple_path_diagnostic(
                        root,
                        &path,
                        DiagnosticCode::FileByteLimitExceeded,
                        format!(
                            "content source is {file_bytes} bytes; configured per-file maximum is {} bytes",
                            budget.limits.max_file_bytes
                        ),
                    ));
                    return false;
                }
                let remaining = budget
                    .limits
                    .max_total_bytes
                    .saturating_sub(budget.content_bytes);
                if file_bytes > remaining {
                    diagnostics.push(simple_path_diagnostic(
                        root,
                        &path,
                        DiagnosticCode::TotalByteLimitExceeded,
                        format!(
                            "content sources exceed configured aggregate maximum of {} bytes",
                            budget.limits.max_total_bytes
                        ),
                    ));
                    return false;
                }
                budget.content_bytes += file_bytes;
            }
            files.push(path);
        } else {
            diagnostics.push(simple_path_diagnostic(
                root,
                &path,
                DiagnosticCode::UnsupportedFileType,
                "only regular files and directories are allowed",
            ));
        }
    }
    true
}

fn collision_diagnostics(sources: &[SourceFile]) -> Vec<ContentDiagnostic> {
    let mut groups: BTreeMap<String, Vec<&SourceFile>> = BTreeMap::new();
    for source in sources {
        groups
            .entry(source.id.collision_key())
            .or_default()
            .push(source);
    }
    let mut diagnostics = Vec::new();
    for group in groups.values_mut() {
        group.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let Some((first, rest)) = group.split_first() else {
            continue;
        };
        for source in rest {
            diagnostics.push(
                ContentDiagnostic::new(
                    DiagnosticCode::IdCollision,
                    source.path.clone(),
                    Some(source.relative_path.clone()),
                    Some(source.format.diagnostic_format()),
                    None,
                    format!(
                        "content ID {:?} collides case-insensitively with {}",
                        source.id, first.relative_path
                    ),
                )
                .with_related_path(first.path.clone()),
            );
        }
    }
    diagnostics
}

fn read_source_bytes(
    root: &Dir,
    source: &SourceFile,
    limits: ContentLimits,
    bytes_read: &mut u64,
) -> Result<Vec<u8>, Box<ContentDiagnostic>> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = root
        .open_with(Path::new(&source.relative_path), &options)
        .map_err(|error| io_diagnostic_for_source(source, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_diagnostic_for_source(source, error))?;
    if !metadata.is_file() {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::UnsupportedFileType,
            Some(source.format.diagnostic_format()),
            None,
            "source handle is not a regular file",
        )
        .into());
    }
    let file_bytes = metadata.len();
    if file_bytes > limits.max_file_bytes {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::FileByteLimitExceeded,
            Some(source.format.diagnostic_format()),
            None,
            format!(
                "content source is {file_bytes} bytes; configured per-file maximum is {} bytes",
                limits.max_file_bytes
            ),
        )
        .into());
    }
    let total_remaining = limits.max_total_bytes.saturating_sub(*bytes_read);
    if file_bytes > total_remaining {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::TotalByteLimitExceeded,
            Some(source.format.diagnostic_format()),
            None,
            format!(
                "content sources exceed configured aggregate maximum of {} bytes",
                limits.max_total_bytes
            ),
        )
        .into());
    }

    let read_limit = limits.max_file_bytes.min(total_remaining).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| io_diagnostic_for_source(source, error))?;
    let actual_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual_bytes > limits.max_file_bytes {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::FileByteLimitExceeded,
            Some(source.format.diagnostic_format()),
            None,
            format!(
                "content source exceeds configured per-file maximum of {} bytes while reading",
                limits.max_file_bytes
            ),
        )
        .into());
    }
    if actual_bytes > total_remaining {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::TotalByteLimitExceeded,
            Some(source.format.diagnostic_format()),
            None,
            format!(
                "content sources exceed configured aggregate maximum of {} bytes while reading",
                limits.max_total_bytes
            ),
        )
        .into());
    }
    *bytes_read += actual_bytes;
    Ok(bytes)
}

fn parse_source<T>(
    root: &Dir,
    source: &SourceFile,
    options: LoadOptions,
    bytes_read: &mut u64,
) -> Result<Entry<T>, Box<ContentDiagnostic>>
where
    T: DeserializeOwned,
{
    let metadata = fs::symlink_metadata(&source.path)
        .map_err(|error| io_diagnostic_for_source(source, error))?;
    if is_link(&metadata) {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::SymlinkRejected,
            Some(source.format.diagnostic_format()),
            None,
            "source became a symbolic link during loading",
        )
        .into());
    }
    if !metadata.is_file() {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::UnsupportedFileType,
            Some(source.format.diagnostic_format()),
            None,
            "source is no longer a regular file",
        )
        .into());
    }
    let bytes = read_source_bytes(root, source, options.limits, bytes_read)?;
    let raw_text = std::str::from_utf8(&bytes).map_err(|error| {
        let lossy = String::from_utf8_lossy(&bytes);
        source_diagnostic(
            source,
            DiagnosticCode::InvalidUtf8,
            Some(source.format.diagnostic_format()),
            Some(SourceSpan::from_range(
                &lossy,
                error.valid_up_to(),
                error
                    .valid_up_to()
                    .saturating_add(error.error_len().unwrap_or(1)),
            )),
            format!("source is not UTF-8: {error}"),
        )
    })?;
    if raw_text.starts_with('\u{feff}') {
        return Err(source_diagnostic(
            source,
            DiagnosticCode::BomRejected,
            Some(source.format.diagnostic_format()),
            Some(SourceSpan::from_range(raw_text, 0, '\u{feff}'.len_utf8())),
            "UTF-8 byte order marks are not allowed",
        )
        .into());
    }
    let normalized = normalize_line_endings(raw_text);
    let text = normalized.as_ref();
    let fingerprint = fingerprint(source, options, text);

    let (data, frontmatter, markdown) = match source.format {
        ContentFormat::Json => {
            let data = serde_json::from_str(text).map_err(|error| {
                let span = span_from_line_column(text, error.line(), error.column());
                source_diagnostic(
                    source,
                    DiagnosticCode::Deserialize,
                    Some(DiagnosticFormat::Json),
                    Some(span),
                    format!("cannot deserialize JSON: {error}"),
                )
            })?;
            (data, None, None)
        }
        ContentFormat::Toml => {
            let data = toml::from_str(text).map_err(|error: toml::de::Error| {
                let span = error
                    .span()
                    .map(|range| SourceSpan::from_range(text, range.start, range.end));
                source_diagnostic(
                    source,
                    DiagnosticCode::Deserialize,
                    Some(DiagnosticFormat::Toml),
                    span,
                    format!("cannot deserialize TOML: {error}"),
                )
            })?;
            (data, None, None)
        }
        ContentFormat::Markdown => {
            let parts = split_frontmatter(text).map_err(|problem| {
                let (code, span, message) = match problem {
                    FrontmatterProblem::Missing { span } => (
                        DiagnosticCode::MissingFrontmatter,
                        span,
                        "Markdown must begin with TOML +++ or YAML --- front matter".to_owned(),
                    ),
                    FrontmatterProblem::Unterminated { frontmatter, span } => (
                        DiagnosticCode::UnterminatedFrontmatter,
                        span,
                        format!("unterminated {} front matter", frontmatter.as_str()),
                    ),
                };
                source_diagnostic(
                    source,
                    code,
                    Some(DiagnosticFormat::Markdown),
                    Some(span),
                    message,
                )
            })?;
            let data = match parts.frontmatter {
                Frontmatter::Toml => {
                    toml::from_str(parts.metadata).map_err(|error: toml::de::Error| {
                        let span = error.span().map(|range| {
                            SourceSpan::from_range(
                                text,
                                parts.metadata_offset + range.start,
                                parts.metadata_offset + range.end,
                            )
                        });
                        source_diagnostic(
                            source,
                            DiagnosticCode::Deserialize,
                            Some(DiagnosticFormat::TomlFrontmatter),
                            span,
                            format!("cannot deserialize TOML front matter: {error}"),
                        )
                    })?
                }
                Frontmatter::Yaml => serde_saphyr::from_str(parts.metadata).map_err(|error| {
                    let span = yaml_error_span(text, parts.metadata, parts.metadata_offset, &error);
                    source_diagnostic(
                        source,
                        DiagnosticCode::Deserialize,
                        Some(DiagnosticFormat::YamlFrontmatter),
                        span,
                        format!("cannot deserialize YAML front matter: {error}"),
                    )
                })?,
            };
            let markdown = markdown::parse_markdown(parts.body.to_owned(), options.markdown_policy)
                .map_err(|error| {
                    let body_span = error.span();
                    let span = SourceSpan::from_range(
                        text,
                        parts.body_offset + body_span.offset,
                        parts.body_offset + body_span.offset + body_span.length,
                    );
                    source_diagnostic(
                        source,
                        DiagnosticCode::UnsafeMarkdown,
                        Some(DiagnosticFormat::Markdown),
                        Some(span),
                        error.message(),
                    )
                })?;
            (data, Some(parts.frontmatter), Some(markdown))
        }
    };

    Ok(Entry {
        id: source.id.clone(),
        source_path: source.path.clone(),
        relative_path: source.relative_path.clone(),
        format: source.format,
        frontmatter,
        data,
        markdown,
        fingerprint,
    })
}

struct FrontmatterParts<'a> {
    frontmatter: Frontmatter,
    metadata: &'a str,
    metadata_offset: usize,
    body: &'a str,
    body_offset: usize,
}

#[derive(Debug)]
enum FrontmatterProblem {
    Missing {
        span: SourceSpan,
    },
    Unterminated {
        frontmatter: Frontmatter,
        span: SourceSpan,
    },
}

impl Frontmatter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Toml => "TOML +++",
            Self::Yaml => "YAML ---",
        }
    }
}

impl fmt::Display for Frontmatter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn split_frontmatter(source: &str) -> Result<FrontmatterParts<'_>, FrontmatterProblem> {
    let content = source;
    let (first_line, first_next) = line_at(content, 0);
    let frontmatter = match first_line {
        "+++" => Frontmatter::Toml,
        "---" => Frontmatter::Yaml,
        _ => {
            let end = first_next.saturating_sub(1).max(first_line.len());
            return Err(FrontmatterProblem::Missing {
                span: SourceSpan::from_range(source, 0, end),
            });
        }
    };
    let delimiter = match frontmatter {
        Frontmatter::Toml => "+++",
        Frontmatter::Yaml => "---",
    };
    let metadata_start = first_next;
    let mut cursor = first_next;
    while cursor < content.len() {
        let line_start = cursor;
        let (line, next) = line_at(content, cursor);
        if line == delimiter {
            return Ok(FrontmatterParts {
                frontmatter,
                metadata: &content[metadata_start..line_start],
                metadata_offset: metadata_start,
                body: &content[next..],
                body_offset: next,
            });
        }
        if next <= cursor {
            break;
        }
        cursor = next;
    }
    Err(FrontmatterProblem::Unterminated {
        frontmatter,
        span: SourceSpan::from_range(source, 0, delimiter.len()),
    })
}

fn line_at(source: &str, start: usize) -> (&str, usize) {
    let remainder = &source[start..];
    if let Some(relative_end) = remainder.find('\n') {
        let end = start + relative_end;
        let line = source[start..end]
            .strip_suffix('\r')
            .unwrap_or(&source[start..end]);
        (line, end + 1)
    } else {
        let line = source[start..]
            .strip_suffix('\r')
            .unwrap_or(&source[start..]);
        (line, source.len())
    }
}

fn yaml_error_span(
    source: &str,
    metadata: &str,
    metadata_offset: usize,
    error: &serde_saphyr::Error,
) -> Option<SourceSpan> {
    let location = error.location()?;
    let offset = offset_from_line_column(
        metadata,
        usize::try_from(location.line()).ok()?,
        usize::try_from(location.column()).ok()?,
    );
    Some(SourceSpan::from_range(
        source,
        metadata_offset + offset,
        metadata_offset + offset + 1,
    ))
}

fn span_from_line_column(source: &str, line: usize, column: usize) -> SourceSpan {
    let offset = offset_from_line_column(source, line, column);
    SourceSpan::from_range(source, offset, offset.saturating_add(1))
}

fn offset_from_line_column(source: &str, line: usize, column: usize) -> usize {
    let target_line = line.max(1);
    let mut line_start = 0;
    for _ in 1..target_line {
        let Some(relative) = source[line_start..].find('\n') else {
            return source.len();
        };
        line_start += relative + 1;
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |relative| line_start + relative);
    let line_source = &source[line_start..line_end];
    let target_column = column.saturating_sub(1);
    line_start
        + line_source
            .char_indices()
            .nth(target_column)
            .map_or(line_source.len(), |(offset, _)| offset)
}

fn detect_format(path: &Path) -> Option<ContentFormat> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "json" => Some(ContentFormat::Json),
        "toml" => Some(ContentFormat::Toml),
        "md" | "markdown" => Some(ContentFormat::Markdown),
        _ => None,
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn normalized_relative_path(path: &Path) -> Result<String, String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    format!("content path is not valid UTF-8: {}", path.display())
                })?;
                validate_portable_segment(value)?;
                components.push(value);
            }
            _ => {
                return Err(format!(
                    "content path is not a normalized relative path: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(components.join("/"))
}

fn validate_portable_segment(segment: &str) -> Result<(), String> {
    if !segment.is_ascii() {
        return Err(format!(
            "content path segment must be ASCII for portable IDs: {segment:?}"
        ));
    }
    if segment.ends_with(['.', ' ']) {
        return Err(format!(
            "content path segment cannot end in a dot or space: {segment:?}"
        ));
    }
    if segment.bytes().any(|byte| {
        byte < 0x20
            || byte == 0x7f
            || matches!(
                byte,
                b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
            )
    }) {
        return Err(format!(
            "content path segment contains a non-portable character: {segment:?}"
        ));
    }
    let basename = segment.split('.').next().unwrap_or(segment);
    let uppercase = basename.to_ascii_uppercase();
    let reserved = matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_numbered_name(&uppercase, "COM")
        || reserved_numbered_name(&uppercase, "LPT");
    if reserved {
        return Err(format!(
            "content path segment uses a reserved Windows name: {segment:?}"
        ));
    }
    Ok(())
}

fn reserved_numbered_name(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

fn normalize_line_endings(source: &str) -> Cow<'_, str> {
    if source.contains("\r\n") {
        Cow::Owned(source.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(source)
    }
}

fn fingerprint(
    source: &SourceFile,
    options: LoadOptions,
    normalized_source: &str,
) -> ContentFingerprint {
    let mut digest = Sha256::new();
    for component in [
        FINGERPRINT_CONTRACT_VERSION,
        source.id.as_str(),
        source.relative_path.as_str(),
        content_format_name(source.format),
        markdown_policy_name(options.markdown_policy),
        normalized_source,
    ] {
        digest.update(
            u64::try_from(component.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        digest.update(component.as_bytes());
    }
    ContentFingerprint::new(format!("{:x}", digest.finalize()))
}

const fn content_format_name(format: ContentFormat) -> &'static str {
    match format {
        ContentFormat::Json => "json",
        ContentFormat::Toml => "toml",
        ContentFormat::Markdown => "markdown",
    }
}

const fn markdown_policy_name(policy: MarkdownPolicy) -> &'static str {
    match policy {
        MarkdownPolicy::Safe => "safe",
        MarkdownPolicy::Trusted => "trusted",
    }
}

fn source_diagnostic(
    source: &SourceFile,
    code: DiagnosticCode,
    format: Option<DiagnosticFormat>,
    span: Option<SourceSpan>,
    message: impl Into<String>,
) -> ContentDiagnostic {
    ContentDiagnostic::new(
        code,
        source.path.clone(),
        Some(source.relative_path.clone()),
        format,
        span,
        message,
    )
}

fn io_diagnostic_for_source(source: &SourceFile, error: std::io::Error) -> ContentDiagnostic {
    source_diagnostic(
        source,
        DiagnosticCode::Io,
        Some(source.format.diagnostic_format()),
        None,
        format!("I/O error: {error}"),
    )
}

fn io_diagnostic(root: &Path, path: &Path, error: std::io::Error) -> ContentDiagnostic {
    simple_path_diagnostic(
        root,
        path,
        DiagnosticCode::Io,
        format!("I/O error: {error}"),
    )
}

fn simple_path_diagnostic(
    root: &Path,
    path: &Path,
    code: DiagnosticCode,
    message: impl Into<String>,
) -> ContentDiagnostic {
    let relative_path = path
        .strip_prefix(root)
        .ok()
        .and_then(|relative| normalized_relative_path(relative).ok())
        .filter(|relative| !relative.is_empty());
    ContentDiagnostic::new(code, path.to_path_buf(), relative_path, None, None, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_split_preserves_normalized_body_and_offsets() {
        let source = "+++\ntitle = \"One\"\n+++\n# Body\n";
        let parts = split_frontmatter(source).expect("front matter");
        assert_eq!(parts.frontmatter, Frontmatter::Toml);
        assert_eq!(parts.metadata, "title = \"One\"\n");
        assert_eq!(parts.body, "# Body\n");
        assert_eq!(
            &source[parts.metadata_offset..],
            "title = \"One\"\n+++\n# Body\n"
        );
        assert_eq!(&source[parts.body_offset..], "# Body\n");
    }

    #[test]
    fn line_column_conversion_handles_unicode() {
        let source = "one\nméxico\nthree";
        assert_eq!(offset_from_line_column(source, 2, 2), 5);
        let span = span_from_line_column(source, 2, 2);
        assert_eq!((span.line, span.column), (2, 2));
    }

    #[test]
    fn extension_detection_is_ascii_case_insensitive() {
        assert_eq!(
            detect_format(Path::new("post.MD")),
            Some(ContentFormat::Markdown)
        );
        assert_eq!(
            detect_format(Path::new("post.JSON")),
            Some(ContentFormat::Json)
        );
        assert_eq!(detect_format(Path::new("post.txt")), None);
    }

    #[test]
    fn portable_segments_reject_reserved_and_ambiguous_names() {
        for rejected in [
            "café.md",
            "CON.md",
            "lpt9.toml",
            "bad?.md",
            "trail .md ",
            "trail.",
        ] {
            assert!(validate_portable_segment(rejected).is_err(), "{rejected}");
        }
        for accepted in ["post.md", "my post.toml", "com10.json"] {
            assert!(validate_portable_segment(accepted).is_ok(), "{accepted}");
        }
    }

    #[test]
    fn source_swap_to_symlink_is_rejected_by_confined_handle_open() {
        let nonce = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("pliego-content-root-{nonce}"));
        let outside = std::env::temp_dir().join(format!("pliego-content-outside-{nonce}.json"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("entry.json"), br#"{"title":"inside"}"#).unwrap();
        fs::write(&outside, br#"{"title":"outside"}"#).unwrap();
        let source = SourceFile::new(&root, root.join("entry.json"), ContentFormat::Json).unwrap();
        let confined = open_confined_root(&root).unwrap();
        fs::remove_file(root.join("entry.json")).unwrap();

        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("entry.json"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, root.join("entry.json"));
        if linked.is_ok() {
            let mut bytes_read = 0;
            assert!(
                read_source_bytes(&confined, &source, ContentLimits::new(), &mut bytes_read)
                    .is_err()
            );
        }

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }
}
