// SPDX-License-Identifier: Apache-2.0

use crate::{ActionPolicy, CachePolicy, DataError, LoaderPolicy};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DataPolicyGrants {
    loaders: BTreeMap<String, String>,
    actions: BTreeMap<String, String>,
    caches: BTreeMap<String, String>,
}

impl DataPolicyGrants {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn loader(mut self, policy: &LoaderPolicy) -> Result<Self, DataError> {
        insert(
            &mut self.loaders,
            policy.id(),
            policy.contract_digest(),
            "loader",
        )?;
        Ok(self)
    }

    pub fn action(mut self, policy: &ActionPolicy) -> Result<Self, DataError> {
        insert(
            &mut self.actions,
            policy.id(),
            policy.contract_digest(),
            "action",
        )?;
        Ok(self)
    }

    pub fn cache(mut self, policy: &CachePolicy) -> Result<Self, DataError> {
        insert(
            &mut self.caches,
            policy.id(),
            policy.contract_digest(),
            "cache",
        )?;
        Ok(self)
    }

    pub(crate) fn permits_loader(&self, policy: &LoaderPolicy) -> bool {
        permits(&self.loaders, policy.id(), &policy.contract_digest())
    }

    pub(crate) fn permits_action(&self, policy: &ActionPolicy) -> bool {
        permits(&self.actions, policy.id(), &policy.contract_digest())
    }

    pub(crate) fn permits_cache(&self, policy: &CachePolicy) -> bool {
        permits(&self.caches, policy.id(), &policy.contract_digest())
    }
}

fn insert(
    grants: &mut BTreeMap<String, String>,
    id: &str,
    digest: String,
    kind: &str,
) -> Result<(), DataError> {
    if let Some(existing) = grants.get(id) {
        if existing == &digest {
            return Ok(());
        }
        return Err(DataError::PolicyNotGranted(format!(
            "conflicting {kind} policy {id}"
        )));
    }
    grants.insert(id.to_owned(), digest);
    Ok(())
}

fn permits(grants: &BTreeMap<String, String>, id: &str, digest: &str) -> bool {
    grants.get(id).is_some_and(|current| current == digest)
}
