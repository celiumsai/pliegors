// SPDX-License-Identifier: Apache-2.0

use crate::receipt::DataDurationBucket as DurationBucket;
use crate::{
    DataContext, DataError, DataOperation, DataOutcome, DataReceipt, DataSizeBucket, ResourceLease,
    ResourceRequirement, validate_stable_id,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::any::Any;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::OnceCell;

pub type LoaderFuture<Output> =
    Pin<Box<dyn Future<Output = Result<Output, DataError>> + Send + 'static>>;

pub trait Loader<Input, Output>: Send + Sync + 'static {
    fn load(&self, context: LoaderContext, input: Input) -> LoaderFuture<Output>;
}

impl<Input, Output, Function, OutputFuture> Loader<Input, Output> for Function
where
    Function: Fn(LoaderContext, Input) -> OutputFuture + Send + Sync + 'static,
    OutputFuture: Future<Output = Result<Output, DataError>> + Send + 'static,
{
    fn load(&self, context: LoaderContext, input: Input) -> LoaderFuture<Output> {
        Box::pin(self(context, input))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoaderPolicy {
    id: String,
    semantic_revision: u32,
    input_schema_id: String,
    output_schema_id: String,
    resources: Vec<ResourceRequirement>,
    cache_policy_id: Option<String>,
    max_output_bytes: usize,
    deduplicate_in_request: bool,
}

impl LoaderPolicy {
    pub fn new(
        id: impl Into<String>,
        semantic_revision: u32,
        input_schema_id: impl Into<String>,
        output_schema_id: impl Into<String>,
    ) -> Result<Self, DataError> {
        let id = id.into();
        let input_schema_id = input_schema_id.into();
        let output_schema_id = output_schema_id.into();
        for value in [&id, &input_schema_id, &output_schema_id] {
            if !validate_stable_id(value) {
                return Err(DataError::InvalidStableId(value.clone()));
            }
        }
        if semantic_revision == 0 {
            return Err(DataError::InvalidLoaderPolicy(
                "semantic revision must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            id,
            semantic_revision,
            input_schema_id,
            output_schema_id,
            resources: Vec::new(),
            cache_policy_id: None,
            max_output_bytes: 256 * 1_024,
            deduplicate_in_request: true,
        })
    }

    pub fn resource(mut self, requirement: ResourceRequirement) -> Result<Self, DataError> {
        if self
            .resources
            .iter()
            .any(|current| current.id() == requirement.id())
        {
            return Err(DataError::DuplicateResource(requirement.id().to_owned()));
        }
        if self.resources.len() >= 32 {
            return Err(DataError::InvalidLoaderPolicy(
                "a loader may require at most 32 resources".to_owned(),
            ));
        }
        self.resources.push(requirement);
        Ok(self)
    }

    pub fn cache_policy(mut self, id: impl Into<String>) -> Result<Self, DataError> {
        let id = id.into();
        if !validate_stable_id(&id) {
            return Err(DataError::InvalidStableId(id));
        }
        self.cache_policy_id = Some(id);
        Ok(self)
    }

    pub fn max_output_bytes(mut self, maximum: usize) -> Result<Self, DataError> {
        if maximum == 0 || maximum > 16 * 1_024 * 1_024 {
            return Err(DataError::InvalidLoaderPolicy(
                "max output bytes must be between 1 and 16777216".to_owned(),
            ));
        }
        self.max_output_bytes = maximum;
        Ok(self)
    }

    pub fn deduplicate_in_request(mut self, enabled: bool) -> Self {
        self.deduplicate_in_request = enabled;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn semantic_revision(&self) -> u32 {
        self.semantic_revision
    }

    pub fn resource_requirements(&self) -> &[ResourceRequirement] {
        &self.resources
    }

    pub fn cache_policy_id(&self) -> Option<&str> {
        self.cache_policy_id.as_deref()
    }

    pub fn max_output_bytes_value(&self) -> usize {
        self.max_output_bytes
    }

    pub fn deduplicates_in_request(&self) -> bool {
        self.deduplicate_in_request
    }

    pub fn contract_digest(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"pliego-loader-policy-v1\0");
        for value in [
            self.id.as_bytes(),
            &self.semantic_revision.to_be_bytes(),
            self.input_schema_id.as_bytes(),
            self.output_schema_id.as_bytes(),
            self.cache_policy_id.as_deref().unwrap_or("").as_bytes(),
            &self.max_output_bytes.to_be_bytes(),
            &[u8::from(self.deduplicate_in_request)],
        ] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value);
        }
        let mut resources = self.resources.iter().collect::<Vec<_>>();
        resources.sort_by(|left, right| left.id().cmp(right.id()));
        for resource in resources {
            digest.update(resource.id().as_bytes());
            digest.update([0]);
            for capability in resource.capabilities().iter() {
                digest.update(capability.as_bytes());
                digest.update([0]);
            }
        }
        encode_hex(&digest.finalize())
    }
}

#[derive(Clone)]
pub struct LoaderContext {
    data: DataContext,
    policy: Arc<LoaderPolicy>,
}

impl LoaderContext {
    pub fn data(&self) -> &DataContext {
        &self.data
    }

    pub fn policy(&self) -> &LoaderPolicy {
        &self.policy
    }

    pub fn resource<T>(&self, id: &str) -> Result<ResourceLease<T>, DataError>
    where
        T: Send + Sync + 'static,
    {
        let requirement = self
            .policy
            .resources
            .iter()
            .find(|requirement| requirement.id() == id)
            .ok_or_else(|| DataError::ResourceUnavailable(id.to_owned()))?;
        self.data.resource(requirement)
    }
}

impl Debug for LoaderContext {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoaderContext")
            .field("loader_id", &self.policy.id)
            .field("semantic_revision", &self.policy.semantic_revision)
            .field("request_id", &self.data.identity().request_id())
            .field("route_id", &self.data.identity().route_id())
            .finish()
    }
}

type ErasedOutput = Arc<dyn Any + Send + Sync>;
pub(crate) type LoaderCell = Arc<OnceCell<Result<ErasedOutput, DataError>>>;

impl DataContext {
    pub async fn load<Input, Output, Implementation>(
        &self,
        policy: &LoaderPolicy,
        implementation: &Implementation,
        input: Input,
    ) -> Result<Arc<Output>, DataError>
    where
        Input: Serialize + Send + 'static,
        Output: Serialize + Send + Sync + 'static,
        Implementation: Loader<Input, Output>,
    {
        self.require_loader_policy(policy)?;
        if self.is_closed() {
            return Err(DataError::ContextClosed);
        }
        if self.cancellation().is_cancelled() {
            return Err(DataError::Cancelled);
        }
        let input_bytes = canonical_json_bytes(&input)?;
        let key = invocation_key(policy, &input_bytes, self.identity().route_id());
        let (cell, deduplicated) = if policy.deduplicate_in_request {
            let mut cells = self
                .inner
                .loader_cells
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(existing) = cells.get(&key) {
                (existing.clone(), true)
            } else {
                let cell = Arc::new(OnceCell::new());
                cells.insert(key, cell.clone());
                (cell, false)
            }
        } else {
            (Arc::new(OnceCell::new()), false)
        };

        let started = Instant::now();
        let context = LoaderContext {
            data: self.clone(),
            policy: Arc::new(policy.clone()),
        };
        let result = cell
            .get_or_init(|| async move {
                let cancellation = context.data.cancellation().clone();
                let deadline = tokio::time::Instant::from_std(context.data.deadline());
                let future = implementation.load(context.clone(), input);
                let output = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(DataError::Cancelled),
                    _ = tokio::time::sleep_until(deadline) => Err(DataError::Deadline),
                    result = future => result,
                }?;
                let output_bytes =
                    serde_json::to_vec(&output).map_err(|_| DataError::Serialization)?;
                if output_bytes.len() > policy.max_output_bytes {
                    return Err(DataError::LoaderOutput {
                        actual: output_bytes.len(),
                        maximum: policy.max_output_bytes,
                    });
                }
                Ok(Arc::new(output) as ErasedOutput)
            })
            .await
            .clone();

        let (outcome, output_size, code) = match &result {
            Ok(output) => {
                let bytes = output
                    .clone()
                    .downcast::<Output>()
                    .map_err(|_| {
                        DataError::LoaderFailure("deduplicated output type mismatch".to_owned())
                    })
                    .and_then(|value| {
                        serde_json::to_vec(&*value).map_err(|_| DataError::Serialization)
                    })
                    .map(|bytes| bytes.len())
                    .unwrap_or(0);
                (DataOutcome::Success, bytes, None)
            }
            Err(DataError::Cancelled | DataError::Deadline) => (
                DataOutcome::Cancelled,
                0,
                Some(DataError::Cancelled.code().to_owned()),
            ),
            Err(error) => (DataOutcome::Failed, 0, Some(error.code().to_owned())),
        };
        self.record_receipt(DataReceipt {
            contract: "dev.pliegors.data-receipt/v1".to_owned(),
            operation: DataOperation::Loader,
            operation_id: policy.id.clone(),
            semantic_revision: policy.semantic_revision,
            outcome,
            duration_bucket: DurationBucket::from_duration(started.elapsed()),
            output_size_bucket: DataSizeBucket::from_bytes(output_size),
            deduplicated,
            cancel_reason: self.cancel_reason(),
            diagnostic_code: code,
        });

        result?
            .downcast::<Output>()
            .map_err(|_| DataError::LoaderFailure("deduplicated output type mismatch".to_owned()))
    }
}

fn invocation_key(policy: &LoaderPolicy, input: &[u8], route_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"pliego-loader-invocation-v1\0");
    for value in [
        policy.id.as_bytes(),
        &policy.semantic_revision.to_be_bytes(),
        policy.input_schema_id.as_bytes(),
        policy.output_schema_id.as_bytes(),
        policy.cache_policy_id.as_deref().unwrap_or("").as_bytes(),
        route_id.as_bytes(),
        input,
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value);
    }
    encode_hex(&digest.finalize())
}

pub(crate) fn canonical_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, DataError> {
    let value = serde_json::to_value(value).map_err(|_| DataError::Serialization)?;
    let mut output = Vec::new();
    write_canonical_json(&value, &mut output)?;
    Ok(output)
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), DataError> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            output.extend(serde_json::to_vec(value).map_err(|_| DataError::Serialization)?);
        }
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                output.extend(serde_json::to_vec(key).map_err(|_| DataError::Serialization)?);
                output.push(b':');
                write_canonical_json(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
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
