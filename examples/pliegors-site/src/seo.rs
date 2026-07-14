use crate::content::{Locale, PageMetadata, PageType, RobotsPolicy, SeoConfig};
use pliego_ssg::Head;
use serde_json::{Value, json};

pub const SITE_STYLESHEET: &str = "/assets/pliegors.css";
pub const CLIENT_MODULE: &str = "/assets/pliegors_site_boot.js";

pub const THEME_BOOTSTRAP: &str = r#"(() => {
  const key = 'pliegors:theme:v1';
  let theme = 'system';
  try { theme = localStorage.getItem(key) || 'system'; } catch {}
  if (!['system', 'light', 'dark'].includes(theme)) theme = 'system';
  document.documentElement.dataset.theme = theme;
  document.documentElement.dataset.resolvedTheme = theme === 'system'
    ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
    : theme;
})();"#;

pub fn page_head(
    seo: &SeoConfig,
    page: &PageMetadata,
    locale: Locale,
    pathname: &str,
    page_type: PageType,
    robots: RobotsPolicy,
) -> Head {
    let title = page.title.text(locale);
    let description = page.description.text(locale);
    let canonical_path = canonical_pathname(pathname);
    let canonical = absolute_url(seo, &canonical_path);
    let alternates = language_alternates(seo, pathname);
    let image_path = if page.image.is_empty() {
        &seo.default_image
    } else {
        &page.image
    };
    let image = absolute_url(seo, image_path);
    let image_alt = page.image_alt.text(locale);
    let graph = structured_graph(seo, page, locale, pathname, page_type, robots);

    let mut head = Head::new(title)
        .description(description)
        .canonical(canonical.clone())
        .icon("/favicon.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/apple-touch-icon.png")
        .alternate("en", alternates.en)
        .alternate("es", alternates.es)
        .alternate("x-default", alternates.x_default)
        .inline_script(THEME_BOOTSTRAP)
        .stylesheet(SITE_STYLESHEET)
        .module_script(CLIENT_MODULE)
        .meta("generator", "PliegoRS")
        .meta("theme-color", "#171916")
        .meta("color-scheme", "light dark")
        .meta("application-name", seo.site_name.as_str())
        .meta("robots", robots.as_str())
        .meta("twitter:card", "summary_large_image")
        .meta("twitter:title", title)
        .meta("twitter:description", description)
        .meta("twitter:image", image.as_str())
        .meta("twitter:image:alt", image_alt)
        .property_meta("og:type", "website")
        .property_meta("og:site_name", seo.site_name.as_str())
        .property_meta(
            "og:locale",
            if locale.is_spanish() {
                "es_ES"
            } else {
                "en_US"
            },
        )
        .property_meta(
            "og:locale:alternate",
            if locale.is_spanish() {
                "en_US"
            } else {
                "es_ES"
            },
        )
        .property_meta("og:title", title)
        .property_meta("og:description", description)
        .property_meta("og:url", canonical)
        .property_meta("og:image", image.as_str())
        .property_meta("og:image:secure_url", image.as_str())
        .property_meta("og:image:type", social_image_type(image_path))
        .property_meta("og:image:alt", image_alt);
    if let Some((width, height)) = social_image_dimensions(image_path) {
        head = head
            .property_meta("og:image:width", width)
            .property_meta("og:image:height", height);
    }
    if !graph.is_null() {
        head = head.json_ld(graph);
    }
    head
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageAlternates {
    pub en: String,
    pub es: String,
    pub x_default: String,
}

pub fn language_alternates(seo: &SeoConfig, pathname: &str) -> LanguageAlternates {
    let canonical = canonical_pathname(pathname);
    let base = canonical
        .strip_prefix("/es/")
        .map(|path| normalize_pathname(&format!("/{path}")))
        .unwrap_or_else(|| {
            if canonical == "/es/" {
                "/".into()
            } else {
                canonical
            }
        });
    let spanish = if base == "/" {
        "/es/".into()
    } else {
        normalize_pathname(&format!("/es{base}"))
    };
    LanguageAlternates {
        en: absolute_url(seo, &base),
        es: absolute_url(seo, &spanish),
        x_default: absolute_url(seo, &base),
    }
}

pub fn canonical_pathname(pathname: &str) -> String {
    let normalized = normalize_pathname(pathname);
    if normalized == "/404.html/" {
        "/".into()
    } else {
        normalized
    }
}

pub fn normalize_pathname(pathname: &str) -> String {
    let raw = pathname.split(['?', '#']).next().unwrap_or("/");
    let mut path = if raw.starts_with('/') {
        raw.to_owned()
    } else {
        format!("/{raw}")
    };
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if path == "/" {
        path
    } else {
        format!("{}/", path.trim_end_matches('/'))
    }
}

pub fn absolute_url(seo: &SeoConfig, path: &str) -> String {
    if path.starts_with("https://") || path.starts_with("http://") {
        return path.to_owned();
    }
    format!(
        "{}/{}",
        seo.origin.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn structured_graph(
    seo: &SeoConfig,
    page: &PageMetadata,
    locale: Locale,
    pathname: &str,
    page_type: PageType,
    robots: RobotsPolicy,
) -> Value {
    if robots == RobotsPolicy::NoindexNofollow {
        return Value::Null;
    }
    let url = absolute_url(seo, &canonical_pathname(pathname));
    let organization_id = format!("{}#organization", seo.origin);
    let website_id = format!("{}#website", seo.origin);
    let breadcrumb_id = format!("{url}#breadcrumb");
    let page_kind = match page_type {
        PageType::Standard => "WebPage",
        PageType::About => "AboutPage",
        PageType::Collection => "CollectionPage",
    };
    let mut graph = vec![
        json!({
            "@type": page_kind,
            "@id": format!("{url}#webpage"),
            "url": url,
            "name": page.title.text(locale),
            "description": page.description.text(locale),
            "inLanguage": locale.language_tag(),
            "isPartOf": { "@id": website_id },
            "publisher": { "@id": organization_id },
            "breadcrumb": { "@id": breadcrumb_id },
            "primaryImageOfPage": { "@type": "ImageObject", "url": absolute_url(seo, &page.image) },
        }),
        breadcrumb_schema(seo, locale, pathname, page.title.text(locale)),
    ];
    if pathname == "/" || pathname == "/es" {
        graph.push(organization_schema(seo, locale));
        graph.push(website_schema(seo, locale));
    }
    json!({ "@context": "https://schema.org", "@graph": graph })
}

fn organization_schema(seo: &SeoConfig, locale: Locale) -> Value {
    json!({
        "@type": "Organization",
        "@id": format!("{}#organization", seo.origin),
        "name": seo.site_name,
        "legalName": seo.legal_name,
        "url": absolute_url(seo, "/"),
        "email": seo.contact_email,
        "description": seo.organization_description.text(locale),
        "location": { "@type": "Place", "name": seo.location },
        "logo": {
            "@type": "ImageObject",
            "url": absolute_url(seo, &seo.logo),
            "width": seo.logo_width,
            "height": seo.logo_height,
        }
    })
}

fn website_schema(seo: &SeoConfig, locale: Locale) -> Value {
    json!({
        "@type": "WebSite",
        "@id": format!("{}#website", seo.origin),
        "url": absolute_url(seo, "/"),
        "name": seo.site_name,
        "description": seo.website_description.text(locale),
        "inLanguage": locale.language_tag(),
        "publisher": { "@id": format!("{}#organization", seo.origin) },
    })
}

fn breadcrumb_schema(seo: &SeoConfig, locale: Locale, pathname: &str, title: &str) -> Value {
    let home = if locale.is_spanish() { "/es/" } else { "/" };
    let url = absolute_url(seo, &canonical_pathname(pathname));
    let mut items = vec![json!({
        "@type": "ListItem", "position": 1, "name": "PliegoRS", "item": absolute_url(seo, home)
    })];
    if canonical_pathname(pathname) != home {
        items.push(json!({ "@type": "ListItem", "position": 2, "name": title, "item": url }));
    }
    json!({ "@type": "BreadcrumbList", "@id": format!("{url}#breadcrumb"), "itemListElement": items })
}

fn social_image_type(path: &str) -> &'static str {
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else if path.ends_with(".avif") {
        "image/avif"
    } else {
        "image/jpeg"
    }
}

fn social_image_dimensions(path: &str) -> Option<(&'static str, &'static str)> {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    if path.ends_with("/fold-hero.webp") || path.ends_with("/fold-hero.avif") {
        Some(("1672", "941"))
    } else if path.ends_with("/fold-portrait.webp") || path.ends_with("/fold-portrait.avif") {
        Some(("1122", "1402"))
    } else if path.ends_with("/ledger-wide.webp") || path.ends_with("/ledger-wide.avif") {
        Some(("1536", "1024"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_paths_and_languages_are_stable() {
        assert_eq!(normalize_pathname("docs//events?ref=nav"), "/docs/events/");
        assert_eq!(canonical_pathname("/404.html"), "/");
    }

    #[test]
    fn social_image_dimensions_match_the_published_assets() {
        assert_eq!(
            social_image_dimensions("/media/pliegors/fold-hero.webp"),
            Some(("1672", "941"))
        );
        assert_eq!(
            social_image_dimensions("/media/pliegors/fold-portrait.avif?rev=1"),
            Some(("1122", "1402"))
        );
        assert_eq!(
            social_image_dimensions("/media/pliegors/ledger-wide.webp"),
            Some(("1536", "1024"))
        );
        assert_eq!(social_image_dimensions("/media/unknown.webp"), None);
    }
}
