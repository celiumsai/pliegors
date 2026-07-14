// SPDX-License-Identifier: Apache-2.0

use pliego_dom::{IntoView, View, el};
use pliego_ssg::{Asset, Head, Page, Site};
use std::path::PathBuf;

const CSS: &[u8] = include_bytes!("../assets/site.css");
const MARK: &[u8] = include_bytes!("../assets/pliego-mark.svg");
const MANIFEST: &[u8] = include_bytes!("../assets/site.webmanifest");
const SITE_URL: &str = "https://example.com";

fn head(title: &str, description: &str, path: &str) -> Head {
    Head::new(title)
        .description(description)
        .canonical(format!("{SITE_URL}{path}"))
        .icon("/assets/pliego-mark.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/assets/pliego-mark.svg")
        .stylesheet("/assets/site.css")
        .meta("theme-color", "#f4f3ec")
        .meta("generator", "PliegoRS")
        .property_meta("og:type", "website")
        .property_meta("og:title", title)
        .property_meta("og:description", description)
}

fn navigation() -> View {
    el("nav")
        .class("nav shell")
        .attr("aria-label", "Primary navigation")
        .child(
            el("a")
                .class("brand")
                .attr("href", "/")
                .attr("aria-label", "Home")
                .child(
                    el("img")
                        .attr("src", "/assets/pliego-mark.svg")
                        .attr("width", "32")
                        .attr("height", "32")
                        .attr("alt", ""),
                )
                .child("FORM / 01"),
        )
        .child(el("a").class("nav-link").attr("href", "#contact").child("Begin a project"))
        .into_view()
}

fn home() -> View {
    vec![
        navigation(),
        el("main")
            .child(
                el("section")
                    .class("hero shell")
                    .attr("aria-labelledby", "hero-title")
                    .child(el("p").class("eyebrow").child("Independent practice / Medellin"))
                    .child(
                        el("h1")
                            .id("hero-title")
                            .child("A precise place for a clear point of view."),
                    )
                    .child(
                        el("div")
                            .class("hero-foot")
                            .child(el("p").child("A native PliegoRS foundation for portfolios, studios, and small editorial sites."))
                            .child(el("a").class("arrow-link").attr("href", "#work").child("View the structure ↓")),
                    ),
            )
            .child(
                el("section")
                    .id("work")
                    .class("work shell")
                    .attr("aria-labelledby", "work-title")
                    .child(
                        el("div")
                            .class("section-index")
                            .child(el("span").child("01"))
                            .child(el("h2").id("work-title").child("Selected capabilities")),
                    )
                    .child(
                        el("div")
                            .class("work-grid")
                            .child(el("article").class("work-item work-item-primary").child(el("p").child("01 / Structure")).child(el("h3").child("Routes and complete documents in Rust")).child(el("span").child("Deterministic output")))
                            .child(el("article").class("work-item").child(el("p").child("02 / Behavior")).child(el("h3").child("Fine-grained browser interaction")).child(el("span").child("WASM when needed")))
                            .child(el("article").class("work-item").child(el("p").child("03 / Delivery")).child(el("h3").child("Assets with an auditable ledger")).child(el("span").child("Built for the edge"))),
                    ),
            )
            .child(
                el("section")
                    .id("contact")
                    .class("contact shell")
                    .child(el("p").class("eyebrow").child("Your next move"))
                    .child(el("h2").child("Replace this sentence with the reason your work needs to exist."))
                    .child(el("a").class("contact-link").attr("href", "mailto:hello@example.com").child("hello@example.com")),
            )
            .into_view(),
        el("footer")
            .class("footer shell")
            .child(el("span").child("Built with PliegoRS"))
            .child(el("span").child("Edition / 2026"))
            .into_view(),
    ]
    .into_view()
}

fn not_found() -> View {
    el("main")
        .class("not-found shell")
        .child(el("p").class("eyebrow").child("Error / 404"))
        .child(el("h1").child("This route has not been composed."))
        .child(el("a").class("arrow-link").attr("href", "/").child("← Return home"))
        .into_view()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));
    Site::new()
        .page(Page::new(
            "/",
            head(
                "FORM / A PliegoRS foundation",
                "A minimal Rust-native website foundation built with PliegoRS.",
                "/",
            ),
            home(),
        ))
        .page(Page::new(
            "/404.html",
            head(
                "Not found / FORM",
                "The requested route does not exist.",
                "/404.html",
            ),
            not_found(),
        ))
        .asset(Asset::new("assets/site.css", CSS.to_vec()))
        .asset(Asset::new("assets/pliego-mark.svg", MARK.to_vec()))
        .asset(Asset::new("site.webmanifest", MANIFEST.to_vec()))
        .build(output)?;
    Ok(())
}
