// SPDX-License-Identifier: Apache-2.0

use crate::context::ContextState;
use crate::{DataCancellation, DataError, validate_stable_id};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeSet<String>);

impl CapabilitySet {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn allowing(mut self, capability: impl Into<String>) -> Result<Self, DataError> {
        let capability = capability.into();
        if !validate_stable_id(&capability) {
            return Err(DataError::InvalidStableId(capability));
        }
        self.0.insert(capability);
        Ok(self)
    }

    pub fn allows(&self, capability: &str) -> bool {
        self.0.contains(capability)
    }

    pub fn is_superset(&self, required: &Self) -> bool {
        self.0.is_superset(&required.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

impl Debug for CapabilitySet {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.debug_set().entries(self.0.iter()).finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceSpec {
    id: String,
    provider_id: String,
    capabilities: CapabilitySet,
    max_lease_ms: Option<u64>,
    description: String,
}

impl ResourceSpec {
    pub fn new(id: impl Into<String>, provider_id: impl Into<String>) -> Result<Self, DataError> {
        let id = id.into();
        let provider_id = provider_id.into();
        for value in [&id, &provider_id] {
            if !validate_stable_id(value) {
                return Err(DataError::InvalidStableId(value.clone()));
            }
        }
        Ok(Self {
            id,
            provider_id,
            capabilities: CapabilitySet::none(),
            max_lease_ms: None,
            description: String::new(),
        })
    }

    pub fn with_capabilities(mut self, capabilities: CapabilitySet) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn max_lease(mut self, duration: Duration) -> Result<Self, DataError> {
        let millis = u64::try_from(duration.as_millis())
            .map_err(|_| DataError::InvalidStableId("resource-lease-duration".to_owned()))?;
        if millis == 0 {
            return Err(DataError::InvalidStableId(
                "resource-lease-duration".to_owned(),
            ));
        }
        self.max_lease_ms = Some(millis);
        Ok(self)
    }

    pub fn description(mut self, description: impl Into<String>) -> Result<Self, DataError> {
        let description = description.into();
        if description.len() > 160
            || description
                .chars()
                .any(|value| matches!(value, '\r' | '\n' | '\0'))
        {
            return Err(DataError::InvalidStableId(
                "resource-description".to_owned(),
            ));
        }
        self.description = description;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }
}

pub(crate) struct StoredResource {
    spec: ResourceSpec,
    value: Arc<dyn Any + Send + Sync>,
}

#[derive(Clone, Default)]
pub struct ResourceRegistry {
    entries: Arc<BTreeMap<String, StoredResource>>,
}

impl ResourceRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub(crate) fn entry(&self, id: &str) -> Option<&StoredResource> {
        self.entries.get(id)
    }
}

#[derive(Default)]
pub struct ResourceRegistryBuilder {
    entries: BTreeMap<String, StoredResource>,
}

impl ResourceRegistryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T>(mut self, spec: ResourceSpec, value: T) -> Result<Self, DataError>
    where
        T: Send + Sync + 'static,
    {
        let id = spec.id.clone();
        if self
            .entries
            .insert(
                id.clone(),
                StoredResource {
                    spec,
                    value: Arc::new(value),
                },
            )
            .is_some()
        {
            return Err(DataError::DuplicateResource(id));
        }
        Ok(self)
    }

    pub fn register_arc<T>(mut self, spec: ResourceSpec, value: Arc<T>) -> Result<Self, DataError>
    where
        T: Send + Sync + 'static,
    {
        let id = spec.id.clone();
        if self
            .entries
            .insert(id.clone(), StoredResource { spec, value })
            .is_some()
        {
            return Err(DataError::DuplicateResource(id));
        }
        Ok(self)
    }

    pub fn seal(self) -> ResourceRegistry {
        ResourceRegistry {
            entries: Arc::new(self.entries),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceRequirement {
    id: String,
    capabilities: CapabilitySet,
}

impl ResourceRequirement {
    pub fn new(id: impl Into<String>) -> Result<Self, DataError> {
        let id = id.into();
        if !validate_stable_id(&id) {
            return Err(DataError::InvalidStableId(id));
        }
        Ok(Self {
            id,
            capabilities: CapabilitySet::none(),
        })
    }

    pub fn requiring(mut self, capability: impl Into<String>) -> Result<Self, DataError> {
        self.capabilities = self.capabilities.allowing(capability)?;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceGrant {
    id: String,
    capabilities: CapabilitySet,
}

impl ResourceGrant {
    pub fn new(id: impl Into<String>) -> Result<Self, DataError> {
        let id = id.into();
        if !validate_stable_id(&id) {
            return Err(DataError::InvalidStableId(id));
        }
        Ok(Self {
            id,
            capabilities: CapabilitySet::none(),
        })
    }

    pub fn allowing(mut self, capability: impl Into<String>) -> Result<Self, DataError> {
        self.capabilities = self.capabilities.allowing(capability)?;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }
}

pub struct ResourceLease<T> {
    id: String,
    provider_id: String,
    capabilities: CapabilitySet,
    value: Arc<T>,
    state: Arc<ContextState>,
    cancellation: DataCancellation,
    deadline: Instant,
}

impl<T> ResourceLease<T> {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    pub fn cancellation(&self) -> &DataCancellation {
        &self.cancellation
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn get(&self) -> Result<&T, DataError> {
        self.ensure_active()?;
        Ok(&self.value)
    }

    pub fn use_with<R>(&self, operation: impl FnOnce(&T) -> R) -> Result<R, DataError> {
        self.ensure_active()?;
        Ok(operation(&self.value))
    }

    fn ensure_active(&self) -> Result<(), DataError> {
        if self.state.is_closed() {
            return Err(DataError::ContextClosed);
        }
        if self.cancellation.is_cancelled() {
            return Err(DataError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(DataError::Deadline);
        }
        Ok(())
    }
}

impl<T> Debug for ResourceLease<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResourceLease")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("capabilities", &self.capabilities)
            .field("active", &!self.state.is_closed())
            .finish()
    }
}

pub(crate) fn acquire<T>(
    registry: &ResourceRegistry,
    grants: &BTreeMap<String, CapabilitySet>,
    requirement: &ResourceRequirement,
    state: Arc<ContextState>,
    cancellation: DataCancellation,
    request_deadline: Instant,
) -> Result<ResourceLease<T>, DataError>
where
    T: Send + Sync + 'static,
{
    if state.is_closed() {
        return Err(DataError::ContextClosed);
    }
    let entry = registry
        .entry(requirement.id())
        .ok_or_else(|| DataError::ResourceUnavailable(requirement.id().to_owned()))?;
    let granted = grants
        .get(requirement.id())
        .ok_or_else(|| DataError::ResourceUnavailable(requirement.id().to_owned()))?;
    for capability in requirement.capabilities().iter() {
        if !granted.allows(capability) || !entry.spec.capabilities.allows(capability) {
            return Err(DataError::MissingCapability {
                resource: requirement.id().to_owned(),
                capability: capability.to_owned(),
            });
        }
    }
    let value = entry
        .value
        .clone()
        .downcast::<T>()
        .map_err(|_| DataError::ResourceTypeMismatch(requirement.id().to_owned()))?;
    let deadline = entry
        .spec
        .max_lease_ms
        .map(|millis| Instant::now() + Duration::from_millis(millis))
        .map_or(request_deadline, |lease_deadline| {
            lease_deadline.min(request_deadline)
        });
    Ok(ResourceLease {
        id: entry.spec.id.clone(),
        provider_id: entry.spec.provider_id.clone(),
        capabilities: requirement.capabilities.clone(),
        value,
        state,
        cancellation,
        deadline,
    })
}
