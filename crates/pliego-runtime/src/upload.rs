// SPDX-License-Identifier: Apache-2.0

use crate::{Body, DataContext};
use multer::{Constraints, Multipart, SizeLimit};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MultipartFieldKind {
    Text,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultipartPolicy {
    temp_directory: PathBuf,
    fields: BTreeMap<String, MultipartFieldKind>,
    max_parts: usize,
    max_files: usize,
    max_text_bytes: usize,
    max_file_bytes: usize,
    max_total_bytes: usize,
    max_filename_bytes: usize,
}

impl MultipartPolicy {
    pub fn new(temp_directory: impl AsRef<Path>) -> Result<Self, UploadError> {
        let temp_directory = std::fs::canonicalize(temp_directory)
            .map_err(|_| UploadError::InvalidPolicy("temporary directory is unavailable"))?;
        if !temp_directory.is_absolute() || !temp_directory.is_dir() {
            return Err(UploadError::InvalidPolicy(
                "temporary directory must be an existing absolute directory",
            ));
        }
        Ok(Self {
            temp_directory,
            fields: BTreeMap::new(),
            max_parts: 32,
            max_files: 8,
            max_text_bytes: 64 * 1_024,
            max_file_bytes: 8 * 1_024 * 1_024,
            max_total_bytes: 16 * 1_024 * 1_024,
            max_filename_bytes: 255,
        })
    }

    pub fn allow_field(
        mut self,
        name: impl Into<String>,
        kind: MultipartFieldKind,
    ) -> Result<Self, UploadError> {
        let name = name.into();
        if !valid_field_name(&name) || self.fields.len() >= 64 {
            return Err(UploadError::InvalidPolicy("multipart field ID is invalid"));
        }
        if self.fields.insert(name, kind).is_some() {
            return Err(UploadError::InvalidPolicy(
                "multipart field ID is duplicated",
            ));
        }
        Ok(self)
    }

    pub fn limits(
        mut self,
        max_parts: usize,
        max_files: usize,
        max_text_bytes: usize,
        max_file_bytes: usize,
        max_total_bytes: usize,
        max_filename_bytes: usize,
    ) -> Result<Self, UploadError> {
        if max_parts == 0
            || max_parts > 256
            || max_files > max_parts
            || max_text_bytes == 0
            || max_text_bytes > 4 * 1_024 * 1_024
            || max_file_bytes == 0
            || max_file_bytes > 64 * 1_024 * 1_024
            || max_total_bytes == 0
            || max_total_bytes > 256 * 1_024 * 1_024
            || max_file_bytes > max_total_bytes
            || max_filename_bytes == 0
            || max_filename_bytes > 1_024
        {
            return Err(UploadError::InvalidPolicy("multipart limits are invalid"));
        }
        self.max_parts = max_parts;
        self.max_files = max_files;
        self.max_text_bytes = max_text_bytes;
        self.max_file_bytes = max_file_bytes;
        self.max_total_bytes = max_total_bytes;
        self.max_filename_bytes = max_filename_bytes;
        Ok(self)
    }

    pub fn max_total_bytes(&self) -> usize {
        self.max_total_bytes
    }
}

pub struct MultipartForm {
    fields: BTreeMap<String, Vec<MultipartPart>>,
    decoded_bytes: usize,
}

impl MultipartForm {
    pub fn values(&self, name: &str) -> Option<&[MultipartPart]> {
        self.fields.get(name).map(Vec::as_slice)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.fields.get(name)?.as_slice() {
            [MultipartPart::Text(value)] => Some(value),
            _ => None,
        }
    }

    pub fn file(&self, name: &str) -> Option<&UploadFile> {
        match self.fields.get(name)?.as_slice() {
            [MultipartPart::File(value)] => Some(value),
            _ => None,
        }
    }

    pub fn decoded_bytes(&self) -> usize {
        self.decoded_bytes
    }
}

impl Debug for MultipartForm {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MultipartForm")
            .field("field_names", &self.fields.keys().collect::<Vec<_>>())
            .field(
                "part_count",
                &self.fields.values().map(Vec::len).sum::<usize>(),
            )
            .field("decoded_bytes", &self.decoded_bytes)
            .finish()
    }
}

pub enum MultipartPart {
    Text(String),
    File(UploadFile),
}

impl Debug for MultipartPart {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(_) => formatter.write_str("Text([REDACTED])"),
            Self::File(file) => Debug::fmt(file, formatter),
        }
    }
}

pub struct UploadFile {
    original_filename: Option<String>,
    content_type: Option<String>,
    size: usize,
    artifact: Arc<TempArtifact>,
    context: DataContext,
}

impl UploadFile {
    pub fn original_filename(&self) -> Option<&str> {
        self.original_filename.as_deref()
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub async fn read(&self, maximum: usize) -> Result<Vec<u8>, UploadError> {
        if maximum == 0 || self.size > maximum {
            return Err(UploadError::FileLimit);
        }
        if self.context.is_closed() || self.artifact.deleted.load(Ordering::Acquire) {
            return Err(UploadError::ContextClosed);
        }
        if self.context.cancellation().is_cancelled() {
            return Err(UploadError::Cancelled);
        }
        let read = tokio::fs::read(&self.artifact.path);
        let bytes = tokio::select! {
            biased;
            _ = self.context.cancellation().cancelled() => return Err(UploadError::Cancelled),
            result = read => result.map_err(|_| UploadError::StorageFailure)?,
        };
        if bytes.len() != self.size || bytes.len() > maximum {
            return Err(UploadError::StorageFailure);
        }
        Ok(bytes)
    }
}

impl Debug for UploadFile {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UploadFile")
            .field(
                "filename",
                &self.original_filename.as_ref().map(|_| "[REDACTED]"),
            )
            .field("content_type", &self.content_type)
            .field("size", &self.size)
            .field("available", &!self.artifact.deleted.load(Ordering::Acquire))
            .finish()
    }
}

struct TempArtifact {
    path: PathBuf,
    deleted: AtomicBool,
}

impl TempArtifact {
    fn delete(&self) {
        if !self.deleted.swap(true, Ordering::AcqRel) {
            match std::fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {}
            }
        }
    }
}

impl Drop for TempArtifact {
    fn drop(&mut self) {
        self.delete();
    }
}

pub(crate) async fn parse_multipart(
    context: &DataContext,
    body: Body,
    content_type: &str,
    policy: &MultipartPolicy,
    maximum: usize,
) -> Result<MultipartForm, UploadError> {
    if policy.fields.is_empty() || maximum == 0 {
        return Err(UploadError::InvalidPolicy(
            "multipart policy has no admitted fields or byte budget",
        ));
    }
    let total_limit = policy.max_total_bytes.min(maximum);
    let boundary =
        multer::parse_boundary(content_type).map_err(|_| UploadError::InvalidBoundary)?;
    let constraints = Constraints::new().size_limit(
        SizeLimit::new()
            .whole_stream(total_limit as u64)
            .per_field(policy.max_file_bytes.max(policy.max_text_bytes) as u64),
    );
    let mut multipart = Multipart::with_constraints(body.into_data_stream(), boundary, constraints);
    let mut fields = BTreeMap::<String, Vec<MultipartPart>>::new();
    let mut part_count = 0usize;
    let mut file_count = 0usize;
    let mut decoded_bytes = 0usize;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| UploadError::Malformed)?
    {
        part_count += 1;
        if part_count > policy.max_parts {
            return Err(UploadError::PartLimit);
        }
        if context.is_closed() {
            return Err(UploadError::ContextClosed);
        }
        if context.cancellation().is_cancelled() {
            return Err(UploadError::Cancelled);
        }
        let name = field
            .name()
            .ok_or(UploadError::MissingFieldName)?
            .to_owned();
        let kind = policy
            .fields
            .get(&name)
            .copied()
            .ok_or(UploadError::UnknownField)?;
        let original_filename = admitted_filename(field.file_name(), policy.max_filename_bytes)?;
        let content_type = field.content_type().map(ToString::to_string);
        if content_type
            .as_ref()
            .is_some_and(|value| value.len() > 256 || value.chars().any(char::is_control))
        {
            return Err(UploadError::Malformed);
        }

        let part = match kind {
            MultipartFieldKind::Text => {
                if original_filename.is_some() {
                    return Err(UploadError::FieldKindMismatch);
                }
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(|_| UploadError::Malformed)? {
                    decoded_bytes = decoded_bytes
                        .checked_add(chunk.len())
                        .ok_or(UploadError::TotalLimit)?;
                    if decoded_bytes > total_limit
                        || bytes.len().saturating_add(chunk.len()) > policy.max_text_bytes
                    {
                        return Err(UploadError::TextLimit);
                    }
                    bytes.extend_from_slice(&chunk);
                }
                MultipartPart::Text(String::from_utf8(bytes).map_err(|_| UploadError::InvalidText)?)
            }
            MultipartFieldKind::File => {
                file_count += 1;
                if file_count > policy.max_files || original_filename.is_none() {
                    return Err(UploadError::FileLimit);
                }
                let mut temporary = tempfile::Builder::new()
                    .prefix("pliego-upload-")
                    .tempfile_in(&policy.temp_directory)
                    .map_err(|_| UploadError::StorageFailure)?;
                let mut size = 0usize;
                while let Some(chunk) = field.chunk().await.map_err(|_| UploadError::Malformed)? {
                    size = size
                        .checked_add(chunk.len())
                        .ok_or(UploadError::FileLimit)?;
                    decoded_bytes = decoded_bytes
                        .checked_add(chunk.len())
                        .ok_or(UploadError::TotalLimit)?;
                    if size > policy.max_file_bytes || decoded_bytes > total_limit {
                        return Err(UploadError::FileLimit);
                    }
                    temporary
                        .as_file_mut()
                        .write_all(&chunk)
                        .map_err(|_| UploadError::StorageFailure)?;
                }
                temporary
                    .as_file_mut()
                    .flush()
                    .map_err(|_| UploadError::StorageFailure)?;
                let path = temporary
                    .into_temp_path()
                    .keep()
                    .map_err(|_| UploadError::StorageFailure)?;
                let artifact = Arc::new(TempArtifact {
                    path,
                    deleted: AtomicBool::new(false),
                });
                let cleanup_artifact = artifact.clone();
                context
                    .register_cleanup(move |_| {
                        cleanup_artifact.delete();
                        Ok(())
                    })
                    .map_err(|_| UploadError::CleanupRegistration)?;
                MultipartPart::File(UploadFile {
                    original_filename,
                    content_type,
                    size,
                    artifact,
                    context: context.clone(),
                })
            }
        };
        fields.entry(name).or_default().push(part);
    }
    Ok(MultipartForm {
        fields,
        decoded_bytes,
    })
}

fn admitted_filename(value: Option<&str>, maximum: usize) -> Result<Option<String>, UploadError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty()
        || value.len() > maximum
        || value
            .chars()
            .any(|character| matches!(character, '\0' | '\r' | '\n'))
    {
        return Err(UploadError::InvalidFilename);
    }
    Ok(Some(value.to_owned()))
}

fn valid_field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadError {
    InvalidPolicy(&'static str),
    InvalidBoundary,
    Malformed,
    MissingFieldName,
    UnknownField,
    FieldKindMismatch,
    InvalidFilename,
    InvalidText,
    PartLimit,
    FileLimit,
    TextLimit,
    TotalLimit,
    StorageFailure,
    CleanupRegistration,
    ContextClosed,
    Cancelled,
}

impl UploadError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy(_) => "PLG-UPL-001",
            Self::InvalidBoundary | Self::Malformed | Self::MissingFieldName => "PLG-UPL-101",
            Self::UnknownField | Self::FieldKindMismatch => "PLG-UPL-102",
            Self::InvalidFilename | Self::InvalidText => "PLG-UPL-103",
            Self::PartLimit | Self::FileLimit | Self::TextLimit | Self::TotalLimit => "PLG-UPL-104",
            Self::StorageFailure | Self::CleanupRegistration => "PLG-UPL-500",
            Self::ContextClosed => "PLG-UPL-410",
            Self::Cancelled => "PLG-UPL-408",
        }
    }
}

impl Display for UploadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPolicy(message) => message,
            Self::InvalidBoundary => "multipart boundary is missing or invalid",
            Self::Malformed => "multipart body is malformed",
            Self::MissingFieldName => "multipart part has no field name",
            Self::UnknownField => "multipart field is not declared",
            Self::FieldKindMismatch => "multipart field kind does not match its policy",
            Self::InvalidFilename => "multipart filename is invalid",
            Self::InvalidText => "multipart text field is not UTF-8",
            Self::PartLimit => "multipart part limit was exceeded",
            Self::FileLimit => "multipart file limit was exceeded",
            Self::TextLimit => "multipart text limit was exceeded",
            Self::TotalLimit => "multipart total storage limit was exceeded",
            Self::StorageFailure => "multipart temporary storage failed",
            Self::CleanupRegistration => "multipart cleanup could not be registered",
            Self::ContextClosed => "multipart request context is closed",
            Self::Cancelled => "multipart parsing was cancelled",
        })
    }
}

impl std::error::Error for UploadError {}
