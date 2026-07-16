// SPDX-License-Identifier: Apache-2.0

mod domain;

use pliego_dom::{IntoView, View, el};
use pliego_ssg::{Asset, Head, Page, Site};
use std::path::PathBuf;

const CSS: &[u8] = include_bytes!("../assets/site.css");
const FAVICON: &[u8] = include_bytes!("../assets/favicon.svg");
const MANIFEST: &[u8] = include_bytes!("../assets/site.webmanifest");
const ROBOTS: &[u8] = include_bytes!("../assets/robots.txt");
const SITE_URL: &str = "https://example.com";

fn head(title: &str, description: &str, path: &str) -> Head {
    Head::new(title)
        .description(description)
        .canonical(format!("{SITE_URL}{path}"))
        .icon("/assets/favicon.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/assets/favicon.svg")
        .stylesheet("/assets/site.css")
        .meta("theme-color", "#151714")
        .meta("generator", "PliegoRS")
        .property_meta("og:type", "website")
        .property_meta("og:title", title)
        .property_meta("og:description", description)
}

fn brand() -> View {
    el("a")
        .class("brand")
        .attr("href", "/")
        .attr("aria-label", "__NAME__ home")
        .child(
            el("img")
                .attr("src", "/assets/favicon.svg")
                .attr("width", "34")
                .attr("height", "34")
                .attr("alt", ""),
        )
        .child(el("span").child("__NAME__"))
        .into_view()
}

fn navigation() -> View {
    el("header")
        .class("site-header")
        .child(
            el("nav")
                .class("shell nav")
                .attr("aria-label", "Primary navigation")
                .child(brand())
                .child(
                    el("div")
                        .class("nav-links")
                        .child(el("a").attr("href", "/guide/").child("Guide"))
                        .child(
                            el("a")
                                .attr("href", "https://pliegors.dev/docs/")
                                .child("Documentation"),
                        ),
                ),
        )
        .into_view()
}

fn command(label: &str, value: &str) -> View {
    el("div")
        .class("command")
        .child(el("span").child(label))
        .child(el("code").child(value))
        .into_view()
}

fn home(event_count: usize) -> View {
    vec![
        navigation(),
        el("main")
            .child(
                el("section")
                    .class("hero shell")
                    .attr("aria-labelledby", "hero-title")
                    .child(
                        el("div")
                            .class("hero-copy")
                            .child(el("p").class("eyebrow").child("PLIEGORS / READY"))
                            .child(el("h1").id("hero-title").child("Your Rust site is running."))
                            .child(
                                el("p")
                                    .class("lede")
                                    .child("A complete first document, rendered by Rust and ready to become yours."),
                            )
                            .child(
                                el("div")
                                    .class("hero-actions")
                                    .child(el("a").class("button primary").attr("href", "/guide/").child("Start building"))
                                    .child(el("a").class("button secondary").attr("href", "https://pliegors.dev/docs/getting-started/").child("Read the docs")),
                            ),
                    )
                    .child(
                        el("div")
                            .class("status-panel")
                            .child(
                                el("div")
                                    .class("panel-head")
                                    .child(el("span").class("signal").attr("aria-hidden", "true"))
                                    .child(el("span").child("Development session"))
                                    .child(el("strong").child(format!("{event_count:02}"))),
                            )
                            .child(command("CHECK", "pliego check"))
                            .child(command("DEVELOP", "pliego dev"))
                            .child(command("SHIP", "pliego build")),
                    ),
            )
            .child(
                el("section")
                    .class("first-change")
                    .attr("aria-labelledby", "change-title")
                    .child(
                        el("div")
                            .class("shell change-grid")
                            .child(
                                el("div")
                                    .class("change-intro")
                                    .child(el("p").class("eyebrow").child("FIRST CHANGE"))
                                    .child(el("h2").id("change-title").child("Make it respond to you."))
                                    .child(el("p").child("Open the view, change the headline, and save. PliegoRS rebuilds the document and refreshes this page.")),
                            )
                            .child(
                                el("div")
                                    .class("file-card")
                                    .child(el("div").class("file-label").child("src/main.rs"))
                                    .child(el("pre").child(el("code").child("fn home() -> View {\n    el(\"main\")\n        .child(\"Build something exact.\")\n        .into_view()\n}"))),
                            ),
                    ),
            )
            .child(
                el("section")
                    .class("system shell")
                    .attr("aria-labelledby", "system-title")
                    .child(
                        el("div")
                            .class("section-heading")
                            .child(el("p").class("eyebrow").child("WHAT IS WIRED"))
                            .child(el("h2").id("system-title").child("A small project with a complete contract.")),
                    )
                    .child(
                        el("div")
                            .class("capability-grid")
                            .child(el("article").child(el("span").child("01")).child(el("h3").child("Rust views")).child(el("p").child("Routes and semantic documents authored with typed builders.")))
                            .child(el("article").child(el("span").child("02")).child(el("h3").child("Deterministic output")).child(el("p").child("Static assets and an auditable build ledger in target/site.")))
                            .child(el("article").child(el("span").child("03")).child(el("h3").child("Native identity")).child(el("p").child("Metadata, manifest, favicon, 404, and crawler policy included."))),
                    ),
            )
            .into_view(),
        footer(),
    ]
    .into_view()
}

fn guide() -> View {
    vec![
        navigation(),
        el("main")
            .class("guide shell")
            .child(el("p").class("eyebrow").child("LOCAL GUIDE / 01"))
            .child(el("h1").child("Know the project before you replace it."))
            .child(
                el("div")
                    .class("guide-grid")
                    .child(
                        el("aside")
                            .class("guide-index")
                            .child(el("a").attr("href", "#structure").child("Project structure"))
                            .child(el("a").attr("href", "#route").child("Add a route"))
                            .child(el("a").attr("href", "#release").child("Build for release")),
                    )
                    .child(
                        el("div")
                            .class("guide-body")
                            .child(
                                el("section")
                                    .id("structure")
                                    .child(el("span").class("step").child("01"))
                                    .child(el("h2").child("Project structure"))
                                    .child(el("p").child("The source owns documents. The assets directory owns browser-delivered files. pliego.toml connects the Cargo package to the output directory."))
                                    .child(el("pre").child(el("code").child("src/main.rs\nassets/site.css\nassets/favicon.svg\npliego.toml\nCargo.toml"))),
                            )
                            .child(
                                el("section")
                                    .id("route")
                                    .child(el("span").class("step").child("02"))
                                    .child(el("h2").child("Add a route"))
                                    .child(el("p").child("Create a view function, then register one more Page in main(). The route becomes a complete HTML document during the build."))
                                    .child(el("pre").child(el("code").child(".page(Page::new(\n    \"/notes/\",\n    head(\"Notes\", \"Project notes.\", \"/notes/\"),\n    notes(),\n))"))),
                            )
                            .child(
                                el("section")
                                    .id("release")
                                    .child(el("span").class("step").child("03"))
                                    .child(el("h2").child("Build for release"))
                                    .child(el("p").child("Run the checks, build the output, and inspect its ledger before publishing target/site to your own infrastructure."))
                                    .child(el("pre").child(el("code").child("pliego check\npliego build\npliego inspect\npliego preview"))),
                            ),
                    ),
            )
            .child(el("a").class("next-link").attr("href", "https://pliegors.dev/docs/").child("Continue in the complete documentation ->"))
            .into_view(),
        footer(),
    ]
    .into_view()
}

fn not_found() -> View {
    el("main")
        .class("error-page")
        .child(brand())
        .child(el("p").class("error-code").child("404 / ROUTE_NOT_FOUND"))
        .child(el("h1").child("This document has not been composed."))
        .child(el("p").child("The route is missing from this PliegoRS site. Check the URL or return to the first document."))
        .child(el("a").class("button primary").attr("href", "/").child("Return home"))
        .into_view()
}

fn footer() -> View {
    el("footer")
        .class("footer shell")
        .child(el("span").child("Built with PliegoRS"))
        .child(
            el("a")
                .attr("href", "https://pliegors.dev")
                .child("pliegors.dev"),
        )
        .into_view()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));
    let state = domain::first_replayable_state()?;
    Site::new()
        .page(
            Page::new(
                "/",
                head(
                    "__NAME__ / Built with PliegoRS",
                    "A new Rust-native website built with PliegoRS.",
                    "/",
                ),
                home(state.notes.len()),
            )
            .source("src/domain.rs")
            .source("src/main.rs"),
        )
        .page(
            Page::new(
                "/guide/",
                head(
                    "Project guide / __NAME__",
                    "The local first-use guide for this PliegoRS project.",
                    "/guide/",
                ),
                guide(),
            )
            .source("src/main.rs"),
        )
        .page(
            Page::new(
                "/404.html",
                head(
                    "Route not found / __NAME__",
                    "The requested route does not exist.",
                    "/404.html",
                ),
                not_found(),
            )
            .source("src/main.rs"),
        )
        .asset(Asset::new("assets/site.css", CSS.to_vec()).source("assets/site.css"))
        .asset(Asset::new("assets/favicon.svg", FAVICON.to_vec()).source("assets/favicon.svg"))
        .asset(Asset::new("site.webmanifest", MANIFEST.to_vec()).source("assets/site.webmanifest"))
        .asset(Asset::new("robots.txt", ROBOTS.to_vec()).source("assets/robots.txt"))
        .build(output)?;
    Ok(())
}
