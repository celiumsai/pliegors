// SPDX-License-Identifier: AGPL-3.0-only

use crate::wire::WireError;

const ROOT_PREFIX: &[u8] = b"pliego/";

pub(crate) fn tenant_prefix(tenant: &str) -> Result<Vec<u8>, WireError> {
    if tenant.is_empty()
        || tenant.len() > 64
        || !tenant
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(WireError::Malformed);
    }
    let mut key = Vec::with_capacity(ROOT_PREFIX.len() + tenant.len() + 1);
    key.extend_from_slice(ROOT_PREFIX);
    key.extend_from_slice(tenant.as_bytes());
    key.push(b'/');
    Ok(key)
}
