// SPDX-License-Identifier: AGPL-3.0-only

use crate::{ConsoleStore, ConsoleStoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

const STATE_CONTRACT: &str = "dev.pliegors.hyphae-console-state/v1";
const MAX_STATE_BYTES: usize = 64 * 1024;
const MAX_ACTIVITY: usize = 16;

pub(super) type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(super) trait ConsoleStateStore: Send + Sync {
    fn get<'a>(
        &'a self,
        tenant_id: &'a str,
    ) -> StoreFuture<'a, Result<Option<Vec<u8>>, ConsoleStoreError>>;

    fn put<'a>(
        &'a self,
        tenant_id: &'a str,
        value: &'a [u8],
        idempotency_token: u128,
    ) -> StoreFuture<'a, Result<(), ConsoleStoreError>>;
}

impl ConsoleStateStore for ConsoleStore {
    fn get<'a>(
        &'a self,
        tenant_id: &'a str,
    ) -> StoreFuture<'a, Result<Option<Vec<u8>>, ConsoleStoreError>> {
        Box::pin(async move { self.get(tenant_id).await })
    }

    fn put<'a>(
        &'a self,
        tenant_id: &'a str,
        value: &'a [u8],
        idempotency_token: u128,
    ) -> StoreFuture<'a, Result<(), ConsoleStoreError>> {
        Box::pin(async move { self.put(tenant_id, value, idempotency_token).await })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConsoleState {
    contract: String,
    tenant_id: String,
    revision: u64,
    counter: u64,
    activity: Vec<ActivityEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ActivityEntry {
    revision: u64,
    actor: String,
    kind: String,
}

impl ConsoleState {
    pub(super) fn empty(tenant_id: &str) -> Result<Self, StateError> {
        validate_identifier(tenant_id)?;
        Ok(Self {
            contract: STATE_CONTRACT.to_owned(),
            tenant_id: tenant_id.to_owned(),
            revision: 0,
            counter: 0,
            activity: Vec::new(),
        })
    }

    pub(super) fn decode(tenant_id: &str, bytes: &[u8]) -> Result<Self, StateError> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StateError::Bound);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| StateError::Invalid)?;
        value.validate(tenant_id)?;
        Ok(value)
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, StateError> {
        self.validate(&self.tenant_id)?;
        let bytes = serde_json::to_vec(self).map_err(|_| StateError::Invalid)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StateError::Bound);
        }
        Ok(bytes)
    }

    pub(super) fn increment(&mut self, actor: &str) -> Result<(), StateError> {
        validate_identifier(actor)?;
        self.revision = self.revision.checked_add(1).ok_or(StateError::Overflow)?;
        self.counter = self.counter.checked_add(1).ok_or(StateError::Overflow)?;
        self.activity.push(ActivityEntry {
            revision: self.revision,
            actor: actor.to_owned(),
            kind: "counter-incremented".to_owned(),
        });
        if self.activity.len() > MAX_ACTIVITY {
            self.activity.remove(0);
        }
        Ok(())
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn counter(&self) -> u64 {
        self.counter
    }

    pub(super) fn activity(&self) -> &[ActivityEntry] {
        &self.activity
    }

    fn validate(&self, expected_tenant: &str) -> Result<(), StateError> {
        if self.contract != STATE_CONTRACT || self.tenant_id != expected_tenant {
            return Err(StateError::Invalid);
        }
        validate_identifier(&self.tenant_id)?;
        if self.activity.len() > MAX_ACTIVITY {
            return Err(StateError::Bound);
        }
        let mut previous = 0;
        for item in &self.activity {
            validate_identifier(&item.actor)?;
            if item.kind != "counter-incremented"
                || item.revision == 0
                || item.revision <= previous
                || item.revision > self.revision
            {
                return Err(StateError::Invalid);
            }
            previous = item.revision;
        }
        Ok(())
    }
}

impl ActivityEntry {
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn actor(&self) -> &str {
        &self.actor
    }

    pub(super) fn kind(&self) -> &str {
        &self.kind
    }
}

pub(super) fn mutation_identity(tenant_id: &str, state: &[u8]) -> u128 {
    let mut hash = Sha256::new();
    hash.update(b"dev.pliegors.hyphae-console-mutation/v1\0");
    hash.update((tenant_id.len() as u64).to_be_bytes());
    hash.update(tenant_id.as_bytes());
    hash.update((state.len() as u64).to_be_bytes());
    hash.update(state);
    let digest: [u8; 32] = hash.finalize().into();
    let value = u128::from_be_bytes(digest[..16].try_into().expect("SHA-256 has sixteen bytes"));
    value.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cross_tenant_and_unknown_state_fields() {
        let cross_tenant = br#"{"contract":"dev.pliegors.hyphae-console-state/v1","tenantId":"tenant-b","revision":0,"counter":0,"activity":[]}"#;
        assert_eq!(
            ConsoleState::decode("tenant-a", cross_tenant),
            Err(StateError::Invalid)
        );
        let unknown = br#"{"contract":"dev.pliegors.hyphae-console-state/v1","tenantId":"tenant-a","revision":0,"counter":0,"activity":[],"extra":true}"#;
        assert_eq!(
            ConsoleState::decode("tenant-a", unknown),
            Err(StateError::Invalid)
        );
    }

    #[test]
    fn mutation_identity_is_tenant_and_payload_bound() {
        let state = ConsoleState::empty("tenant-a").unwrap().encode().unwrap();
        assert_eq!(
            mutation_identity("tenant-a", &state),
            mutation_identity("tenant-a", &state)
        );
        assert_ne!(
            mutation_identity("tenant-a", &state),
            mutation_identity("tenant-b", &state)
        );
    }
}

fn validate_identifier(value: &str) -> Result<(), StateError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(StateError::Invalid);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StateError {
    Invalid,
    Bound,
    Overflow,
}
