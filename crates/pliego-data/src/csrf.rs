// SPDX-License-Identifier: Apache-2.0

use crate::{SecretHandle, SessionToken, validate_stable_id};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Eq, PartialEq)]
pub struct CsrfToken {
    key_version: u32,
    mac: Vec<u8>,
}

impl CsrfToken {
    pub fn parse(value: &str) -> Result<Self, CsrfError> {
        let Some((version, encoded)) = value.split_once('.') else {
            return Err(CsrfError::InvalidToken);
        };
        let Some(version) = version.strip_prefix('v') else {
            return Err(CsrfError::InvalidToken);
        };
        let key_version = version
            .parse::<u32>()
            .map_err(|_| CsrfError::InvalidToken)?;
        if key_version == 0 {
            return Err(CsrfError::InvalidToken);
        }
        let mac = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| CsrfError::InvalidToken)?;
        if mac.len() != 32 {
            return Err(CsrfError::InvalidToken);
        }
        Ok(Self { key_version, mac })
    }

    pub fn as_form_value(&self) -> String {
        format!(
            "v{}.{}",
            self.key_version,
            URL_SAFE_NO_PAD.encode(&self.mac)
        )
    }
}

impl Debug for CsrfToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CsrfToken")
            .field("key_version", &self.key_version)
            .field("mac", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct CsrfManager {
    active_version: u32,
    keys: BTreeMap<u32, SecretHandle>,
}

impl CsrfManager {
    pub fn new(
        active: SecretHandle,
        predecessors: impl IntoIterator<Item = SecretHandle>,
    ) -> Result<Self, CsrfError> {
        let active_version = active.version();
        let mut keys = BTreeMap::from([(active.version(), active)]);
        for key in predecessors {
            if keys.insert(key.version(), key).is_some() {
                return Err(CsrfError::DuplicateKeyVersion);
            }
        }
        if keys.len() > 4 {
            return Err(CsrfError::TooManyKeys);
        }
        Ok(Self {
            active_version,
            keys,
        })
    }

    pub fn issue(
        &self,
        session: &SessionToken,
        action_id: &str,
        action_revision: u32,
    ) -> Result<CsrfToken, CsrfError> {
        validate_action(action_id, action_revision)?;
        let key = self
            .keys
            .get(&self.active_version)
            .ok_or(CsrfError::UnknownKeyVersion)?;
        let message = message(session, action_id, action_revision);
        let mac = key.with_bytes(|bytes| {
            let mut mac = HmacSha256::new_from_slice(bytes)
                .expect("HMAC accepts keys of every validated length");
            mac.update(&message);
            mac.finalize().into_bytes().to_vec()
        });
        Ok(CsrfToken {
            key_version: self.active_version,
            mac,
        })
    }

    pub fn verify(
        &self,
        token: &CsrfToken,
        session: &SessionToken,
        action_id: &str,
        action_revision: u32,
    ) -> Result<bool, CsrfError> {
        validate_action(action_id, action_revision)?;
        let Some(key) = self.keys.get(&token.key_version) else {
            return Ok(false);
        };
        let message = message(session, action_id, action_revision);
        Ok(key.with_bytes(|bytes| {
            let mut mac = HmacSha256::new_from_slice(bytes)
                .expect("HMAC accepts keys of every validated length");
            mac.update(&message);
            mac.verify_slice(&token.mac).is_ok()
        }))
    }
}

impl Debug for CsrfManager {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CsrfManager")
            .field("active_version", &self.active_version)
            .field("readable_versions", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn validate_action(action_id: &str, action_revision: u32) -> Result<(), CsrfError> {
    if !validate_stable_id(action_id) || action_revision == 0 {
        return Err(CsrfError::InvalidAction);
    }
    Ok(())
}

fn message(session: &SessionToken, action_id: &str, action_revision: u32) -> Vec<u8> {
    let mut message = Vec::new();
    for value in [
        b"pliego-csrf-v1".as_slice(),
        session.digest().as_bytes(),
        action_id.as_bytes(),
        &action_revision.to_be_bytes(),
    ] {
        message.extend((value.len() as u64).to_be_bytes());
        message.extend(value);
    }
    message
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CsrfError {
    InvalidToken,
    InvalidAction,
    DuplicateKeyVersion,
    TooManyKeys,
    UnknownKeyVersion,
}

impl CsrfError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidToken | Self::InvalidAction => "PLG-CSRF-101",
            Self::DuplicateKeyVersion | Self::TooManyKeys => "PLG-CSRF-001",
            Self::UnknownKeyVersion => "PLG-CSRF-102",
        }
    }
}

impl Display for CsrfError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken => formatter.write_str("invalid CSRF token"),
            Self::InvalidAction => formatter.write_str("invalid CSRF action identity"),
            Self::DuplicateKeyVersion => formatter.write_str("duplicate CSRF key version"),
            Self::TooManyKeys => formatter.write_str("CSRF key ring exceeds four versions"),
            Self::UnknownKeyVersion => formatter.write_str("unknown CSRF key version"),
        }
    }
}

impl std::error::Error for CsrfError {}
