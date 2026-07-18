// SPDX-License-Identifier: Apache-2.0

use crate::{Capability, ExtensionManifest};
use std::collections::BTreeSet;
use wasmparser::{
    ComponentExternalKind, ComponentTypeRef, Parser, Payload, Validator, WasmFeatures,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentInspection {
    pub imports: Vec<String>,
    pub exports: Vec<String>,
    pub required_capabilities: Vec<Capability>,
}

impl ComponentInspection {
    pub fn verify_manifest(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        if self.imports != manifest.imports {
            return Err(format!(
                "component imports do not match manifest: binary={:?}, manifest={:?}",
                self.imports, manifest.imports
            ));
        }
        if self.exports != manifest.exports {
            return Err(format!(
                "component exports do not match manifest: binary={:?}, manifest={:?}",
                self.exports, manifest.exports
            ));
        }
        for capability in &self.required_capabilities {
            if manifest.capabilities.binary_search(capability).is_err() {
                return Err(format!(
                    "component import requires undeclared capability `{}`",
                    capability.as_str()
                ));
            }
        }
        Ok(())
    }
}

pub fn inspect_component(bytes: &[u8]) -> Result<ComponentInspection, String> {
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("component exceeds the 128 MiB inspection ceiling".to_owned());
    }
    Validator::new_with_features(WasmFeatures::default())
        .validate_all(bytes)
        .map_err(|error| format!("invalid WebAssembly component: {error}"))?;

    let mut imports = BTreeSet::new();
    let mut exports = BTreeSet::new();
    let mut capabilities = BTreeSet::new();
    let mut depth = 0_u32;
    for payload in Parser::new(0).parse_all(bytes) {
        let payload =
            payload.map_err(|error| format!("cannot inspect WebAssembly component: {error}"))?;
        match payload {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| "component nesting depth overflowed".to_owned())?;
                continue;
            }
            Payload::End(_) if depth > 0 => {
                depth -= 1;
                continue;
            }
            _ if depth > 0 => continue,
            Payload::ComponentImportSection(section) => {
                for import in section {
                    let import =
                        import.map_err(|error| format!("invalid component import: {error}"))?;
                    if matches!(import.ty, ComponentTypeRef::Type(_)) {
                        continue;
                    }
                    let name = import.name.0.to_owned();
                    classify_import(&name, &mut capabilities)?;
                    imports.insert(name);
                }
            }
            Payload::ComponentExportSection(section) => {
                for export in section {
                    let export =
                        export.map_err(|error| format!("invalid component export: {error}"))?;
                    if export.kind == ComponentExternalKind::Type {
                        continue;
                    }
                    exports.insert(export.name.0.to_owned());
                }
            }
            _ => {}
        }
    }
    Ok(ComponentInspection {
        imports: imports.into_iter().collect(),
        exports: exports.into_iter().collect(),
        required_capabilities: capabilities.into_iter().collect(),
    })
}

fn classify_import(name: &str, capabilities: &mut BTreeSet<Capability>) -> Result<(), String> {
    let package = name.split('@').next().unwrap_or(name);
    if package.starts_with("wasi:filesystem/") {
        capabilities.insert(Capability::FilesystemRead);
        capabilities.insert(Capability::FilesystemWrite);
    } else if package.starts_with("wasi:sockets/") {
        capabilities.insert(Capability::Network);
    } else if package.starts_with("wasi:cli/environment") {
        capabilities.insert(Capability::Environment);
    } else if package.starts_with("wasi:clocks/") {
        capabilities.insert(Capability::Clock);
    } else if package.starts_with("wasi:random/") {
        capabilities.insert(Capability::Random);
    } else if package.starts_with("wasi:http/") {
        capabilities.insert(Capability::Http);
    } else if package.starts_with("pliego:effects/broker") {
        capabilities.insert(Capability::EffectBroker);
    } else if package.starts_with("wasi:io/") || package.starts_with("pliego:diagnostics/sink") {
    } else {
        return Err(format!(
            "component import `{name}` has no preview capability classification"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_empty_component_has_no_ambient_power() {
        let bytes = wat::parse_str("(component)").unwrap();
        let report = inspect_component(&bytes).unwrap();
        assert!(report.imports.is_empty());
        assert!(report.exports.is_empty());
        assert!(report.required_capabilities.is_empty());
    }

    #[test]
    fn environment_import_is_classified_before_execution() {
        let bytes = wat::parse_str(
            r#"(component
                (type $get-env (func (result string)))
                (import "wasi:cli/environment@0.2.4" (func $get-env (type $get-env)))
            )"#,
        )
        .unwrap();
        let report = inspect_component(&bytes).unwrap();
        assert_eq!(report.imports, ["wasi:cli/environment@0.2.4"]);
        assert_eq!(report.required_capabilities, [Capability::Environment]);
    }

    #[test]
    fn filesystem_import_is_conservatively_read_write() {
        let bytes = wat::parse_str(
            r#"(component
                (type $probe (func))
                (import "wasi:filesystem/types@0.2.4" (func $probe (type $probe)))
            )"#,
        )
        .unwrap();
        let report = inspect_component(&bytes).unwrap();
        assert_eq!(
            report.required_capabilities,
            [Capability::FilesystemRead, Capability::FilesystemWrite]
        );
    }
}
