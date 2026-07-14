use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    Es,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::En, Self::Es];

    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
        }
    }

    pub const fn language_tag(self) -> &'static str {
        match self {
            Self::En => "en-US",
            Self::Es => "es-ES",
        }
    }

    pub const fn is_spanish(self) -> bool {
        matches!(self, Self::Es)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Localized<T> {
    pub en: T,
    pub es: T,
}

pub type Bilingual = Localized<String>;

impl Localized<String> {
    pub fn text(&self, locale: Locale) -> &str {
        match locale {
            Locale::En => &self.en,
            Locale::Es => &self.es,
        }
    }
}

pub fn locale_path(locale: Locale, path: &str) -> String {
    let normalized = if path == "/" {
        String::new()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    if locale.is_spanish() {
        format!(
            "/es{}",
            if normalized.is_empty() {
                "/"
            } else {
                &normalized
            }
        )
    } else if normalized.is_empty() {
        "/".to_owned()
    } else {
        normalized
    }
}

pub fn alternate_path(locale: Locale, pathname: &str) -> String {
    if pathname == "/404.html" {
        return if locale.is_spanish() {
            "/".into()
        } else {
            "/es/".into()
        };
    }
    if locale.is_spanish() {
        pathname
            .strip_prefix("/es")
            .filter(|path| path.is_empty() || path.starts_with('/'))
            .map(|path| if path.is_empty() { "/" } else { path })
            .unwrap_or(pathname)
            .to_owned()
    } else if pathname == "/" {
        "/es/".into()
    } else {
        format!("/es{pathname}")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum PageType {
    #[serde(rename = "web-page")]
    Standard,
    #[serde(rename = "about-page")]
    About,
    #[serde(rename = "collection-page")]
    Collection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RobotsPolicy {
    IndexFollow,
    NoindexNofollow,
}

impl RobotsPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IndexFollow => "index, follow, max-image-preview:large, max-snippet:-1",
            Self::NoindexNofollow => "noindex, nofollow",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteManifest {
    pub site_origin: String,
    pub not_found: String,
    pub routes: Vec<RouteRecord>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteRecord {
    pub path: String,
    pub page: String,
    pub page_type: PageType,
    pub robots: RobotsPolicy,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeoConfig {
    pub origin: String,
    pub site_name: String,
    pub legal_name: String,
    pub logo: String,
    pub logo_width: u16,
    pub logo_height: u16,
    pub location: String,
    pub default_image: String,
    pub contact_email: String,
    pub organization_description: Bilingual,
    pub website_description: Bilingual,
    pub critical_assets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageMetadata {
    pub key: String,
    pub title: Bilingual,
    pub description: Bilingual,
    pub image: String,
    pub image_alt: Bilingual,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationItem {
    pub key: String,
    pub label: Bilingual,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceCopy {
    pub label: Bilingual,
    pub system: Bilingual,
    pub light: Bilingual,
    pub dark: Bilingual,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuCopy {
    pub open: Bilingual,
    pub close: Bilingual,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FooterCopy {
    pub line: Bilingual,
    pub endorsement: Bilingual,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellCopy {
    pub descriptor: Bilingual,
    pub tagline: Bilingual,
    pub skip: Bilingual,
    pub navigation: Vec<NavigationItem>,
    pub appearance: AppearanceCopy,
    pub menu: MenuCopy,
    pub footer: FooterCopy,
}

pub struct SiteContent {
    routes: RouteManifest,
    seo: SeoConfig,
    shell: ShellCopy,
    metadata: BTreeMap<String, PageMetadata>,
}

impl SiteContent {
    pub fn load() -> Result<Self, ContentError> {
        Self::load_from(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("content"))
    }

    pub fn load_from(root: impl Into<PathBuf>) -> Result<Self, ContentError> {
        let root = root.into();
        let routes: RouteManifest = read_toml(&root.join("routes/site.toml"))?;
        let seo: SeoConfig = read_toml(&root.join("seo/site.toml"))?;
        let shell: ShellCopy = read_json(&root.join("copy/shell.json"))?;
        let mut metadata = BTreeMap::new();
        for route in &routes.routes {
            let item: PageMetadata =
                read_toml(&root.join(format!("seo-pages/{}.toml", route.page)))?;
            if item.key != route.page {
                return Err(ContentError::Contract(format!(
                    "metadata key {} does not match route page {}",
                    item.key, route.page
                )));
            }
            metadata.insert(item.key.clone(), item);
        }
        let content = Self {
            routes,
            seo,
            shell,
            metadata,
        };
        content.validate()?;
        Ok(content)
    }

    fn validate(&self) -> Result<(), ContentError> {
        if self.routes.site_origin != "https://pliegors.dev"
            || self.seo.origin != self.routes.site_origin
        {
            return Err(ContentError::Contract(
                "canonical origin must be https://pliegors.dev".into(),
            ));
        }
        if self.seo.contact_email != "hello@pliegors.dev" {
            return Err(ContentError::Contract(
                "contact email must be hello@pliegors.dev".into(),
            ));
        }
        if self.routes.not_found != "/404.html" {
            return Err(ContentError::Contract(
                "not-found output must remain /404.html".into(),
            ));
        }
        if self.routes.routes.is_empty()
            || self.shell.navigation.is_empty()
            || self.seo.critical_assets.is_empty()
            || self
                .seo
                .critical_assets
                .iter()
                .any(|asset| !asset.starts_with('/') || asset.contains(".."))
        {
            return Err(ContentError::Contract(
                "route and navigation registries cannot be empty".into(),
            ));
        }
        for route in &self.routes.routes {
            if !route.path.starts_with('/')
                || route.path.contains("//")
                || !self.metadata.contains_key(&route.page)
            {
                return Err(ContentError::Contract(format!(
                    "invalid route contract for {}",
                    route.path
                )));
            }
        }
        Ok(())
    }

    pub fn routes(&self) -> &RouteManifest {
        &self.routes
    }
    pub fn seo(&self) -> &SeoConfig {
        &self.seo
    }
    pub fn shell(&self) -> &ShellCopy {
        &self.shell
    }
    pub fn metadata(&self, page: &str) -> Option<&PageMetadata> {
        self.metadata.get(page)
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ContentError> {
    let source = std::fs::read_to_string(path)
        .map_err(|source| ContentError::Read(path.to_owned(), source))?;
    serde_json::from_str(&source)
        .map_err(|source| ContentError::Json(path.to_owned(), source.to_string()))
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ContentError> {
    let source = std::fs::read_to_string(path)
        .map_err(|source| ContentError::Read(path.to_owned(), source))?;
    toml::from_str(&source)
        .map_err(|source| ContentError::Toml(path.to_owned(), source.to_string()))
}

#[derive(Debug)]
pub enum ContentError {
    Read(PathBuf, std::io::Error),
    Json(PathBuf, String),
    Toml(PathBuf, String),
    Contract(String),
}

impl fmt::Display for ContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, source) => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Json(path, source) => {
                write!(formatter, "invalid JSON in {}: {source}", path.display())
            }
            Self::Toml(path, source) => {
                write!(formatter, "invalid TOML in {}: {source}", path.display())
            }
            Self::Contract(message) => {
                write!(formatter, "invalid PliegoRS site content: {message}")
            }
        }
    }
}

impl std::error::Error for ContentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_contract_is_canonical_and_bilingual() {
        let content = SiteContent::load().unwrap();
        assert_eq!(content.seo().origin, "https://pliegors.dev");
        assert_eq!(content.seo().contact_email, "hello@pliegors.dev");
        assert!(
            content
                .routes()
                .routes
                .iter()
                .all(|route| content.metadata(&route.page).is_some())
        );
    }

    #[test]
    fn locale_routes_round_trip() {
        assert_eq!(locale_path(Locale::Es, "/docs"), "/es/docs");
        assert_eq!(alternate_path(Locale::Es, "/es/docs"), "/docs");
    }
}
