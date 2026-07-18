// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use wit_parser::Resolve;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WitPackageReport {
    pub package: String,
    pub interfaces: Vec<String>,
    pub worlds: Vec<String>,
}

pub fn validate_wit_package(path: &Path) -> Result<WitPackageReport, String> {
    let mut resolve = Resolve::default();
    let (package_id, _) = resolve
        .push_dir(path)
        .map_err(|error| format!("invalid WIT package {}: {error:#}", path.display()))?;
    let package = &resolve.packages[package_id];
    let mut interfaces = package
        .interfaces
        .keys()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut worlds = package
        .worlds
        .keys()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    interfaces.sort();
    worlds.sort();
    Ok(WitPackageReport {
        package: package.name.to_string(),
        interfaces,
        worlds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_normative_wit_packages_parse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("wit");
        let expected = [
            "build",
            "component",
            "deploy",
            "diagnostics",
            "effects",
            "http",
            "manifest",
        ];
        for name in expected {
            let report = validate_wit_package(&root.join(name)).unwrap();
            assert!(report.package.starts_with("pliego:"));
            assert!(!report.worlds.is_empty() || !report.interfaces.is_empty());
        }
    }
}
