// SPDX-License-Identifier: Apache-2.0

use http::request::Parts;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestLimits {
    pub max_request_target_bytes: usize,
    pub max_header_count: usize,
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub max_response_bytes: usize,
    pub max_diagnostics: usize,
    pub max_cleanups: usize,
    pub max_concurrent_requests: usize,
    pub deadline_ms: u64,
    pub graceful_shutdown_ms: u64,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            max_request_target_bytes: 8 * 1_024,
            max_header_count: 100,
            max_header_bytes: 64 * 1_024,
            max_body_bytes: 8 * 1_024 * 1_024,
            max_response_bytes: 16 * 1_024 * 1_024,
            max_diagnostics: 64,
            max_cleanups: 64,
            max_concurrent_requests: 1_024,
            deadline_ms: 30_000,
            graceful_shutdown_ms: 30_000,
        }
    }
}

impl RequestLimits {
    pub fn validate(&self) -> Result<(), LimitError> {
        for (name, value, maximum) in [
            (
                "max_request_target_bytes",
                self.max_request_target_bytes,
                64 * 1_024,
            ),
            ("max_header_count", self.max_header_count, 1_024),
            ("max_header_bytes", self.max_header_bytes, 1024 * 1_024),
            ("max_body_bytes", self.max_body_bytes, 1024 * 1024 * 1024),
            (
                "max_response_bytes",
                self.max_response_bytes,
                1024 * 1024 * 1024,
            ),
            ("max_diagnostics", self.max_diagnostics, 1_024),
            ("max_cleanups", self.max_cleanups, 4_096),
            (
                "max_concurrent_requests",
                self.max_concurrent_requests,
                65_536,
            ),
        ] {
            if value == 0 || value > maximum {
                return Err(LimitError::InvalidPolicy {
                    name,
                    value,
                    maximum,
                });
            }
        }
        if self.deadline_ms == 0 || self.deadline_ms > 24 * 60 * 60 * 1_000 {
            return Err(LimitError::InvalidPolicy {
                name: "deadline_ms",
                value: self.deadline_ms as usize,
                maximum: (24 * 60 * 60 * 1_000) as usize,
            });
        }
        if self.graceful_shutdown_ms == 0 || self.graceful_shutdown_ms > 60 * 60 * 1_000 {
            return Err(LimitError::InvalidPolicy {
                name: "graceful_shutdown_ms",
                value: self.graceful_shutdown_ms as usize,
                maximum: (60 * 60 * 1_000) as usize,
            });
        }
        Ok(())
    }

    pub fn deadline(&self) -> Duration {
        Duration::from_millis(self.deadline_ms)
    }

    pub fn graceful_shutdown_deadline(&self) -> Duration {
        Duration::from_millis(self.graceful_shutdown_ms)
    }

    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("RequestLimits serialization is infallible");
        let digest = Sha256::digest(bytes);
        encode_hex(&digest)
    }

    pub(crate) fn admit_head(&self, parts: &Parts) -> Result<(), LimitError> {
        if parts.headers.contains_key(http::header::CONTENT_LENGTH)
            && parts.headers.contains_key(http::header::TRANSFER_ENCODING)
        {
            return Err(LimitError::AmbiguousBodyLength);
        }
        let target_bytes = parts.uri.to_string().len();
        if target_bytes > self.max_request_target_bytes {
            return Err(LimitError::RequestTarget {
                actual: target_bytes,
                maximum: self.max_request_target_bytes,
            });
        }
        if parts.headers.len() > self.max_header_count {
            return Err(LimitError::HeaderCount {
                actual: parts.headers.len(),
                maximum: self.max_header_count,
            });
        }
        let header_bytes = parts
            .headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>();
        if header_bytes > self.max_header_bytes {
            return Err(LimitError::HeaderBytes {
                actual: header_bytes,
                maximum: self.max_header_bytes,
            });
        }
        if let Some(value) = parts.headers.get(http::header::CONTENT_LENGTH) {
            let value = value
                .to_str()
                .map_err(|_| LimitError::InvalidContentLength)?;
            let length = value
                .parse::<u64>()
                .map_err(|_| LimitError::InvalidContentLength)?;
            if length > self.max_body_bytes as u64 {
                return Err(LimitError::BodyBytes {
                    actual: length,
                    maximum: self.max_body_bytes as u64,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn admit_body_format(&self, parts: &Parts) -> Result<(), LimitError> {
        if parts.headers.contains_key(http::header::CONTENT_ENCODING) {
            let mut saw_encoding = false;
            for value in parts.headers.get_all(http::header::CONTENT_ENCODING) {
                let value = value
                    .to_str()
                    .map_err(|_| LimitError::UnsupportedContentEncoding)?;
                for encoding in value.split(',').map(str::trim) {
                    saw_encoding = true;
                    if encoding.is_empty() || !encoding.eq_ignore_ascii_case("identity") {
                        return Err(LimitError::UnsupportedContentEncoding);
                    }
                }
            }
            if !saw_encoding {
                return Err(LimitError::UnsupportedContentEncoding);
            }
        }
        if let Some(value) = parts.headers.get(http::header::CONTENT_TYPE) {
            if let Ok(value) = value.to_str() {
                let media_type = value.split(';').next().unwrap_or_default().trim();
                if media_type
                    .get(..10)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("multipart/"))
                {
                    return Err(LimitError::UnsupportedMultipart);
                }
            }
        }
        Ok(())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LimitError {
    InvalidPolicy {
        name: &'static str,
        value: usize,
        maximum: usize,
    },
    RequestTarget {
        actual: usize,
        maximum: usize,
    },
    HeaderCount {
        actual: usize,
        maximum: usize,
    },
    HeaderBytes {
        actual: usize,
        maximum: usize,
    },
    InvalidContentLength,
    BodyBytes {
        actual: u64,
        maximum: u64,
    },
    UnsupportedContentEncoding,
    UnsupportedMultipart,
    AmbiguousBodyLength,
}

impl LimitError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy { .. } => "PLG-RUN-001",
            Self::RequestTarget { .. } => "PLG-RUN-101",
            Self::HeaderCount { .. } | Self::HeaderBytes { .. } => "PLG-RUN-102",
            Self::InvalidContentLength => "PLG-RUN-103",
            Self::BodyBytes { .. } => "PLG-RUN-104",
            Self::UnsupportedContentEncoding => "PLG-RUN-108",
            Self::UnsupportedMultipart => "PLG-RUN-109",
            Self::AmbiguousBodyLength => "PLG-RUN-110",
        }
    }
}

impl Display for LimitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy {
                name,
                value,
                maximum,
            } => {
                write!(formatter, "invalid {name} {value}; maximum is {maximum}")
            }
            Self::RequestTarget { actual, maximum } => {
                write!(
                    formatter,
                    "request target has {actual} bytes; maximum is {maximum}"
                )
            }
            Self::HeaderCount { actual, maximum } => {
                write!(
                    formatter,
                    "request has {actual} headers; maximum is {maximum}"
                )
            }
            Self::HeaderBytes { actual, maximum } => {
                write!(
                    formatter,
                    "request headers have {actual} bytes; maximum is {maximum}"
                )
            }
            Self::InvalidContentLength => formatter.write_str("invalid Content-Length header"),
            Self::BodyBytes { actual, maximum } => {
                write!(
                    formatter,
                    "request body declares {actual} bytes; maximum is {maximum}"
                )
            }
            Self::UnsupportedContentEncoding => formatter
                .write_str("request content encoding is unsupported without a decoded-byte budget"),
            Self::UnsupportedMultipart => formatter.write_str(
                "multipart requests are unsupported without bounded part and storage policies",
            ),
            Self::AmbiguousBodyLength => {
                formatter.write_str("request contains conflicting body framing headers")
            }
        }
    }
}

impl std::error::Error for LimitError {}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    #[test]
    fn default_policy_is_valid_and_digest_is_stable() {
        let limits = RequestLimits::default();
        limits.validate().unwrap();
        assert_eq!(limits.digest(), limits.clone().digest());
        assert_eq!(limits.digest().len(), 64);
    }

    #[test]
    fn rejects_zero_and_unbounded_policy_values() {
        let limits = RequestLimits {
            max_header_count: 0,
            ..RequestLimits::default()
        };
        assert!(matches!(
            limits.validate(),
            Err(LimitError::InvalidPolicy { .. })
        ));
        let limits = RequestLimits {
            deadline_ms: 24 * 60 * 60 * 1_000 + 1,
            ..RequestLimits::default()
        };
        assert!(matches!(
            limits.validate(),
            Err(LimitError::InvalidPolicy { .. })
        ));
    }

    #[test]
    fn preflights_content_length_before_body_admission() {
        let request = Request::builder()
            .uri("/upload")
            .header("content-length", "9000000")
            .body(())
            .unwrap();
        let (parts, _) = request.into_parts();
        assert!(matches!(
            RequestLimits::default().admit_head(&parts),
            Err(LimitError::BodyBytes { .. })
        ));
    }

    #[test]
    fn rejects_implicit_decompression_and_multipart_parsing() {
        for (name, value, expected) in [
            (
                "content-encoding",
                "gzip",
                LimitError::UnsupportedContentEncoding,
            ),
            (
                "content-type",
                "multipart/form-data; boundary=bounded",
                LimitError::UnsupportedMultipart,
            ),
        ] {
            let request = Request::builder()
                .uri("/upload")
                .header(name, value)
                .body(())
                .unwrap();
            let (parts, _) = request.into_parts();
            assert_eq!(
                RequestLimits::default().admit_body_format(&parts),
                Err(expected)
            );
        }
        let request = Request::builder()
            .uri("/upload")
            .header("content-encoding", "identity")
            .header("content-type", "application/octet-stream")
            .body(())
            .unwrap();
        let (parts, _) = request.into_parts();
        RequestLimits::default().admit_body_format(&parts).unwrap();
    }
}
