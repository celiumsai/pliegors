use super::{brand_mark, locale_path};
use crate::content::{Locale, ShellCopy};
use pliego_dom::{IntoView, View, el};

pub fn footer(locale: Locale, shell: &ShellCopy) -> View {
    el("footer")
        .class("site-footer")
        .child(
            el("div")
                .class("footer-statement")
                .child(brand_mark(58, None))
                .child(el("p").child(shell.footer.line.text(locale))),
        )
        .child(
            el("div")
                .class("footer-columns")
                .child(
                    el("div")
                        .class("footer-identity")
                        .child(el("p").class("utility-label").child("PLIEGORS.DEV"))
                        .child(el("p").child(shell.footer.endorsement.text(locale)))
                        .child(
                            el("a")
                                .attr("href", "mailto:hello@pliegors.dev")
                                .child("hello@pliegors.dev"),
                        ),
                )
                .child(footer_directory(locale)),
        )
        .child(
            el("div")
                .class("footer-floor")
                .child(el("span").child("© 2026 Celiums Solutions LLC"))
                .child(el("span").child("Medellín · Worldwide"))
                .child(el("span").child(localize(
                    locale,
                    "Private development",
                    "Desarrollo privado",
                ))),
        )
        .into_view()
}

fn footer_directory(locale: Locale) -> View {
    el("div")
        .class("footer-directory")
        .child(directory(
            localize(locale, "Framework", "Framework"),
            locale,
            &[
                ("/docs", "Documentation", "Documentación"),
                ("/docs/getting-started", "Getting started", "Primeros pasos"),
                (
                    "/docs/developer-loop",
                    "Developer loop",
                    "Bucle de desarrollo",
                ),
                (
                    "/docs/events-and-folds",
                    "Events and folds",
                    "Eventos y folds",
                ),
                (
                    "/docs/artifact-trust",
                    "Artifact trust",
                    "Confianza de artefactos",
                ),
                ("/docs/crate-reference", "Crates and API", "Crates y API"),
            ],
        ))
        .child(directory(
            localize(locale, "Project", "Proyecto"),
            locale,
            &[
                ("/about", "About", "Acerca de"),
                ("/changelog", "Changelog", "Cambios"),
                ("/security", "Security", "Seguridad"),
                ("/accessibility", "Accessibility", "Accesibilidad"),
            ],
        ))
        .child(directory(
            localize(locale, "Legal", "Legal"),
            locale,
            &[
                ("/legal", "Legal register", "Registro legal"),
                ("/legal/terms", "Terms", "Términos"),
                ("/legal/privacy", "Privacy", "Privacidad"),
                ("/legal/cookies", "Cookies", "Cookies"),
                ("/legal/acceptable-use", "Acceptable use", "Uso aceptable"),
            ],
        ))
        .child(
            el("nav")
                .attr("aria-label", "External")
                .child(el("p").class("utility-label").child("SOURCE"))
                .child(
                    el("a")
                        .attr("href", "https://github.com/celiumsai/pliegors")
                        .child("GitHub ↗"),
                )
                .child(
                    el("a")
                        .attr("href", "mailto:hello@pliegors.dev")
                        .child("Contact ↗"),
                ),
        )
        .into_view()
}

fn directory(label: &str, locale: Locale, links: &[(&str, &str, &str)]) -> View {
    let mut nav = el("nav")
        .attr("aria-label", label)
        .child(el("p").class("utility-label").child(label.to_owned()));
    for (path, en, es) in links {
        nav = nav.child(
            el("a")
                .attr("href", locale_path(locale, path))
                .child(localize(locale, en, es)),
        );
    }
    nav.into_view()
}

fn localize<'a>(locale: Locale, en: &'a str, es: &'a str) -> &'a str {
    if locale.is_spanish() { es } else { en }
}
