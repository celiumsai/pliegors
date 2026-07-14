use std::fmt;
use std::path::{Path, PathBuf};

/// Stable category for a content-loading diagnostic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum DiagnosticCode {
    Io,
    RootNotDirectory,
    SymlinkRejected,
    UnsupportedFileType,
    InvalidPath,
    InvalidUtf8,
    BomRejected,
    MissingFrontmatter,
    UnterminatedFrontmatter,
    Deserialize,
    IdCollision,
    UnsafeMarkdown,
    DepthLimitExceeded,
    EntryLimitExceeded,
    FileByteLimitExceeded,
    TotalByteLimitExceeded,
}

impl DiagnosticCode {
    /// Machine-readable code suitable for CLI and CI output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Io => "content.io",
            Self::RootNotDirectory => "content.root_not_directory",
            Self::SymlinkRejected => "content.symlink_rejected",
            Self::UnsupportedFileType => "content.unsupported_file_type",
            Self::InvalidPath => "content.invalid_path",
            Self::InvalidUtf8 => "content.invalid_utf8",
            Self::BomRejected => "content.bom_rejected",
            Self::MissingFrontmatter => "content.missing_frontmatter",
            Self::UnterminatedFrontmatter => "content.unterminated_frontmatter",
            Self::Deserialize => "content.deserialize",
            Self::IdCollision => "content.id_collision",
            Self::UnsafeMarkdown => "content.unsafe_markdown",
            Self::DepthLimitExceeded => "content.limit.depth",
            Self::EntryLimitExceeded => "content.limit.entries",
            Self::FileByteLimitExceeded => "content.limit.file_bytes",
            Self::TotalByteLimitExceeded => "content.limit.total_bytes",
        }
    }

    pub(crate) const fn is_resource_limit(self) -> bool {
        matches!(
            self,
            Self::DepthLimitExceeded
                | Self::EntryLimitExceeded
                | Self::FileByteLimitExceeded
                | Self::TotalByteLimitExceeded
        )
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The parser or document layer responsible for a diagnostic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum DiagnosticFormat {
    Json,
    Toml,
    Markdown,
    YamlFrontmatter,
    TomlFrontmatter,
}

impl DiagnosticFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Markdown => "markdown",
            Self::YamlFrontmatter => "yaml-frontmatter",
            Self::TomlFrontmatter => "toml-frontmatter",
        }
    }
}

impl fmt::Display for DiagnosticFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A byte span plus human-readable, one-indexed source coordinates.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceSpan {
    pub offset: usize,
    pub length: usize,
    pub line: usize,
    pub column: usize,
}

impl SourceSpan {
    pub(crate) fn from_range(source: &str, start: usize, end: usize) -> Self {
        let offset = start.min(source.len());
        let end = end.max(offset).min(source.len());
        let prefix = &source[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
        let column = source[line_start..offset].chars().count() + 1;
        Self {
            offset,
            length: end - offset,
            line,
            column,
        }
    }
}

/// One actionable problem found while discovering or parsing a collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentDiagnostic {
    code: DiagnosticCode,
    path: PathBuf,
    relative_path: Option<String>,
    related_path: Option<PathBuf>,
    format: Option<DiagnosticFormat>,
    span: Option<SourceSpan>,
    message: String,
}

impl ContentDiagnostic {
    pub(crate) fn new(
        code: DiagnosticCode,
        path: PathBuf,
        relative_path: Option<String>,
        format: Option<DiagnosticFormat>,
        span: Option<SourceSpan>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            path,
            relative_path,
            related_path: None,
            format,
            span,
            message: message.into(),
        }
    }

    pub(crate) fn with_related_path(mut self, path: PathBuf) -> Self {
        self.related_path = Some(path);
        self
    }

    pub const fn code(&self) -> DiagnosticCode {
        self.code
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Portable, slash-separated path relative to the collection root.
    pub fn relative_path(&self) -> Option<&str> {
        self.relative_path.as_deref()
    }

    pub fn related_path(&self) -> Option<&Path> {
        self.related_path.as_deref()
    }

    pub const fn format(&self) -> Option<DiagnosticFormat> {
        self.format
    }

    pub const fn span(&self) -> Option<SourceSpan> {
        self.span
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    fn sort_key(&self) -> (&str, &Path, DiagnosticCode, Option<SourceSpan>) {
        (
            self.relative_path.as_deref().unwrap_or(""),
            &self.path,
            self.code,
            self.span,
        )
    }
}

impl fmt::Display for ContentDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.code)?;
        if let Some(relative_path) = &self.relative_path {
            formatter.write_str(relative_path)?;
        } else {
            write!(formatter, "{}", self.path.display())?;
        }
        if let Some(format) = self.format {
            write!(formatter, " [{format}]")?;
        }
        if let Some(span) = self.span {
            write!(formatter, ":{}:{}", span.line, span.column)?;
        }
        write!(formatter, ": {}", self.message)
    }
}

/// Deterministically ordered diagnostics from one collection load.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadError {
    diagnostics: Vec<ContentDiagnostic>,
}

impl LoadError {
    pub(crate) fn new(mut diagnostics: Vec<ContentDiagnostic>) -> Self {
        diagnostics.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
        Self { diagnostics }
    }

    pub fn diagnostics(&self) -> &[ContentDiagnostic] {
        &self.diagnostics
    }

    pub fn into_diagnostics(self) -> Vec<ContentDiagnostic> {
        self.diagnostics
    }

    /// Stable text form useful for CI snapshots.
    pub fn snapshot(&self) -> String {
        let mut output = self
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        output.push('\n');
        output
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.diagnostics.len() == 1 {
            return self.diagnostics[0].fmt(formatter);
        }
        writeln!(formatter, "{} content diagnostics:", self.diagnostics.len())?;
        for diagnostic in &self.diagnostics {
            writeln!(formatter, "- {diagnostic}")?;
        }
        Ok(())
    }
}

impl std::error::Error for LoadError {}
