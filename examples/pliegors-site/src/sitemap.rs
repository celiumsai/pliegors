const ORIGIN: &str = "https://pliegors.dev";

pub fn index() -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n  <sitemap><loc>{ORIGIN}/sitemap-0.xml</loc></sitemap>\n</sitemapindex>\n"
    )
    .into_bytes()
}

pub fn localized<T: AsRef<str>>(routes: &[T]) -> Result<Vec<u8>, String> {
    if routes.iter().any(|route| {
        let route = route.as_ref();
        !route.starts_with('/') || route.contains('&') || route.contains('<') || route.contains('>')
    }) {
        return Err("PliegoRS sitemap received an unsafe route".into());
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for route in routes {
        let canonical = canonical_path(route.as_ref());
        xml.push_str("  <url><loc>");
        xml.push_str(ORIGIN);
        xml.push_str(&canonical);
        xml.push_str("</loc></url>\n");
    }
    xml.push_str("</urlset>\n");
    Ok(xml.into_bytes())
}

fn canonical_path(route: &str) -> String {
    if route == "/" {
        "/".into()
    } else {
        format!("{}/", route.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_trailing_slash_canonicals() {
        let xml = String::from_utf8(localized(&["/", "/es", "/docs"]).unwrap()).unwrap();
        assert!(xml.contains("https://pliegors.dev/</loc>"));
        assert!(xml.contains("https://pliegors.dev/es/</loc>"));
        assert!(xml.contains("https://pliegors.dev/docs/</loc>"));
    }

    #[test]
    fn rejects_unsafe_routes() {
        assert!(localized(&["relative"]).is_err());
        assert!(localized(&["/bad&route"]).is_err());
    }
}
