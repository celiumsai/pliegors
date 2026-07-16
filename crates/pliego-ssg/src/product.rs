// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Framework-owned product topology for build adapters.

use std::collections::BTreeSet;
use std::fmt;

const MAX_ITEMS: usize = 65_535;
const MAX_ID_BYTES: usize = 256;
const MAX_PATH_BYTES: usize = 4 * 1024;

/// One product component and the Cargo source units that define its build-time assets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductComponent {
    id: String,
    source_units: Vec<String>,
}

/// One product route, its components, and the resumable islands it renders.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductRoute {
    id: String,
    path: String,
    components: Vec<String>,
    islands: Vec<String>,
}

/// One resumable product island and its component set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductIsland {
    id: String,
    name: String,
    components: Vec<String>,
}

/// Complete framework-owned application registry consumed by build adapters.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProductRegistry {
    components: Vec<ProductComponent>,
    routes: Vec<ProductRoute>,
    islands: Vec<ProductIsland>,
}

/// Failure to validate a product registry before an adapter consumes it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductRegistryError {
    message: String,
}

impl ProductComponent {
    /// Creates a component without any registered Cargo source units.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source_units: Vec::new(),
        }
    }

    /// Registers one project-relative Cargo source unit owned by this component.
    #[must_use]
    pub fn source_unit(mut self, path: impl Into<String>) -> Self {
        self.source_units.push(path.into());
        self
    }

    /// Returns the stable product component ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the complete registered source-unit set in declaration order.
    #[must_use]
    pub fn source_units(&self) -> &[String] {
        &self.source_units
    }
}

impl ProductRoute {
    /// Creates a route with no registered components or islands.
    pub fn new(id: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            components: Vec::new(),
            islands: Vec::new(),
        }
    }

    /// Registers one component used directly by this route.
    #[must_use]
    pub fn component(mut self, id: impl Into<String>) -> Self {
        self.components.push(id.into());
        self
    }

    /// Registers one resumable island rendered by this route.
    #[must_use]
    pub fn island(mut self, id: impl Into<String>) -> Self {
        self.islands.push(id.into());
        self
    }

    /// Returns the stable route ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the application route path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns direct route component IDs in declaration order.
    #[must_use]
    pub fn components(&self) -> &[String] {
        &self.components
    }

    /// Returns rendered island IDs in declaration order.
    #[must_use]
    pub fn islands(&self) -> &[String] {
        &self.islands
    }
}

impl ProductIsland {
    /// Creates one resumable island without registered components.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            components: Vec::new(),
        }
    }

    /// Registers one component owned by this island.
    #[must_use]
    pub fn component(mut self, id: impl Into<String>) -> Self {
        self.components.push(id.into());
        self
    }

    /// Returns the stable island ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the rendered island name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns island component IDs in declaration order.
    #[must_use]
    pub fn components(&self) -> &[String] {
        &self.components
    }
}

impl ProductRegistry {
    /// Creates an empty product registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            components: Vec::new(),
            routes: Vec::new(),
            islands: Vec::new(),
        }
    }

    /// Adds one product component.
    #[must_use]
    pub fn component(mut self, component: ProductComponent) -> Self {
        self.components.push(component);
        self
    }

    /// Adds one product route.
    #[must_use]
    pub fn route(mut self, route: ProductRoute) -> Self {
        self.routes.push(route);
        self
    }

    /// Adds one resumable island.
    #[must_use]
    pub fn island(mut self, island: ProductIsland) -> Self {
        self.islands.push(island);
        self
    }

    /// Returns registered components in declaration order.
    #[must_use]
    pub fn components(&self) -> &[ProductComponent] {
        &self.components
    }

    /// Returns registered routes in declaration order.
    #[must_use]
    pub fn routes(&self) -> &[ProductRoute] {
        &self.routes
    }

    /// Returns registered islands in declaration order.
    #[must_use]
    pub fn islands(&self) -> &[ProductIsland] {
        &self.islands
    }

    /// Validates IDs, paths, uniqueness, references, and defensive limits.
    ///
    /// # Errors
    ///
    /// Returns an error when the registry is incomplete, ambiguous, unsafe, or oversized.
    pub fn validate(&self) -> Result<(), ProductRegistryError> {
        bounded(
            self.components.len() + self.routes.len() + self.islands.len() <= MAX_ITEMS,
            "product registry exceeds 65,535 top-level items",
        )?;
        bounded(
            !self.components.is_empty(),
            "product registry has no components",
        )?;
        bounded(!self.routes.is_empty(), "product registry has no routes")?;

        let mut component_ids = BTreeSet::new();
        let mut source_count = 0_usize;
        for component in &self.components {
            validate_id(&component.id, "component ID")?;
            bounded(
                component_ids.insert(component.id.as_str()),
                format!("duplicate product component `{}`", component.id),
            )?;
            bounded(
                !component.source_units.is_empty(),
                format!("product component `{}` has no source units", component.id),
            )?;
            let mut source_units = BTreeSet::new();
            for source in &component.source_units {
                validate_source_unit(source)?;
                bounded(
                    source_units.insert(source.as_str()),
                    format!(
                        "component `{}` repeats source unit `{source}`",
                        component.id
                    ),
                )?;
                source_count = source_count
                    .checked_add(1)
                    .ok_or_else(|| invalid("product source-unit count overflow"))?;
            }
        }
        bounded(
            source_count <= MAX_ITEMS,
            "product registry exceeds 65,535 source-unit registrations",
        )?;

        let mut island_ids = BTreeSet::new();
        let mut island_names = BTreeSet::new();
        for island in &self.islands {
            validate_id(&island.id, "island ID")?;
            validate_name(&island.name, "island name")?;
            bounded(
                island_ids.insert(island.id.as_str()),
                format!("duplicate product island `{}`", island.id),
            )?;
            bounded(
                island_names.insert(island.name.as_str()),
                format!("duplicate rendered island name `{}`", island.name),
            )?;
            validate_component_references(
                &island.components,
                &component_ids,
                &format!("island `{}`", island.id),
            )?;
        }

        let mut route_ids = BTreeSet::new();
        let mut route_paths = BTreeSet::new();
        for route in &self.routes {
            validate_id(&route.id, "route ID")?;
            validate_route_path(&route.path)?;
            bounded(
                route_ids.insert(route.id.as_str()),
                format!("duplicate product route `{}`", route.id),
            )?;
            bounded(
                route_paths.insert(route.path.as_str()),
                format!("duplicate product route path `{}`", route.path),
            )?;
            validate_component_references(
                &route.components,
                &component_ids,
                &format!("route `{}`", route.id),
            )?;
            let mut seen_islands = BTreeSet::new();
            for island in &route.islands {
                bounded(
                    island_ids.contains(island.as_str()),
                    format!("route `{}` references unknown island `{island}`", route.id),
                )?;
                bounded(
                    seen_islands.insert(island.as_str()),
                    format!("route `{}` repeats island `{island}`", route.id),
                )?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for ProductRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProductRegistryError {}

fn validate_component_references(
    references: &[String],
    component_ids: &BTreeSet<&str>,
    owner: &str,
) -> Result<(), ProductRegistryError> {
    bounded(!references.is_empty(), format!("{owner} has no components"))?;
    let mut seen = BTreeSet::new();
    for component in references {
        bounded(
            component_ids.contains(component.as_str()),
            format!("{owner} references unknown component `{component}`"),
        )?;
        bounded(
            seen.insert(component.as_str()),
            format!("{owner} repeats component `{component}`"),
        )?;
    }
    Ok(())
}

fn validate_id(value: &str, role: &str) -> Result<(), ProductRegistryError> {
    bounded(
        !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value.trim() == value
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte)),
        format!("invalid {role} `{value}`"),
    )
}

fn validate_name(value: &str, role: &str) -> Result<(), ProductRegistryError> {
    bounded(
        !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value.trim() == value
            && !value.chars().any(char::is_control),
        format!("invalid {role} `{value}`"),
    )
}

fn validate_route_path(path: &str) -> Result<(), ProductRegistryError> {
    bounded(
        path.starts_with('/')
            && path.len() <= MAX_PATH_BYTES
            && !path.contains(['\\', '?', '#', '\0'])
            && !path.contains("//")
            && path
                .split('/')
                .skip(1)
                .all(|segment| segment != "." && segment != ".."),
        format!("invalid product route path `{path}`"),
    )
}

fn validate_source_unit(path: &str) -> Result<(), ProductRegistryError> {
    bounded(
        !path.is_empty()
            && path.len() <= MAX_PATH_BYTES
            && path.trim() == path
            && path.ends_with(".rs")
            && !path.starts_with('/')
            && !path.contains(['\\', '\0'])
            && path
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != ".."),
        format!("invalid product source unit `{path}`"),
    )
}

fn bounded(condition: bool, message: impl Into<String>) -> Result<(), ProductRegistryError> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

fn invalid(message: impl Into<String>) -> ProductRegistryError {
    ProductRegistryError {
        message: message.into(),
    }
}

/// Registers a product component at the Cargo source unit containing the invocation.
///
/// The captured `file!()` value is normalized to `/` so adapters receive a portable path without
/// duplicating a handwritten filename beside the component declaration.
#[macro_export]
macro_rules! product_component {
    ($id:expr) => {
        $crate::ProductComponent::new($id).source_unit(file!().replace('\\', "/"))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_registry() -> ProductRegistry {
        ProductRegistry::new()
            .component(ProductComponent::new("app::global").source_unit("src/global.rs"))
            .component(ProductComponent::new("app::counter").source_unit("src/counter.rs"))
            .island(ProductIsland::new("counter", "visit-counter").component("app::counter"))
            .route(
                ProductRoute::new("home", "/")
                    .component("app::global")
                    .island("counter"),
            )
    }

    #[test]
    fn complete_registry_is_accepted() {
        valid_registry().validate().unwrap();
    }

    #[test]
    fn product_component_macro_captures_its_cargo_source() {
        let component = crate::product_component!("app::captured");
        assert_eq!(component.id(), "app::captured");
        assert_eq!(
            component.source_units(),
            &["crates/pliego-ssg/src/product.rs"]
        );
    }

    #[test]
    fn unsafe_sources_and_dangling_references_are_rejected() {
        let unsafe_source = ProductRegistry::new()
            .component(ProductComponent::new("app::global").source_unit("../global.rs"))
            .route(ProductRoute::new("home", "/").component("app::global"));
        assert!(unsafe_source.validate().is_err());

        let dangling = ProductRegistry::new()
            .component(ProductComponent::new("app::global").source_unit("src/global.rs"))
            .route(ProductRoute::new("home", "/").component("app::missing"));
        assert!(dangling.validate().is_err());
    }

    #[test]
    fn duplicates_are_rejected_before_adapter_use() {
        let duplicate = valid_registry()
            .component(ProductComponent::new("app::global").source_unit("src/duplicate.rs"));
        assert!(duplicate.validate().is_err());

        let duplicate_source = ProductRegistry::new()
            .component(
                ProductComponent::new("app::global")
                    .source_unit("src/global.rs")
                    .source_unit("src/global.rs"),
            )
            .route(ProductRoute::new("home", "/").component("app::global"));
        assert!(duplicate_source.validate().is_err());
    }
}
