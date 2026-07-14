// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Lifecycle-scoped ESM adapter islands for external browser libraries.

use pliego_dom::{IntoView, View, el};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LOADER_PATH: &str = "assets/pliego-adapters.js";
pub const POLICY_BOOTSTRAP_PATH: &str = "assets/pliego-policy.js";
/// Stable browser contract implemented by this crate.
pub const ADAPTER_API_VERSION: u16 = 1;
/// Maximum serialized props embedded in one adapter island.
pub const MAX_PROPS_BYTES: usize = 32_768;

pub const LOADER_JS: &str = include_str!("runtime-v1.js");
pub const POLICY_BOOTSTRAP_JS: &str = concat!(
    "const r=document.documentElement,t=['universal','lite','balanced','signature'],",
    "q=globalThis.__PLIEGO_REQUESTED_TIER__,s=Boolean(globalThis.navigator?.connection?.saveData),",
    "m=globalThis.__PLIEGO_REQUESTED_MOTION__==='reduced'||",
    "Boolean(globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches),",
    "p=s?'universal':t.includes(q)?q:t.includes(r.dataset.pliegoTier)?",
    "r.dataset.pliegoTier:'balanced';r.dataset.pliegoTier=p;",
    "r.dataset.pliegoMotion=m?'reduced':'default';",
    "globalThis.__PLIEGO_ACTIVE_TIER__=p;\n",
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadTrigger {
    Immediate,
    Visible,
    Idle,
    Interaction,
}

impl LoadTrigger {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Visible => "visible",
            Self::Idle => "idle",
            Self::Interaction => "interaction",
        }
    }
}

/// Device budget selected for an adapter island.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum PerformanceTier {
    Universal,
    Lite,
    #[default]
    Balanced,
    Signature,
}

impl PerformanceTier {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Universal => "universal",
            Self::Lite => "lite",
            Self::Balanced => "balanced",
            Self::Signature => "signature",
        }
    }
}

/// Browser resources an adapter intends to consume.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum AdapterCapability {
    Dom,
    Motion,
    SmoothScroll,
    Audio,
    Video,
    WebGl,
    HighFrequencyRaf,
    WebGpu,
}

impl AdapterCapability {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Dom => "dom",
            Self::Motion => "motion",
            Self::SmoothScroll => "smooth-scroll",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::WebGl => "webgl",
            Self::HighFrequencyRaf => "high-frequency-raf",
            Self::WebGpu => "webgpu",
        }
    }
}

/// How the runtime handles the user's reduced-motion preference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum MotionPolicy {
    #[default]
    Auto,
    Full,
    Reduce,
    SkipWhenReduced,
}

impl MotionPolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
            Self::Reduce => "reduce",
            Self::SkipWhenReduced => "skip",
        }
    }
}

/// How an island behaves when the browser requests reduced network usage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum DataPolicy {
    #[default]
    Auto,
    Allow,
    SkipOnSaveData,
}

impl DataPolicy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Allow => "allow",
            Self::SkipOnSaveData => "skip",
        }
    }
}

/// Declarative admission policy evaluated before a plugin module is imported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterPolicy {
    min_tier: PerformanceTier,
    motion: MotionPolicy,
    data: DataPolicy,
    capabilities: BTreeSet<AdapterCapability>,
}

impl Default for AdapterPolicy {
    fn default() -> Self {
        Self {
            min_tier: PerformanceTier::Universal,
            motion: MotionPolicy::Auto,
            data: DataPolicy::Auto,
            capabilities: BTreeSet::new(),
        }
    }
}

impl AdapterPolicy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn min_tier(mut self, tier: PerformanceTier) -> Self {
        self.min_tier = tier;
        self
    }

    #[must_use]
    pub fn motion(mut self, policy: MotionPolicy) -> Self {
        self.motion = policy;
        self
    }

    #[must_use]
    pub fn data(mut self, policy: DataPolicy) -> Self {
        self.data = policy;
        self
    }

    #[must_use]
    pub fn capability(mut self, capability: AdapterCapability) -> Self {
        self.capabilities.insert(capability);
        self
    }
}

#[derive(Debug)]
pub enum AdapterError {
    Invalid(String),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Bundler(String),
    Json(serde_json::Error),
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::Bundler(message) => formatter.write_str(message),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json(source) => write!(formatter, "cannot serialize adapter props: {source}"),
        }
    }
}

impl std::error::Error for AdapterError {}

pub struct AdapterIsland {
    id: String,
    module: String,
    trigger: LoadTrigger,
    policy: AdapterPolicy,
    props: Map<String, Value>,
    child: Option<View>,
}

impl AdapterIsland {
    pub fn new(id: impl Into<String>, module: impl Into<String>) -> Result<Self, AdapterError> {
        let id = id.into();
        validate_id(&id)?;
        let module = module.into();
        validate_module_path(&module)?;
        Ok(Self {
            id,
            module,
            trigger: LoadTrigger::Visible,
            policy: AdapterPolicy::default(),
            props: Map::new(),
            child: None,
        })
    }

    #[must_use]
    pub fn trigger(mut self, trigger: LoadTrigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Replace the complete capability, tier, motion, and data policy.
    #[must_use]
    pub fn policy(mut self, policy: AdapterPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Require a minimum runtime performance tier.
    #[must_use]
    pub fn min_tier(mut self, tier: PerformanceTier) -> Self {
        self.policy.min_tier = tier;
        self
    }

    /// Declare a browser resource the adapter intends to consume.
    #[must_use]
    pub fn capability(mut self, capability: AdapterCapability) -> Self {
        self.policy.capabilities.insert(capability);
        self
    }

    /// Select how reduced-motion preferences affect this island.
    #[must_use]
    pub fn motion_policy(mut self, policy: MotionPolicy) -> Self {
        self.policy.motion = policy;
        self
    }

    /// Select how Save-Data affects this island.
    #[must_use]
    pub fn data_policy(mut self, policy: DataPolicy) -> Self {
        self.policy.data = policy;
        self
    }

    pub fn prop(
        mut self,
        key: impl Into<String>,
        value: impl Into<Value>,
    ) -> Result<Self, AdapterError> {
        let key = key.into();
        validate_id(&key)?;
        self.props.insert(key, value.into());
        Ok(self)
    }

    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(child.into_view());
        self
    }

    pub fn into_view(self) -> Result<View, AdapterError> {
        let props = serde_json::to_string(&self.props).map_err(AdapterError::Json)?;
        if props.len() > MAX_PROPS_BYTES {
            return Err(AdapterError::Invalid(format!(
                "adapter props exceed {MAX_PROPS_BYTES} bytes"
            )));
        }
        let capabilities = self
            .policy
            .capabilities
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let mut element = el("pliego-adapter")
            .attr("data-pliego-api", ADAPTER_API_VERSION.to_string())
            .attr("data-pliego-id", self.id)
            .attr("data-pliego-module", self.module)
            .attr("data-pliego-trigger", self.trigger.as_str())
            .attr("data-pliego-min-tier", self.policy.min_tier.as_str())
            .attr("data-pliego-motion", self.policy.motion.as_str())
            .attr("data-pliego-data", self.policy.data.as_str())
            .attr("data-pliego-props", props)
            .child(self.child.unwrap_or_else(|| View::Fragment(Vec::new())));
        if !capabilities.is_empty() {
            element = element.attr("data-pliego-capabilities", capabilities);
        }
        Ok(element.into_view())
    }
}

pub struct EsbuildBundler {
    executable: PathBuf,
}

impl EsbuildBundler {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// Resolve the platform-specific npm executable from one workspace root.
    pub fn from_workspace(workspace: impl AsRef<Path>) -> Self {
        let executable = if cfg!(windows) {
            "esbuild.cmd"
        } else {
            "esbuild"
        };
        Self::new(
            workspace
                .as_ref()
                .join("node_modules/.bin")
                .join(executable),
        )
    }

    pub fn bundle(
        &self,
        entry: impl AsRef<Path>,
        asset_id: &str,
    ) -> Result<BundledModule, AdapterError> {
        validate_id(asset_id)?;
        let entry = entry.as_ref();
        let output = Command::new(&self.executable)
            .arg(entry)
            .arg("--bundle")
            .arg("--format=esm")
            .arg("--platform=browser")
            .arg("--target=es2020")
            .arg("--minify")
            .arg("--legal-comments=none")
            .output()
            .map_err(|source| AdapterError::Io {
                path: self.executable.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(AdapterError::Bundler(format!(
                "esbuild failed for {}: {}",
                entry.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let bytes = output.stdout;
        if bytes.is_empty() {
            return Err(AdapterError::Bundler(format!(
                "esbuild produced no bytes for {}",
                entry.display()
            )));
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(BundledModule {
            path: format!("assets/{asset_id}.{}.js", &sha256[..16]),
            sha256,
            bytes,
        })
    }
}

pub struct BundledModule {
    pub path: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

pub fn loader_bytes() -> Vec<u8> {
    LOADER_JS.as_bytes().to_vec()
}

/// Emit the pre-loader policy bridge used by adaptive adapter admission.
pub fn policy_bootstrap_bytes() -> Vec<u8> {
    POLICY_BOOTSTRAP_JS.as_bytes().to_vec()
}

pub fn wasm_bootstrap(module: &str) -> Result<Vec<u8>, AdapterError> {
    if !module.starts_with("./")
        || !module.is_ascii()
        || module.len() > 128
        || module.contains("..")
        || module[2..].contains('/')
        || module.contains(['\\', '%', '?', '#', '\'', '"', '\n', '\r'])
        || module
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || !module.ends_with(".js")
    {
        return Err(AdapterError::Invalid(format!(
            "WASM bootstrap module must be a sibling JavaScript asset: {module:?}"
        )));
    }
    Ok(format!("import init from '{module}';init();\n").into_bytes())
}

fn validate_id(value: &str) -> Result<(), AdapterError> {
    let mut characters = value.chars();
    let start = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic());
    let tail = characters
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if value.len() > 128 || !start || !tail {
        return Err(AdapterError::Invalid(format!(
            "invalid adapter identifier: {value:?}"
        )));
    }
    Ok(())
}

fn validate_module_path(module: &str) -> Result<(), AdapterError> {
    if !module.starts_with("/assets/")
        || !module.is_ascii()
        || module.contains("..")
        || module.contains('\\')
        || module.contains(['%', '?', '#'])
        || module.contains("//")
        || !module
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        || module
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || !module.ends_with(".js")
    {
        return Err(AdapterError::Invalid(format!(
            "adapter module must be a safe immutable /assets/*.js path: {module:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_bundler_uses_the_platform_executable() {
        let bundler = EsbuildBundler::from_workspace(Path::new("workspace"));
        let expected = if cfg!(windows) {
            Path::new("workspace/node_modules/.bin/esbuild.cmd")
        } else {
            Path::new("workspace/node_modules/.bin/esbuild")
        };
        assert_eq!(bundler.executable, expected);
    }
    use pliego_dom::render_html;

    #[test]
    fn adapter_contract_is_deterministic_and_escaped() {
        let view = AdapterIsland::new("visit-reveal", "/assets/visit.1234.js")
            .unwrap()
            .prop("distance", 24)
            .unwrap()
            .child(el("h1").child("Visit"))
            .into_view()
            .unwrap();
        assert_eq!(
            render_html(&view),
            r#"<pliego-adapter data-pliego-api="1" data-pliego-id="visit-reveal" data-pliego-module="/assets/visit.1234.js" data-pliego-trigger="visible" data-pliego-min-tier="universal" data-pliego-motion="auto" data-pliego-data="auto" data-pliego-props="{&quot;distance&quot;:24}"><h1>Visit</h1></pliego-adapter>"#
        );
    }

    #[test]
    fn policy_is_versioned_and_capabilities_are_sorted() {
        let view = AdapterIsland::new("scene", "/assets/scene.1234.js")
            .unwrap()
            .trigger(LoadTrigger::Interaction)
            .min_tier(PerformanceTier::Lite)
            .capability(AdapterCapability::WebGl)
            .capability(AdapterCapability::Motion)
            .motion_policy(MotionPolicy::SkipWhenReduced)
            .data_policy(DataPolicy::SkipOnSaveData)
            .into_view()
            .unwrap();
        let html = render_html(&view);
        assert!(html.contains(r#"data-pliego-api="1""#));
        assert!(html.contains(r#"data-pliego-trigger="interaction""#));
        assert!(html.contains(r#"data-pliego-min-tier="lite""#));
        assert!(html.contains(r#"data-pliego-motion="skip""#));
        assert!(html.contains(r#"data-pliego-data="skip""#));
        assert!(html.contains(r#"data-pliego-capabilities="motion,webgl""#));
    }

    #[test]
    fn adapter_rejects_mutable_or_external_module_paths() {
        assert!(AdapterIsland::new("scene", "https://example.com/scene.js").is_err());
        assert!(AdapterIsland::new("scene", "/assets/../secret.js").is_err());
        assert!(AdapterIsland::new("scene", "/assets/%2e%2e/secret.js").is_err());
        assert!(AdapterIsland::new("scene", "/assets/scene.js?mutable=1").is_err());
        assert!(AdapterIsland::new("scene", "/assets/scene.mjs").is_err());
        assert!(AdapterIsland::new("scene", "/assets/escena-ñ.js").is_err());
        assert!(AdapterIsland::new("scene", "/assets//scene.js").is_err());
        assert!(AdapterIsland::new("scene", "/assets/scene'quoted.js").is_err());
    }

    #[test]
    fn adapter_rejects_oversized_props_before_rendering() {
        let island = AdapterIsland::new("scene", "/assets/scene.1234.js")
            .unwrap()
            .prop("payload", "x".repeat(MAX_PROPS_BYTES))
            .unwrap();
        assert!(matches!(island.into_view(), Err(AdapterError::Invalid(_))));
    }

    #[test]
    fn loader_exposes_the_v1_lifecycle_and_runtime_guards() {
        assert!(LOADER_JS.contains("export function createAdapterRuntime"));
        assert!(LOADER_JS.contains("async function performUpdate"));
        assert!(LOADER_JS.contains("function update"));
        assert!(LOADER_JS.contains("function unmount"));
        assert!(LOADER_JS.contains("pliego:cleanup-error"));
        assert!(LOADER_JS.contains("safe same-origin /assets/ URL"));
        assert!(LOADER_JS.contains("prefers-reduced-motion: reduce"));
        assert!(LOADER_JS.contains("connection?.saveData"));
    }

    #[test]
    fn policy_bootstrap_resolves_requested_tier_before_adapter_loading() {
        assert!(POLICY_BOOTSTRAP_JS.contains("__PLIEGO_REQUESTED_TIER__"));
        assert!(POLICY_BOOTSTRAP_JS.contains("__PLIEGO_REQUESTED_MOTION__"));
        assert!(POLICY_BOOTSTRAP_JS.contains("dataset.pliegoTier=p"));
        assert!(POLICY_BOOTSTRAP_JS.contains("__PLIEGO_ACTIVE_TIER__=p"));
    }

    #[test]
    fn wasm_bootstrap_is_framework_owned_and_rejects_escape_paths() {
        assert_eq!(
            wasm_bootstrap("./client.js").unwrap(),
            b"import init from './client.js';init();\n"
        );
        assert!(wasm_bootstrap("../client.js").is_err());
        assert!(wasm_bootstrap("https://example.com/client.js").is_err());
        assert!(wasm_bootstrap("./nested/client.js").is_err());
        assert!(wasm_bootstrap("./%2e%2e-client.js").is_err());
        assert!(wasm_bootstrap("./client?debug=.js").is_err());
    }
}
