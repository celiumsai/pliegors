use crate::components::BaseLayout;
use crate::content::{
    Locale, Localized, PageMetadata, PageType, RobotsPolicy, RouteRecord, SiteContent,
};
use crate::{docs, pages, seo};
use pliego_ssg::Page;

pub fn pages(content: &SiteContent) -> Result<Vec<Page>, String> {
    let mut output = Vec::with_capacity(content.routes().routes.len() * 2 + 1);
    for locale in Locale::ALL {
        for route in &content.routes().routes {
            let pathname = localized_route(locale, &route.path);
            let metadata = content
                .metadata(&route.page)
                .ok_or_else(|| format!("missing metadata for {}", route.page))?;
            let body = render_body(locale, route)?;
            let body = BaseLayout::new(locale, &pathname, content.shell())
                .page_class(&format!("{}-page", route.page))
                .render(body);
            let head = seo::page_head(
                content.seo(),
                metadata,
                locale,
                &pathname,
                route.page_type,
                route.robots,
            );
            output.push(Page::new(pathname, head, body).language(locale.code()));
        }
    }
    output.push(not_found(content));
    Ok(output)
}

fn localized_route(locale: Locale, path: &str) -> String {
    if locale.is_spanish() {
        if path == "/" {
            "/es".into()
        } else {
            format!("/es{path}")
        }
    } else {
        path.to_owned()
    }
}

fn render_body(locale: Locale, route: &RouteRecord) -> Result<pliego_dom::View, String> {
    match route.page.as_str() {
        "home" => Ok(pages::home(locale)),
        "about" => Ok(pages::about(locale)),
        "docs" => Ok(docs::index(locale)),
        page if page.starts_with("docs-") => {
            docs::article(locale, page.trim_start_matches("docs-"))
        }
        "changelog" => Ok(pages::changelog(locale)),
        "security" => Ok(pages::security(locale)),
        "accessibility" => Ok(pages::accessibility(locale)),
        "legal" => Ok(pages::legal_hub(locale)),
        page if page.starts_with("legal-") => {
            pages::legal_document(locale, page.trim_start_matches("legal-"))
        }
        page => Err(format!("unsupported PliegoRS site page {page}")),
    }
}

fn not_found(content: &SiteContent) -> Page {
    let metadata = PageMetadata {
        key: "404".into(),
        title: Localized {
            en: "404 | PliegoRS".into(),
            es: "404 | PliegoRS".into(),
        },
        description: Localized {
            en: "Route not found in the PliegoRS graph.".into(),
            es: "Ruta no encontrada en el grafo de PliegoRS.".into(),
        },
        image: content.seo().default_image.clone(),
        image_alt: Localized {
            en: "PliegoRS framework".into(),
            es: "Framework PliegoRS".into(),
        },
    };
    let head = seo::page_head(
        content.seo(),
        &metadata,
        Locale::En,
        "/404.html",
        PageType::Standard,
        RobotsPolicy::NoindexNofollow,
    );
    let body = BaseLayout::new(Locale::En, "/404.html", content.shell())
        .page_class("not-found-page")
        .render(pages::not_found(Locale::En));
    Page::new("/404.html", head, body).language("en")
}

pub fn sitemap_routes(content: &SiteContent) -> Vec<String> {
    Locale::ALL
        .into_iter()
        .flat_map(|locale| {
            content
                .routes()
                .routes
                .iter()
                .filter(|route| route.robots == RobotsPolicy::IndexFollow)
                .map(move |route| localized_route(locale, &route.path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_authored_routes_have_language_twins() {
        let content = SiteContent::load().unwrap();
        let pages = pages(&content).unwrap();
        assert_eq!(pages.len(), content.routes().routes.len() * 2 + 1);
    }
}
