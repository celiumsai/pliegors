// SPDX-License-Identifier: Apache-2.0

use crate::{OPENSDK_API_VERSION, Plane};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityMatrix {
    pub contract: String,
    pub protocol_version: String,
    pub source: String,
    pub hosts: Vec<HostCompatibility>,
    pub toolchains: Vec<CompatibilityToolchain>,
    pub surfaces: Vec<CompatibilitySurface>,
    pub deprecations: Vec<Deprecation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostCompatibility {
    pub version: String,
    pub api_versions: Vec<String>,
    pub stability: String,
    pub planes: Vec<Plane>,
    pub features: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityToolchain {
    pub id: String,
    pub language: String,
    pub boundary: String,
    pub support: String,
    pub tested_version: String,
    pub checks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilitySurface {
    pub id: String,
    pub protocol: String,
    pub tested_version: String,
    pub checks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Deprecation {
    pub id: String,
    pub state: DeprecationState,
    pub since: String,
    pub earliest_removal: Option<String>,
    pub replacement: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeprecationState {
    Active,
    Deprecated,
    Removed,
}

impl CompatibilityMatrix {
    pub fn validate(&self) -> Result<(), CompatibilityError> {
        if self.contract != "dev.pliegors.sdk-compatibility/v1" {
            return Err(CompatibilityError::new(
                "unsupported compatibility contract",
            ));
        }
        if self.protocol_version != OPENSDK_API_VERSION {
            return Err(CompatibilityError::new(
                "compatibility protocol does not match the OpenSDK API",
            ));
        }
        if self.source != "celiumsai/pliegors" {
            return Err(CompatibilityError::new(
                "compatibility source must be the public PliegoRS repository",
            ));
        }
        require_count(self.hosts.len(), 1, 128, "hosts")?;
        require_count(self.toolchains.len(), 3, 128, "toolchains")?;
        require_count(self.surfaces.len(), 5, 256, "surfaces")?;
        require_count(self.deprecations.len(), 0, 256, "deprecations")?;
        require_sorted_unique_by(&self.hosts, |host| host.version.as_str(), "host versions")?;
        require_sorted_unique_by(
            &self.toolchains,
            |toolchain| toolchain.id.as_str(),
            "toolchains",
        )?;
        require_sorted_unique_by(&self.surfaces, |surface| surface.id.as_str(), "surfaces")?;
        require_sorted_unique_by(
            &self.deprecations,
            |deprecation| deprecation.id.as_str(),
            "deprecations",
        )?;
        for host in &self.hosts {
            require_bounded(&host.version, 64, "host version")?;
            parse_version(&host.version, "host version")?;
            if !matches!(
                host.stability.as_str(),
                "experimental" | "preview" | "stable"
            ) {
                return Err(CompatibilityError::new(
                    "host stability must be experimental, preview, or stable",
                ));
            }
            require_count(host.api_versions.len(), 1, 256, "host API versions")?;
            require_sorted_unique(&host.api_versions, "host API versions")?;
            for version in &host.api_versions {
                require_bounded(version, 128, "host API version")?;
                parse_version(version, "host API version")?;
            }
            require_count(host.features.len(), 0, 256, "host features")?;
            require_sorted_unique(&host.features, "host features")?;
            for feature in &host.features {
                require_slug(feature, "host feature")?;
            }
            require_count(host.planes.len(), 1, 4, "host planes")?;
            require_unique(&host.planes, "host planes")?;
        }
        for toolchain in &self.toolchains {
            require_slug(&toolchain.id, "toolchain id")?;
            require_bounded(&toolchain.language, 128, "toolchain language")?;
            require_bounded(&toolchain.tested_version, 128, "toolchain tested version")?;
            if !matches!(
                toolchain.boundary.as_str(),
                "component-model" | "process-bridge" | "http-bridge"
            ) {
                return Err(CompatibilityError::new("toolchain boundary is unsupported"));
            }
            if !matches!(
                toolchain.support.as_str(),
                "reference" | "conformance" | "experimental"
            ) {
                return Err(CompatibilityError::new(
                    "toolchain support level is unsupported",
                ));
            }
            validate_checks(&toolchain.checks, "toolchain checks")?;
            require_unique(&toolchain.checks, "toolchain checks")?;
        }
        for surface in &self.surfaces {
            require_slug(&surface.id, "surface id")?;
            require_bounded(&surface.tested_version, 128, "surface tested version")?;
            if !matches!(
                surface.protocol.as_str(),
                "browser-component" | "json-rpc" | "mcp"
            ) {
                return Err(CompatibilityError::new(
                    "compatibility surface protocol is unsupported",
                ));
            }
            validate_checks(&surface.checks, "surface checks")?;
            require_unique(&surface.checks, "surface checks")?;
        }
        for deprecation in &self.deprecations {
            deprecation.validate()?;
        }
        Ok(())
    }
}

impl Deprecation {
    fn validate(&self) -> Result<(), CompatibilityError> {
        require_contract_id(&self.id, "deprecation id")?;
        require_bounded(&self.since, 64, "deprecation `since`")?;
        if let Some(removal) = &self.earliest_removal {
            require_bounded(removal, 64, "deprecation `earliestRemoval`")?;
        }
        if let Some(replacement) = &self.replacement {
            require_contract_id(replacement, "deprecation replacement")?;
        }
        let since = parse_version(&self.since, "deprecation `since`")?;
        let earliest = self
            .earliest_removal
            .as_deref()
            .map(|version| parse_version(version, "deprecation `earliestRemoval`"))
            .transpose()?;
        match self.state {
            DeprecationState::Active => {
                if earliest.is_some() || self.replacement.is_some() {
                    return Err(CompatibilityError::new(
                        "active contracts cannot declare removal or replacement",
                    ));
                }
            }
            DeprecationState::Deprecated => {
                let removal = earliest.ok_or_else(|| {
                    CompatibilityError::new(
                        "deprecated contracts require an earliest removal version",
                    )
                })?;
                if removal <= since {
                    return Err(CompatibilityError::new(
                        "earliest removal must be later than the deprecation version",
                    ));
                }
                if self.replacement.as_deref().is_none_or(str::is_empty) {
                    return Err(CompatibilityError::new(
                        "deprecated contracts require a replacement",
                    ));
                }
            }
            DeprecationState::Removed => {
                let removal = earliest.ok_or_else(|| {
                    CompatibilityError::new("removed contracts require their removal version")
                })?;
                if removal < since {
                    return Err(CompatibilityError::new(
                        "removal cannot predate deprecation",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompatibilityError {
    message: String,
}

impl CompatibilityError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompatibilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CompatibilityError {}

fn parse_version(value: &str, label: &str) -> Result<Version, CompatibilityError> {
    Version::parse(value)
        .map_err(|_| CompatibilityError::new(format!("{label} is not valid SemVer: {value}")))
}

fn require_count(
    count: usize,
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<(), CompatibilityError> {
    if !(minimum..=maximum).contains(&count) {
        return Err(CompatibilityError::new(format!(
            "{label} must contain {minimum}-{maximum} entries"
        )));
    }
    Ok(())
}

fn require_bounded(value: &str, maximum: usize, label: &str) -> Result<(), CompatibilityError> {
    if value.is_empty() || value.len() > maximum {
        return Err(CompatibilityError::new(format!(
            "{label} must contain 1-{maximum} bytes"
        )));
    }
    Ok(())
}

fn require_slug(value: &str, label: &str) -> Result<(), CompatibilityError> {
    require_bounded(value, 128, label)?;
    if value.split('-').any(|segment| {
        segment.is_empty()
            || !segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    }) {
        return Err(CompatibilityError::new(format!(
            "{label} must be a lowercase ASCII slug"
        )));
    }
    Ok(())
}

fn require_contract_id(value: &str, label: &str) -> Result<(), CompatibilityError> {
    require_bounded(value, 128, label)?;
    if value.starts_with(['.', '/', '-'])
        || value.ends_with(['.', '/', '-'])
        || value.split(['.', '/', '-']).any(|segment| {
            segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
    {
        return Err(CompatibilityError::new(format!(
            "{label} must be a lowercase ASCII contract id"
        )));
    }
    Ok(())
}

fn validate_checks(values: &[String], label: &str) -> Result<(), CompatibilityError> {
    require_count(values.len(), 0, 256, label)?;
    for value in values {
        require_bounded(value, 128, label)?;
    }
    Ok(())
}

fn require_sorted_unique(values: &[String], label: &str) -> Result<(), CompatibilityError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(CompatibilityError::new(format!(
            "{label} must be sorted and unique"
        )));
    }
    Ok(())
}

fn require_sorted_unique_by<'a, T>(
    values: &'a [T],
    key: impl Fn(&'a T) -> &'a str,
    label: &str,
) -> Result<(), CompatibilityError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        return Err(CompatibilityError::new(format!(
            "{label} must be sorted and unique"
        )));
    }
    Ok(())
}

fn require_unique<T: Ord>(values: &[T], label: &str) -> Result<(), CompatibilityError> {
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(CompatibilityError::new(format!("{label} must be unique")));
    }
    Ok(())
}

pub fn preview_compatibility_matrix() -> CompatibilityMatrix {
    CompatibilityMatrix {
        contract: "dev.pliegors.sdk-compatibility/v1".to_owned(),
        protocol_version: OPENSDK_API_VERSION.to_owned(),
        source: "celiumsai/pliegors".to_owned(),
        hosts: vec![HostCompatibility {
            version: OPENSDK_API_VERSION.to_owned(),
            api_versions: vec![OPENSDK_API_VERSION.to_owned()],
            stability: "preview".to_owned(),
            planes: vec![Plane::Build, Plane::Server, Plane::Browser, Plane::Tooling],
            features: vec![
                "custom-elements-v1".to_owned(),
                "diagnostic-links".to_owned(),
                "mcp-2025-11-25".to_owned(),
            ],
        }],
        toolchains: vec![
            CompatibilityToolchain {
                id: "python-process".to_owned(),
                language: "Python".to_owned(),
                boundary: "process-bridge".to_owned(),
                support: "conformance".to_owned(),
                tested_version: "3.11".to_owned(),
                checks: vec!["transform-equivalence".to_owned()],
            },
            CompatibilityToolchain {
                id: "rust-component".to_owned(),
                language: "Rust".to_owned(),
                boundary: "component-model".to_owned(),
                support: "reference".to_owned(),
                tested_version: "1.86.0".to_owned(),
                checks: vec![
                    "admission".to_owned(),
                    "deny-by-default-runtime".to_owned(),
                    "effect-broker".to_owned(),
                    "runtime-budgets".to_owned(),
                    "transform-equivalence".to_owned(),
                ],
            },
            CompatibilityToolchain {
                id: "rust-process".to_owned(),
                language: "Rust".to_owned(),
                boundary: "process-bridge".to_owned(),
                support: "conformance".to_owned(),
                tested_version: "1.86.0".to_owned(),
                checks: vec!["transform-equivalence".to_owned()],
            },
            CompatibilityToolchain {
                id: "typescript-process".to_owned(),
                language: "TypeScript".to_owned(),
                boundary: "process-bridge".to_owned(),
                support: "conformance".to_owned(),
                tested_version: "Node.js 20 + esbuild 0.28".to_owned(),
                checks: vec!["transform-equivalence".to_owned()],
            },
        ],
        surfaces: vec![
            CompatibilitySurface {
                id: "adapter-lifecycle-v1".to_owned(),
                protocol: "browser-component".to_owned(),
                tested_version: "1".to_owned(),
                checks: vec![
                    "dispose".to_owned(),
                    "hmr".to_owned(),
                    "listener-cleanup".to_owned(),
                    "scope-cleanup".to_owned(),
                    "timer-cleanup".to_owned(),
                ],
            },
            CompatibilitySurface {
                id: "lit-custom-element".to_owned(),
                protocol: "browser-component".to_owned(),
                tested_version: "3.3.3".to_owned(),
                checks: browser_checks(),
            },
            CompatibilitySurface {
                id: "mcp-reference-client".to_owned(),
                protocol: "mcp".to_owned(),
                tested_version: "2025-11-25".to_owned(),
                checks: vec![
                    "initialize".to_owned(),
                    "capability-negotiation".to_owned(),
                    "tools-list".to_owned(),
                    "tools-call".to_owned(),
                ],
            },
            CompatibilitySurface {
                id: "react-custom-element".to_owned(),
                protocol: "browser-component".to_owned(),
                tested_version: "19.2.7".to_owned(),
                checks: browser_checks(),
            },
            CompatibilitySurface {
                id: "svelte-custom-element".to_owned(),
                protocol: "browser-component".to_owned(),
                tested_version: "5.56.6".to_owned(),
                checks: browser_checks(),
            },
        ],
        deprecations: Vec::new(),
    }
}

fn browser_checks() -> Vec<String> {
    vec![
        "mount".to_owned(),
        "update".to_owned(),
        "reduced-motion".to_owned(),
        "hmr".to_owned(),
        "dispose".to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_matrix_is_sorted_unique_and_has_no_hidden_registry() {
        let matrix = preview_compatibility_matrix();
        matrix.validate().unwrap();
        assert_eq!(matrix.protocol_version, OPENSDK_API_VERSION);
        assert_eq!(matrix.hosts.len(), 1);
        assert!(matrix.deprecations.is_empty());
        assert!(matrix.source.contains("pliegors"));
        for host in &matrix.hosts {
            assert!(host.features.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert!(
            matrix
                .toolchains
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );
        assert!(
            matrix
                .surfaces
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );
    }

    #[test]
    fn deprecation_transitions_fail_closed() {
        let mut matrix = preview_compatibility_matrix();
        matrix.deprecations.push(Deprecation {
            id: "build/legacy-transform".to_owned(),
            state: DeprecationState::Deprecated,
            since: "0.1.0-preview.1".to_owned(),
            earliest_removal: None,
            replacement: None,
        });
        assert_eq!(
            matrix.validate().unwrap_err().to_string(),
            "deprecated contracts require an earliest removal version"
        );

        matrix.deprecations[0].earliest_removal = Some("0.2.0".to_owned());
        matrix.deprecations[0].replacement = Some("build/transform".to_owned());
        matrix.validate().unwrap();
    }

    #[test]
    fn compatibility_vocabularies_and_bounds_fail_closed() {
        let mut matrix = preview_compatibility_matrix();
        matrix.toolchains[0].support = "best-effort".to_owned();
        assert_eq!(
            matrix.validate().unwrap_err().to_string(),
            "toolchain support level is unsupported"
        );

        let mut matrix = preview_compatibility_matrix();
        matrix.surfaces[0].protocol = "private-registry".to_owned();
        assert_eq!(
            matrix.validate().unwrap_err().to_string(),
            "compatibility surface protocol is unsupported"
        );
    }
}
