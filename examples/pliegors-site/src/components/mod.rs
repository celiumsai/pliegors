mod brand;
mod footer;
mod header;
mod layout;

pub use crate::content::{alternate_path, locale_path};
pub use brand::brand_mark;
pub use footer::footer;
pub use header::header;
pub use layout::BaseLayout;

use crate::content::Locale;

pub(crate) fn is_active(locale: Locale, pathname: &str, base: &str) -> bool {
    let current = normalize_route(pathname);
    let target = locale_path(locale, base);
    current == target || current.starts_with(&format!("{}/", target.trim_end_matches('/')))
}

fn normalize_route(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let mut normalized = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    if normalized.len() > 1 {
        normalized.truncate(normalized.trim_end_matches('/').len());
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localizes_and_reverses_routes() {
        assert_eq!(locale_path(Locale::Es, "/docs/"), "/es/docs/");
        assert_eq!(alternate_path(Locale::Es, "/es/docs/"), "/docs/");
        assert_eq!(alternate_path(Locale::En, "/404.html"), "/es/");
    }

    #[test]
    fn recognizes_nested_active_routes() {
        assert!(is_active(Locale::Es, "/es/legal/privacy/", "/legal"));
        assert!(!is_active(Locale::En, "/security/", "/docs"));
    }
}
