// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

const MAX_PORTABLE_PATH_BYTES: usize = 4_096;
const MAX_COMPONENT_BYTES: usize = 255;
const MAX_PATH_COMPONENTS: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeContractBinding {
    pub id: String,
    pub sha256: String,
}

impl RuntimeContractBinding {
    pub fn new(id: impl Into<String>, sha256: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            sha256: sha256.into(),
        }
    }
}

pub fn runtime_contract_sha256_v1(
    route_graph_sha256: &str,
    actions: &[RuntimeContractBinding],
    loaders: &[RuntimeContractBinding],
    caches: &[RuntimeContractBinding],
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-runtime-contract-v1\0");
    digest.update(route_graph_sha256.as_bytes());
    for entries in [actions, loaders, caches] {
        for entry in entries {
            digest.update((entry.id.len() as u64).to_be_bytes());
            digest.update(entry.id.as_bytes());
            digest.update(entry.sha256.as_bytes());
        }
    }
    encode_digest(&digest.finalize())
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    encode_digest(&digest)
}

fn encode_digest(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn validate_portable_path(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAX_PORTABLE_PATH_BYTES
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
    {
        return Err(format!("non-portable artifact path: {value}"));
    }
    let normalized: String = value.nfc().collect();
    if normalized.len() > MAX_PORTABLE_PATH_BYTES || normalized != value {
        return Err(format!("artifact path is not NFC-normalized: {value}"));
    }
    let components = normalized.split('/').collect::<Vec<_>>();
    if components.len() > MAX_PATH_COMPONENTS {
        return Err(format!("artifact path has too many components: {value}"));
    }
    for component in components {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.len() > MAX_COMPONENT_BYTES
            || component.ends_with(['.', ' '])
            || component.chars().any(|character| {
                character.is_control()
                    || matches!(character, '\0' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
            })
        {
            return Err(format!("non-portable artifact path component: {value}"));
        }
        let stem = component.split('.').next().unwrap_or(component);
        let reserved: String = stem.nfkc().case_fold().nfkc().collect();
        if matches!(reserved.as_str(), "con" | "prn" | "aux" | "nul")
            || reserved.strip_prefix("com").is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
            || reserved.strip_prefix("lpt").is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        {
            return Err(format!("reserved artifact path component: {value}"));
        }
    }
    Ok(normalized)
}
