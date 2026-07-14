use super::{alternate_path, brand_mark, is_active, locale_path};
use crate::content::{AppearanceCopy, Locale, ShellCopy};
use pliego_dom::{IntoView, View, el};

/// Render the localized product header and its progressive-enhancement hooks.
pub fn header(locale: Locale, pathname: &str, shell: &ShellCopy) -> View {
    let alternate = alternate_path(locale, pathname);
    el("header")
        .class("site-header")
        .attr("data-site-header", "")
        .child(
            el("a")
                .class("site-logo")
                .attr("href", locale_path(locale, "/"))
                .attr(
                    "aria-label",
                    localize(locale, "PliegoRS home", "Inicio de PliegoRS"),
                )
                .attr("title", shell.tagline.text(locale))
                .child(brand_mark(25, None))
                .child(el("span").child("PLIEGO").child(el("b").child("RS"))),
        )
        .child(desktop_navigation(locale, pathname, shell))
        .child(
            el("div")
                .class("header-tools")
                .child(locale_link(locale, &alternate, false))
                .child(theme_control(locale, false, &shell.appearance))
                .child(
                    el("a")
                        .class("icon-link source-link")
                        .attr("href", "https://github.com/celiumsai/pliegors")
                        .attr(
                            "aria-label",
                            localize(locale, "PliegoRS on GitHub", "PliegoRS en GitHub"),
                        )
                        .attr(
                            "title",
                            localize(locale, "PliegoRS on GitHub", "PliegoRS en GitHub"),
                        )
                        .child(icon("github", 19))
                        .child(
                            el("span")
                                .class("source-private-label")
                                .child(localize(locale, "GitHub", "GitHub")),
                        ),
                )
                .child(
                    el("button")
                        .class("menu-toggle")
                        .attr("type", "button")
                        .attr("data-menu-toggle", "")
                        .attr("data-menu-open", shell.menu.open.text(locale))
                        .attr("data-menu-close", shell.menu.close.text(locale))
                        .attr("aria-expanded", "false")
                        .attr("aria-controls", "mobile-menu")
                        .child(
                            el("span")
                                .class("sr-only")
                                .attr("data-menu-label", "")
                                .child(shell.menu.open.text(locale)),
                        )
                        .child(icon_with_class("menu", 21, "menu-open-icon"))
                        .child(icon_with_class("x", 21, "menu-close-icon")),
                ),
        )
        .child(mobile_navigation(locale, pathname, &alternate, shell))
        .child(
            el("div")
                .class("page-progress")
                .attr("aria-hidden", "true")
                .child(el("span").attr("data-page-progress", "")),
        )
        .into_view()
}

fn desktop_navigation(locale: Locale, pathname: &str, shell: &ShellCopy) -> View {
    let mut nav = el("nav").class("desktop-nav").attr(
        "aria-label",
        localize(locale, "Primary navigation", "Navegación principal"),
    );
    for item in shell.navigation.iter().filter(|item| item.key != "account") {
        nav = nav.child(nav_link(
            locale,
            pathname,
            &item.path,
            item.label.text(locale),
        ));
    }
    nav.into_view()
}

fn mobile_navigation(locale: Locale, pathname: &str, alternate: &str, shell: &ShellCopy) -> View {
    let mut nav = el("nav").attr(
        "aria-label",
        localize(locale, "Mobile navigation", "Navegación móvil"),
    );
    for (index, item) in shell.navigation.iter().enumerate() {
        nav = nav.child(numbered_nav_link(
            locale,
            pathname,
            &item.path,
            item.label.text(locale),
            index + 1,
        ));
    }

    el("div")
        .class("mobile-menu")
        .id("mobile-menu")
        .attr("data-mobile-menu", "")
        .attr("role", "dialog")
        .attr("aria-modal", "true")
        .attr(
            "aria-label",
            localize(locale, "Site menu", "Menú del sitio"),
        )
        .attr("aria-hidden", "true")
        .child(nav)
        .child(
            el("div")
                .class("mobile-menu-meta")
                .child(el("p").child(shell.descriptor.text(locale)))
                .child(locale_link(locale, alternate, true))
                .child(
                    el("div")
                        .class("mobile-theme-row")
                        .child(el("span").child(shell.appearance.label.text(locale)))
                        .child(theme_control(locale, true, &shell.appearance)),
                ),
        )
        .into_view()
}

fn nav_link(locale: Locale, pathname: &str, href: &str, label: &str) -> View {
    let mut link = el("a")
        .attr("href", locale_path(locale, href))
        .child(label.to_owned());
    if is_active(locale, pathname, href) {
        link = link.attr("aria-current", "page");
    }
    link.into_view()
}

fn numbered_nav_link(
    locale: Locale,
    pathname: &str,
    href: &str,
    label: &str,
    index: usize,
) -> View {
    let mut link = el("a")
        .attr("href", locale_path(locale, href))
        .attr("data-menu-link", "")
        .child(el("span").child(format!("{index:02}")))
        .child(label.to_owned());
    if is_active(locale, pathname, href) {
        link = link.attr("aria-current", "page");
    }
    link.into_view()
}

fn locale_link(locale: Locale, alternate: &str, expanded_label: bool) -> View {
    let label = match (locale, expanded_label) {
        (Locale::En, false) => "ES",
        (Locale::Es, false) => "EN",
        (Locale::En, true) => "Español",
        (Locale::Es, true) => "English",
    };
    el("a")
        .class(if expanded_label { "" } else { "locale-link" })
        .attr("href", alternate)
        .attr("hreflang", if locale == Locale::En { "es" } else { "en" })
        .attr("lang", if locale == Locale::En { "es" } else { "en" })
        .attr("data-locale-link", "")
        .child(label)
        .into_view()
}

fn theme_control(locale: Locale, mobile: bool, copy: &AppearanceCopy) -> View {
    let class_name = if mobile {
        "theme-control theme-control--mobile"
    } else {
        "theme-control"
    };
    el("div")
        .class(class_name)
        .attr("role", "group")
        .attr("aria-label", copy.label.text(locale))
        .child(theme_button("system", copy.system.text(locale), "monitor"))
        .child(theme_button("light", copy.light.text(locale), "sun"))
        .child(theme_button("dark", copy.dark.text(locale), "moon"))
        .into_view()
}

fn theme_button(value: &str, label: &str, glyph: &str) -> View {
    el("button")
        .attr("type", "button")
        .attr("data-theme-choice", value)
        .attr("title", label)
        .attr("aria-label", label)
        .attr(
            "aria-pressed",
            if value == "system" { "true" } else { "false" },
        )
        .child(icon(glyph, 15))
        .into_view()
}

fn icon(name: &str, size: u16) -> View {
    icon_with_class(name, size, "")
}

fn icon_with_class(name: &str, size: u16, extra_class: &str) -> View {
    let class_name = if extra_class.is_empty() {
        format!("lucide lucide-{name}")
    } else {
        format!("lucide lucide-{name} {extra_class}")
    };
    let mut svg = el("svg")
        .class(class_name)
        .attr("xmlns", "http://www.w3.org/2000/svg")
        .attr("width", size.to_string())
        .attr("height", size.to_string())
        .attr("viewBox", "0 0 24 24")
        .attr("fill", "none")
        .attr("stroke", "currentColor")
        .attr("stroke-width", "2")
        .attr("stroke-linecap", "round")
        .attr("stroke-linejoin", "round")
        .attr("aria-hidden", "true");
    svg = match name {
        "monitor" => svg
            .child(el("rect").attr("width", "20").attr("height", "14").attr("x", "2").attr("y", "3").attr("rx", "2"))
            .child(el("line").attr("x1", "8").attr("x2", "16").attr("y1", "21").attr("y2", "21"))
            .child(el("line").attr("x1", "12").attr("x2", "12").attr("y1", "17").attr("y2", "21")),
        "sun" => svg
            .child(el("circle").attr("cx", "12").attr("cy", "12").attr("r", "4"))
            .child(el("path").attr("d", "M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41")),
        "moon" => svg.child(el("path").attr("d", "M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401")),
        "github" => svg.child(el("path").attr("d", "M15 22v-4a4.8 4.8 0 0 0-1-3.5c3.28-.36 6.72-1.61 6.72-7.25A5.65 5.65 0 0 0 19.22 3.3 5.3 5.3 0 0 0 19.08 0S17.9-.36 15 1.5a13.4 13.4 0 0 0-7 0C5.1-.36 3.92 0 3.92 0a5.3 5.3 0 0 0-.14 3.3A5.65 5.65 0 0 0 2.28 7.3c0 5.6 3.44 6.85 6.72 7.25A4.8 4.8 0 0 0 8 18v4M8 19c-3 .9-3-1.5-4-2")),
        "menu" => svg.child(el("path").attr("d", "M4 5h16M4 12h16M4 19h16")),
        "x" => svg.child(el("path").attr("d", "M18 6 6 18M6 6l12 12")),
        _ => svg,
    };
    svg.into_view()
}

fn localize<'a>(locale: Locale, en: &'a str, es: &'a str) -> &'a str {
    if locale == Locale::Es { es } else { en }
}
