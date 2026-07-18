// SPDX-License-Identifier: Apache-2.0

use crate::{
    Capability, CapabilityPolicy, Determinism, EffectBroker, EffectError, EntryKind, Plane,
    ValidatedExtension, inspect_component,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

mod effect_bindings {
    wasmtime::component::bindgen!({
        path: "wit/effects",
        world: "recorded-effect",
    });
}

mod build_bindings {
    wasmtime::component::bindgen!({
        path: "wit/build",
        world: "transformer",
    });
}

const FUEL_PER_DECLARED_CPU_MS: u64 = 100_000;
const MAX_TRANSFORM_INPUT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
pub struct ComponentHost {
    effect_executor: Option<Arc<dyn EffectExecutor>>,
}

impl fmt::Debug for ComponentHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComponentHost")
            .field("effect_executor", &self.effect_executor.is_some())
            .finish()
    }
}

struct HostState {
    limits: StoreLimits,
    effect_broker: Option<EffectBroker>,
    effect_executor: Option<Arc<dyn EffectExecutor>>,
    output_limit_bytes: usize,
}

pub trait EffectExecutor: Send + Sync + 'static {
    fn execute(
        &self,
        capability: Capability,
        operation: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, String>;
}

impl<F> EffectExecutor for F
where
    F: Fn(Capability, &str, &[u8]) -> Result<Vec<u8>, String> + Send + Sync + 'static,
{
    fn execute(
        &self,
        capability: Capability,
        operation: &str,
        input: &[u8],
    ) -> Result<Vec<u8>, String> {
        self(capability, operation, input)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentHostReceipt {
    pub schema: String,
    pub extension: String,
    pub digest: String,
    pub runtime: String,
    pub fuel_limit: u64,
    pub memory_limit_bytes: u64,
    pub imports: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildTransformInput {
    pub path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub options_json: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildTransformOutput {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub diagnostics_json: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildTransformReceipt {
    pub schema: String,
    pub extension: String,
    pub extension_digest: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub fuel_limit: u64,
    pub fuel_consumed: u64,
    pub memory_limit_bytes: u64,
    pub output_limit_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildTransformExecution {
    pub output: BuildTransformOutput,
    pub receipt: BuildTransformReceipt,
}

impl ComponentHost {
    pub fn deny_by_default() -> Result<Self, ComponentHostError> {
        configured_engine()?;
        Ok(Self {
            effect_executor: None,
        })
    }

    pub fn with_effect_executor(mut self, executor: impl EffectExecutor) -> Self {
        self.effect_executor = Some(Arc::new(executor));
        self
    }

    pub fn instantiate(
        &self,
        extension: &ValidatedExtension,
        bytes: &[u8],
    ) -> Result<ComponentHostReceipt, ComponentHostError> {
        let manifest = extension.manifest();
        if manifest.entry.kind != EntryKind::WasmComponent {
            return Err(ComponentHostError::WrongEntryKind);
        }
        let inspection = inspect_component(bytes).map_err(ComponentHostError::Component)?;
        inspection
            .verify_manifest(manifest)
            .map_err(ComponentHostError::Component)?;
        let has_effect_broker = inspection
            .imports
            .iter()
            .any(|import| import.starts_with("pliego:effects/broker@"));
        let unlinked = inspection
            .imports
            .iter()
            .filter(|import| !import.starts_with("pliego:effects/broker@"))
            .cloned()
            .collect::<Vec<_>>();
        if !unlinked.is_empty() || (has_effect_broker && self.effect_executor.is_none()) {
            return Err(ComponentHostError::UnlinkedImports(inspection.imports));
        }

        let engine = configured_engine()?;
        let component = Component::new(&engine, bytes)
            .map_err(|error| ComponentHostError::Runtime(error.to_string()))?;
        let memory_limit = usize::try_from(manifest.budgets.memory_bytes).map_err(|_| {
            ComponentHostError::Budget("memory budget exceeds host usize".to_owned())
        })?;
        let output_limit_bytes = usize::try_from(manifest.budgets.output_bytes).map_err(|_| {
            ComponentHostError::Budget("output budget exceeds host usize".to_owned())
        })?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_limit)
            .instances(16)
            .memories(1)
            .tables(16)
            .trap_on_grow_failure(true)
            .build();
        let effect_policy = extension
            .receipt()
            .granted_capabilities
            .iter()
            .fold(CapabilityPolicy::deny_all(), |policy, capability| {
                policy.grant(*capability)
            });
        let mut store = Store::new(
            &engine,
            HostState {
                limits,
                effect_broker: has_effect_broker.then(|| EffectBroker::new(effect_policy)),
                effect_executor: self.effect_executor.clone(),
                output_limit_bytes,
            },
        );
        store.limiter(|state| &mut state.limits);
        let fuel_limit = manifest
            .budgets
            .cpu_ms
            .checked_mul(FUEL_PER_DECLARED_CPU_MS)
            .ok_or_else(|| ComponentHostError::Budget("fuel limit overflow".to_owned()))?;
        store
            .set_fuel(fuel_limit)
            .map_err(|error| ComponentHostError::Runtime(error.to_string()))?;
        store.set_epoch_deadline(1);

        let mut linker = Linker::new(&engine);
        if has_effect_broker {
            effect_bindings::pliego::effects::broker::add_to_linker::<
                _,
                wasmtime::component::HasSelf<_>,
            >(&mut linker, |state| state)
            .map_err(|error| ComponentHostError::Runtime(error.to_string()))?;
        }
        let deadline = EpochDeadline::start(&engine, manifest.budgets.wall_time_ms);
        let instantiated = linker.instantiate(&mut store, &component);
        if deadline.finish()? {
            return Err(ComponentHostError::Deadline);
        }
        if let Err(error) = instantiated {
            if store.get_fuel().is_ok_and(|remaining| remaining == 0) {
                return Err(ComponentHostError::FuelExhausted);
            }
            return Err(ComponentHostError::Runtime(error.to_string()));
        }
        Ok(ComponentHostReceipt {
            schema: "dev.pliegors.component-host/v1".to_owned(),
            extension: extension.receipt().extension.clone(),
            digest: extension.receipt().digest.clone(),
            runtime: "wasmtime-component-36.0.8".to_owned(),
            fuel_limit,
            memory_limit_bytes: manifest.budgets.memory_bytes,
            imports: inspection.imports,
        })
    }

    pub fn execute_build_transform(
        &self,
        extension: &ValidatedExtension,
        bytes: &[u8],
        input: BuildTransformInput,
    ) -> Result<BuildTransformExecution, ComponentHostError> {
        let manifest = extension.manifest();
        if manifest.entry.kind != EntryKind::WasmComponent {
            return Err(ComponentHostError::WrongEntryKind);
        }
        if manifest.plane != Plane::Build {
            return Err(ComponentHostError::WrongPlane);
        }
        if manifest.entry.world.as_deref() != Some("pliego:build/transformer@0.1.0") {
            return Err(ComponentHostError::WrongWorld);
        }
        if manifest.determinism != Determinism::Pure {
            return Err(ComponentHostError::Contract(
                "build transform execution requires determinism pure".to_owned(),
            ));
        }
        let inspection = inspect_component(bytes).map_err(ComponentHostError::Component)?;
        inspection
            .verify_manifest(manifest)
            .map_err(ComponentHostError::Component)?;
        if !inspection.imports.is_empty() {
            return Err(ComponentHostError::UnlinkedImports(inspection.imports));
        }
        let input_size = input
            .path
            .len()
            .saturating_add(input.media_type.len())
            .saturating_add(input.bytes.len())
            .saturating_add(input.options_json.len());
        if input_size > MAX_TRANSFORM_INPUT_BYTES {
            return Err(ComponentHostError::Budget(
                "transform input exceeds the 128 MiB runtime ceiling".to_owned(),
            ));
        }
        validate_transform_input(&input)?;

        let engine = configured_engine()?;
        let component = Component::new(&engine, bytes)
            .map_err(|error| ComponentHostError::Runtime(error.to_string()))?;
        let memory_limit = usize::try_from(manifest.budgets.memory_bytes).map_err(|_| {
            ComponentHostError::Budget("memory budget exceeds host usize".to_owned())
        })?;
        let output_limit = usize::try_from(manifest.budgets.output_bytes).map_err(|_| {
            ComponentHostError::Budget("output budget exceeds host usize".to_owned())
        })?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(memory_limit)
            .instances(16)
            .memories(1)
            .tables(16)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(
            &engine,
            HostState {
                limits,
                effect_broker: None,
                effect_executor: None,
                output_limit_bytes: output_limit,
            },
        );
        store.limiter(|state| &mut state.limits);
        let fuel_limit = manifest
            .budgets
            .cpu_ms
            .checked_mul(FUEL_PER_DECLARED_CPU_MS)
            .ok_or_else(|| ComponentHostError::Budget("fuel limit overflow".to_owned()))?;
        store
            .set_fuel(fuel_limit)
            .map_err(|error| ComponentHostError::Runtime(error.to_string()))?;
        store.set_epoch_deadline(1);

        let linker = Linker::new(&engine);
        let guest_input = build_bindings::exports::pliego::build::transform::TransformInput {
            path: input.path,
            media_type: input.media_type,
            bytes: input.bytes,
            options_json: input.options_json,
        };
        let input_sha256 = hash_transform_input(&guest_input);
        let deadline = EpochDeadline::start(&engine, manifest.budgets.wall_time_ms);
        let guest_result =
            match build_bindings::Transformer::instantiate(&mut store, &component, &linker) {
                Ok(bindings) => bindings
                    .pliego_build_transform()
                    .call_apply(&mut store, &guest_input),
                Err(error) => Err(error),
            };
        let deadline_expired = deadline.finish()?;
        if deadline_expired {
            return Err(ComponentHostError::Deadline);
        }
        let guest_result = match guest_result {
            Ok(result) => result,
            Err(_) if store.get_fuel().is_ok_and(|remaining| remaining == 0) => {
                return Err(ComponentHostError::FuelExhausted);
            }
            Err(error) => return Err(ComponentHostError::Runtime(error.to_string())),
        };
        let output = guest_result.map_err(ComponentHostError::Guest)?;
        validate_transform_output(&output)?;
        let output_size = output
            .media_type
            .len()
            .saturating_add(output.bytes.len())
            .saturating_add(output.diagnostics_json.len());
        if output_size > output_limit {
            return Err(ComponentHostError::Budget(format!(
                "transform output exceeds the {output_limit}-byte budget"
            )));
        }
        let fuel_remaining = store
            .get_fuel()
            .map_err(|error| ComponentHostError::Runtime(error.to_string()))?;
        let output_sha256 = hash_transform_output(&output);
        Ok(BuildTransformExecution {
            output: BuildTransformOutput {
                media_type: output.media_type,
                bytes: output.bytes,
                diagnostics_json: output.diagnostics_json,
            },
            receipt: BuildTransformReceipt {
                schema: "dev.pliegors.build-transform-receipt/v1".to_owned(),
                extension: extension.receipt().extension.clone(),
                extension_digest: extension.receipt().digest.clone(),
                input_sha256,
                output_sha256,
                fuel_limit,
                fuel_consumed: fuel_limit.saturating_sub(fuel_remaining),
                memory_limit_bytes: manifest.budgets.memory_bytes,
                output_limit_bytes: manifest.budgets.output_bytes,
            },
        })
    }
}

fn validate_transform_input(input: &BuildTransformInput) -> Result<(), ComponentHostError> {
    validate_transform_path(&input.path)?;
    validate_media_type(&input.media_type)?;
    serde_json::from_str::<serde_json::Value>(&input.options_json).map_err(|error| {
        ComponentHostError::Contract(format!("transform options_json is invalid JSON: {error}"))
    })?;
    Ok(())
}

fn validate_transform_output(
    output: &build_bindings::exports::pliego::build::transform::TransformOutput,
) -> Result<(), ComponentHostError> {
    validate_media_type(&output.media_type)?;
    let diagnostics =
        serde_json::from_str::<serde_json::Value>(&output.diagnostics_json).map_err(|error| {
            ComponentHostError::Contract(format!(
                "transform diagnostics_json is invalid JSON: {error}"
            ))
        })?;
    if !diagnostics.is_array() {
        return Err(ComponentHostError::Contract(
            "transform diagnostics_json must encode an array".to_owned(),
        ));
    }
    Ok(())
}

fn validate_transform_path(path: &str) -> Result<(), ComponentHostError> {
    if path.is_empty()
        || path.len() > 1024
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(ComponentHostError::Contract(
            "transform path must be a normalized relative slash path".to_owned(),
        ));
    }
    Ok(())
}

fn validate_media_type(value: &str) -> Result<(), ComponentHostError> {
    if value.is_empty()
        || value.len() > 256
        || !value.contains('/')
        || value.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(ComponentHostError::Contract(
            "transform media type must be a bounded visible MIME value".to_owned(),
        ));
    }
    Ok(())
}

fn configured_engine() -> Result<Engine, ComponentHostError> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .consume_fuel(true)
        .epoch_interruption(true);
    Engine::new(&config).map_err(|error| ComponentHostError::Runtime(error.to_string()))
}

struct EpochDeadline {
    cancel: mpsc::SyncSender<()>,
    expired: Arc<AtomicBool>,
    timer: std::thread::JoinHandle<()>,
    deadline: Instant,
}

impl EpochDeadline {
    fn start(engine: &Engine, wall_time_ms: u64) -> Self {
        let (cancel, timeout) = mpsc::sync_channel(1);
        let expired = Arc::new(AtomicBool::new(false));
        let timer_expired = Arc::clone(&expired);
        let engine = engine.clone();
        let wall_time = Duration::from_millis(wall_time_ms);
        let deadline = Instant::now() + wall_time;
        let timer = std::thread::spawn(move || {
            if timeout.recv_timeout(wall_time).is_err() {
                timer_expired.store(true, Ordering::Release);
                engine.increment_epoch();
            }
        });
        Self {
            cancel,
            expired,
            timer,
            deadline,
        }
    }

    fn finish(self) -> Result<bool, ComponentHostError> {
        let elapsed = Instant::now() >= self.deadline;
        let _ = self.cancel.send(());
        self.timer
            .join()
            .map_err(|_| ComponentHostError::Runtime("deadline thread panicked".to_owned()))?;
        Ok(elapsed || self.expired.load(Ordering::Acquire))
    }
}

fn hash_transform_input(
    input: &build_bindings::exports::pliego::build::transform::TransformInput,
) -> String {
    use sha2::Digest;

    let mut digest = sha2::Sha256::new();
    for value in [input.path.as_bytes(), input.media_type.as_bytes()] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value);
    }
    digest.update((input.bytes.len() as u64).to_le_bytes());
    digest.update(&input.bytes);
    digest.update((input.options_json.len() as u64).to_le_bytes());
    digest.update(input.options_json.as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn hash_transform_output(
    output: &build_bindings::exports::pliego::build::transform::TransformOutput,
) -> String {
    let mut digest = sha2::Sha256::new();
    for value in [
        output.media_type.as_bytes(),
        output.bytes.as_slice(),
        output.diagnostics_json.as_bytes(),
    ] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value);
    }
    format!("sha256:{:x}", digest.finalize())
}

impl effect_bindings::pliego::effects::broker::Host for HostState {
    fn execute(
        &mut self,
        request: effect_bindings::pliego::effects::broker::EffectRequest,
    ) -> Result<
        effect_bindings::pliego::effects::broker::EffectResponse,
        effect_bindings::pliego::effects::broker::EffectFailure,
    > {
        let failure = |message: String, receipt_json: Option<String>| {
            effect_bindings::pliego::effects::broker::EffectFailure {
                message,
                receipt_json,
            }
        };
        let capability =
            Capability::from_str(&request.capability).map_err(|error| failure(error, None))?;
        let executor = self
            .effect_executor
            .clone()
            .ok_or_else(|| failure("effect executor is unavailable".to_owned(), None))?;
        let output_limit = self.output_limit_bytes;
        let broker = self
            .effect_broker
            .as_mut()
            .ok_or_else(|| failure("effect broker was not admitted".to_owned(), None))?;
        if request.payload.len() > output_limit {
            return Err(failure(
                format!("effect input exceeds the {output_limit}-byte budget"),
                None,
            ));
        }
        let result = broker.execute(capability, &request.operation, &request.payload, || {
            let executed = catch_unwind(AssertUnwindSafe(|| {
                executor.execute(capability, &request.operation, &request.payload)
            }))
            .unwrap_or_else(|_| Err("effect executor panicked".to_owned()));
            match executed {
                Ok(output) if output.len() > output_limit => Err(format!(
                    "effect output exceeds the {output_limit}-byte budget"
                )),
                Ok(output) => Ok(output),
                Err(message) if message.len() > output_limit => Err(format!(
                    "effect failure exceeds the {output_limit}-byte budget"
                )),
                Err(message) => Err(message),
            }
        });
        let output = match result {
            Ok(output) => output,
            Err(EffectError::Executor(error)) => {
                let receipt_json = serde_json::to_string(&error.receipt).map_err(|encode| {
                    failure(format!("cannot encode effect receipt: {encode}"), None)
                })?;
                return Err(failure(error.message, Some(receipt_json)));
            }
            Err(error) => return Err(failure(error.to_string(), None)),
        };
        let receipt = broker
            .receipts()
            .last()
            .ok_or_else(|| failure("effect completed without a receipt".to_owned(), None))?;
        let receipt_json = serde_json::to_string(receipt)
            .map_err(|error| failure(format!("cannot encode effect receipt: {error}"), None))?;
        Ok(effect_bindings::pliego::effects::broker::EffectResponse {
            payload: output,
            receipt_json,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentHostError {
    WrongEntryKind,
    WrongPlane,
    WrongWorld,
    Component(String),
    Contract(String),
    UnlinkedImports(Vec<String>),
    Budget(String),
    Runtime(String),
    Guest(String),
    Deadline,
    FuelExhausted,
}

impl fmt::Display for ComponentHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongEntryKind => {
                formatter.write_str("component host requires a wasm-component entry")
            }
            Self::WrongPlane => formatter.write_str("build transform host requires plane build"),
            Self::WrongWorld => formatter
                .write_str("build transform host requires world pliego:build/transformer@0.1.0"),
            Self::Component(error) => write!(formatter, "component contract failed: {error}"),
            Self::Contract(error) => write!(formatter, "OpenSDK contract failed: {error}"),
            Self::UnlinkedImports(imports) => write!(
                formatter,
                "deny-by-default host has no admitted linker for imports: {}",
                imports.join(", ")
            ),
            Self::Budget(error) => write!(formatter, "component budget failed: {error}"),
            Self::Runtime(error) => write!(formatter, "component runtime failed: {error}"),
            Self::Guest(error) => write!(formatter, "component returned an error: {error}"),
            Self::Deadline => formatter.write_str("component exceeded its wall-time budget"),
            Self::FuelExhausted => formatter.write_str("component exhausted its fuel budget"),
        }
    }
}

impl std::error::Error for ComponentHostError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Budget, Capability, CapabilityPolicy, Determinism, ExtensionEntry, ExtensionIdentity,
        ExtensionManifest, HostContract, Lifecycle, OPENSDK_API_VERSION, OPENSDK_MANIFEST_SCHEMA,
        Plane,
    };
    use semver::Version;
    use sha2::{Digest, Sha256};
    use wit_component::{ComponentEncoder, StringEncoding, dummy_module, embed_component_metadata};
    use wit_parser::{ManglingAndAbi, Resolve};

    fn manifest(
        bytes: &[u8],
        imports: Vec<String>,
        capabilities: Vec<Capability>,
    ) -> ExtensionManifest {
        ExtensionManifest {
            schema: OPENSDK_MANIFEST_SCHEMA.to_owned(),
            api_version: OPENSDK_API_VERSION.to_owned(),
            host_version: ">=0.1.0-preview.1, <0.2.0".to_owned(),
            plane: Plane::Build,
            identity: ExtensionIdentity {
                namespace: "pliego".to_owned(),
                name: "host-fixture".to_owned(),
                version: "0.1.0".to_owned(),
                digest: format!("sha256:{:x}", Sha256::digest(bytes)),
            },
            entry: ExtensionEntry {
                kind: EntryKind::WasmComponent,
                path: "component.wasm".to_owned(),
                world: Some("pliego:build/transformer@0.1.0".to_owned()),
                custom_element: None,
            },
            determinism: if capabilities.is_empty() {
                Determinism::Pure
            } else {
                Determinism::RecordedEffect
            },
            imports,
            exports: Vec::new(),
            capabilities,
            required_features: Vec::new(),
            optional_features: Vec::new(),
            budgets: Budget {
                cpu_ms: 10,
                wall_time_ms: 100,
                memory_bytes: 1024 * 1024,
                output_bytes: 1024,
            },
            lifecycle: Lifecycle {
                init: true,
                update: false,
                suspend: false,
                resume: false,
                dispose: true,
                hmr: false,
            },
        }
    }

    fn effect_import_component() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/effects");
        let mut resolve = Resolve::default();
        let (package, _) = resolve.push_dir(path).unwrap();
        let world = resolve
            .select_world(package, Some("recorded-effect"))
            .unwrap();
        let mut core = dummy_module(&resolve, world, ManglingAndAbi::Standard32);
        embed_component_metadata(&mut core, &resolve, world, StringEncoding::UTF8).unwrap();
        ComponentEncoder::default()
            .module(&core)
            .unwrap()
            .validate(true)
            .encode()
            .unwrap()
    }

    #[test]
    fn pure_component_instantiates_without_ambient_imports() {
        let bytes = wat::parse_str("(component)").unwrap();
        let host_contract = HostContract::preview(
            Version::parse(OPENSDK_API_VERSION).unwrap(),
            CapabilityPolicy::deny_all(),
        );
        let admitted = host_contract
            .admit_component(manifest(&bytes, Vec::new(), Vec::new()), &bytes)
            .unwrap();
        let receipt = ComponentHost::deny_by_default()
            .unwrap()
            .instantiate(&admitted, &bytes)
            .unwrap();
        assert_eq!(receipt.imports, Vec::<String>::new());
        assert_eq!(receipt.memory_limit_bytes, 1024 * 1024);
    }

    #[test]
    fn wasi_environment_filesystem_and_network_are_never_ambiently_linked() {
        let cases = [
            ("wasi:cli/environment@0.2.4", vec![Capability::Environment]),
            (
                "wasi:filesystem/preopens@0.2.4",
                vec![Capability::FilesystemRead, Capability::FilesystemWrite],
            ),
            ("wasi:sockets/network@0.2.4", vec![Capability::Network]),
        ];
        for (import, required) in cases {
            let bytes = wat::parse_str(format!(
                r#"(component
                    (type $probe (func))
                    (import "{import}" (func $probe (type $probe)))
                )"#,
            ))
            .unwrap();
            let mut capabilities = required.clone();
            capabilities.push(Capability::EffectBroker);
            capabilities.sort();
            let policy = required.into_iter().fold(
                CapabilityPolicy::deny_all().grant(Capability::EffectBroker),
                CapabilityPolicy::grant,
            );
            let host_contract =
                HostContract::preview(Version::parse(OPENSDK_API_VERSION).unwrap(), policy);
            let admitted = host_contract
                .admit_component(
                    manifest(&bytes, vec![import.to_owned()], capabilities),
                    &bytes,
                )
                .unwrap();
            assert!(matches!(
                ComponentHost::deny_by_default()
                    .unwrap()
                    .instantiate(&admitted, &bytes),
                Err(ComponentHostError::UnlinkedImports(_))
            ));
        }
    }

    #[test]
    fn component_model_effect_interface_links_and_emits_a_digest_bound_receipt() {
        let bytes = effect_import_component();
        let inspection = inspect_component(&bytes).unwrap();
        assert_eq!(inspection.imports, ["pliego:effects/broker@0.1.0"]);
        let capabilities = vec![Capability::Network, Capability::EffectBroker];
        let policy = CapabilityPolicy::deny_all()
            .grant(Capability::Network)
            .grant(Capability::EffectBroker);
        let mut extension_manifest = manifest(&bytes, inspection.imports, capabilities);
        extension_manifest.entry.world = Some("pliego:effects/recorded-effect@0.1.0".to_owned());
        let admitted =
            HostContract::preview(Version::parse(OPENSDK_API_VERSION).unwrap(), policy.clone())
                .admit_component(extension_manifest, &bytes)
                .unwrap();
        let receipt = ComponentHost::deny_by_default()
            .unwrap()
            .with_effect_executor(|capability, operation: &str, input: &[u8]| {
                assert_eq!(capability, Capability::Network);
                assert_eq!(operation, "fetch");
                assert_eq!(input, b"request");
                Ok(b"response".to_vec())
            })
            .instantiate(&admitted, &bytes)
            .unwrap();
        assert_eq!(receipt.imports, ["pliego:effects/broker@0.1.0"]);

        let mut state = HostState {
            limits: StoreLimitsBuilder::new().build(),
            effect_broker: Some(EffectBroker::new(policy)),
            effect_executor: Some(Arc::new(|_: Capability, _: &str, _: &[u8]| {
                Ok(b"response".to_vec())
            })),
            output_limit_bytes: 1024,
        };
        let response = effect_bindings::pliego::effects::broker::Host::execute(
            &mut state,
            effect_bindings::pliego::effects::broker::EffectRequest {
                capability: "network".to_owned(),
                operation: "fetch".to_owned(),
                payload: b"request".to_vec(),
            },
        )
        .unwrap();
        assert_eq!(response.payload, b"response");
        let effect_receipt: crate::EffectReceipt =
            serde_json::from_str(&response.receipt_json).unwrap();
        assert_eq!(effect_receipt.sequence, 1);
        assert_eq!(effect_receipt.outcome, crate::EffectOutcome::Success);
        assert_eq!(effect_receipt.capability, Capability::Network);
        assert_eq!(
            effect_receipt.input_sha256,
            format!("sha256:{:x}", Sha256::digest(b"request"))
        );

        state.effect_executor = Some(Arc::new(|_: Capability, _: &str, _: &[u8]| {
            Err("upstream unavailable".to_owned())
        }));
        let failure = effect_bindings::pliego::effects::broker::Host::execute(
            &mut state,
            effect_bindings::pliego::effects::broker::EffectRequest {
                capability: "network".to_owned(),
                operation: "fetch".to_owned(),
                payload: b"request".to_vec(),
            },
        )
        .unwrap_err();
        let failure_receipt: crate::EffectReceipt =
            serde_json::from_str(failure.receipt_json.as_deref().unwrap()).unwrap();
        assert_eq!(failure_receipt.sequence, 2);
        assert_eq!(failure_receipt.outcome, crate::EffectOutcome::Error);
        assert_eq!(failure.message, "upstream unavailable");

        state.effect_executor = Some(Arc::new(|_: Capability, _: &str, _: &[u8]| {
            panic!("oversized input reached the executor")
        }));
        let oversized_input = effect_bindings::pliego::effects::broker::Host::execute(
            &mut state,
            effect_bindings::pliego::effects::broker::EffectRequest {
                capability: "network".to_owned(),
                operation: "fetch".to_owned(),
                payload: vec![0; 1025],
            },
        )
        .unwrap_err();
        assert_eq!(
            oversized_input.message,
            "effect input exceeds the 1024-byte budget"
        );
        assert!(oversized_input.receipt_json.is_none());
        assert_eq!(state.effect_broker.as_ref().unwrap().receipts().len(), 2);

        state.effect_executor = Some(Arc::new(|_: Capability, _: &str, _: &[u8]| {
            Err("x".repeat(1025))
        }));
        let oversized_failure = effect_bindings::pliego::effects::broker::Host::execute(
            &mut state,
            effect_bindings::pliego::effects::broker::EffectRequest {
                capability: "network".to_owned(),
                operation: "fetch".to_owned(),
                payload: b"request".to_vec(),
            },
        )
        .unwrap_err();
        assert_eq!(
            oversized_failure.message,
            "effect failure exceeds the 1024-byte budget"
        );
        let oversized_receipt: crate::EffectReceipt =
            serde_json::from_str(oversized_failure.receipt_json.as_deref().unwrap()).unwrap();
        assert_eq!(oversized_receipt.sequence, 3);
        assert_eq!(oversized_receipt.outcome, crate::EffectOutcome::Error);

        state.effect_executor = Some(Arc::new(|_: Capability, _: &str, _: &[u8]| {
            panic!("broken host bridge")
        }));
        let panic_failure = effect_bindings::pliego::effects::broker::Host::execute(
            &mut state,
            effect_bindings::pliego::effects::broker::EffectRequest {
                capability: "network".to_owned(),
                operation: "fetch".to_owned(),
                payload: b"request".to_vec(),
            },
        )
        .unwrap_err();
        assert_eq!(panic_failure.message, "effect executor panicked");
        let panic_receipt: crate::EffectReceipt =
            serde_json::from_str(panic_failure.receipt_json.as_deref().unwrap()).unwrap();
        assert_eq!(panic_receipt.sequence, 4);
        assert_eq!(panic_receipt.outcome, crate::EffectOutcome::Error);
    }

    #[test]
    fn build_transform_contract_rejects_ambiguous_inputs_and_diagnostics() {
        let valid = BuildTransformInput {
            path: "src/content.txt".to_owned(),
            media_type: "text/plain; charset=utf-8".to_owned(),
            bytes: b"hello".to_vec(),
            options_json: "{}".to_owned(),
        };
        validate_transform_input(&valid).unwrap();

        for path in ["", "/etc/passwd", "src\\content.txt", "src/../secret"] {
            let mut input = valid.clone();
            input.path = path.to_owned();
            assert!(matches!(
                validate_transform_input(&input),
                Err(ComponentHostError::Contract(_))
            ));
        }

        let mut invalid_media_type = valid.clone();
        invalid_media_type.media_type = "plain-text".to_owned();
        assert!(matches!(
            validate_transform_input(&invalid_media_type),
            Err(ComponentHostError::Contract(_))
        ));

        let mut invalid_options = valid;
        invalid_options.options_json = "{".to_owned();
        assert!(matches!(
            validate_transform_input(&invalid_options),
            Err(ComponentHostError::Contract(_))
        ));

        let invalid_diagnostics =
            build_bindings::exports::pliego::build::transform::TransformOutput {
                media_type: "text/plain".to_owned(),
                bytes: b"hello".to_vec(),
                diagnostics_json: "{}".to_owned(),
            };
        assert!(matches!(
            validate_transform_output(&invalid_diagnostics),
            Err(ComponentHostError::Contract(_))
        ));
    }

    #[test]
    fn build_transform_output_digest_binds_media_body_and_diagnostics() {
        let output = build_bindings::exports::pliego::build::transform::TransformOutput {
            media_type: "text/plain".to_owned(),
            bytes: b"hello".to_vec(),
            diagnostics_json: "[]".to_owned(),
        };
        let baseline = hash_transform_output(&output);

        let mut changed_media_type = output.clone();
        changed_media_type.media_type = "text/html".to_owned();
        assert_ne!(baseline, hash_transform_output(&changed_media_type));

        let mut changed_body = output.clone();
        changed_body.bytes = b"world".to_vec();
        assert_ne!(baseline, hash_transform_output(&changed_body));

        let mut changed_diagnostics = output;
        changed_diagnostics.diagnostics_json = "[{}]".to_owned();
        assert_ne!(baseline, hash_transform_output(&changed_diagnostics));
    }
}
