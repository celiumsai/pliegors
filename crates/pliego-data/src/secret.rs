// SPDX-License-Identifier: Apache-2.0

use crate::validate_stable_id;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;
use zeroize::Zeroize;

struct SecretMaterial(Vec<u8>);

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone)]
pub struct SecretHandle {
    id: String,
    version: u32,
    material: Arc<SecretMaterial>,
}

impl SecretHandle {
    pub fn new(
        id: impl Into<String>,
        version: u32,
        material: impl Into<Vec<u8>>,
    ) -> Result<Self, SecretError> {
        let id = id.into();
        let material = material.into();
        if !validate_stable_id(&id) || version == 0 {
            return Err(SecretError::InvalidIdentity);
        }
        if material.len() < 32 || material.len() > 4 * 1_024 {
            return Err(SecretError::InvalidLength {
                actual: material.len(),
                minimum: 32,
                maximum: 4 * 1_024,
            });
        }
        Ok(Self {
            id,
            version,
            material: Arc::new(SecretMaterial(material)),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn length_class(&self) -> &'static str {
        match self.material.0.len() {
            0..=63 => "short",
            64..=255 => "medium",
            _ => "long",
        }
    }

    pub fn with_bytes<Result>(&self, operation: impl FnOnce(&[u8]) -> Result) -> Result {
        operation(&self.material.0)
    }
}

impl Debug for SecretHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretHandle")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("length_class", &self.length_class())
            .field("material", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecretError {
    InvalidIdentity,
    InvalidLength {
        actual: usize,
        minimum: usize,
        maximum: usize,
    },
}

impl SecretError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentity => "PLG-SEC-001",
            Self::InvalidLength { .. } => "PLG-SEC-002",
        }
    }
}

impl Display for SecretError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentity => formatter.write_str("invalid secret identity"),
            Self::InvalidLength {
                actual,
                minimum,
                maximum,
            } => write!(
                formatter,
                "secret has {actual} bytes; required range is {minimum}..={maximum}"
            ),
        }
    }
}

impl std::error::Error for SecretError {}
