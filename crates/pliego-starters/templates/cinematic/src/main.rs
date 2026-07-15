// SPDX-License-Identifier: Apache-2.0

use pliego_dom::{IntoView, View, el};
use pliego_ssg::{Asset, Head, Page, Site};
use serde_json::json;
use std::path::PathBuf;

const SITE_URL: &str = "https://cinematic.pliegors.dev";
const CSS: &[u8] = include_bytes!("../assets/site.css");

const FAVICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#090c0d"/><circle cx="32" cy="32" r="18" fill="none" stroke="#f4f1e8" stroke-width="4"/><path d="M6 48 56 14" stroke="#ef573c" stroke-width="5"/></svg>"##;

const MANIFEST: &str = r##"{
  "name": "AFTERLIGHT — Cinematic PliegoRS Starter",
  "short_name": "AFTERLIGHT",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#090c0d",
  "theme_color": "#090c0d",
  "icons": [{"src":"/favicon.svg","sizes":"any","type":"image/svg+xml"}]
}
"##;

const ROBOTS: &str = "User-agent: *\nAllow: /\nSitemap: https://cinematic.pliegors.dev/sitemap.xml\n";
const SITEMAP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://cinematic.pliegors.dev/</loc></url>
</urlset>
"#;

fn brand() -> View {
    el("a")
        .class("brand")
        .attr("href", "/")
        .attr("aria-label", "AFTERLIGHT home")
        .child(el("span").class("brand__mark").attr("aria-hidden", "true"))
        .child(el("strong").child("AFTERLIGHT"))
        .child(el("span").class("brand__edition").child("Film 01 / 2026"))
        .into_view()
}

fn home() -> View {
    el("div")
        .class("site")
        .child(
            el("a")
                .class("skip-link")
                .attr("href", "#content")
                .child("Skip to content"),
        )
        .child(
            el("header")
                .class("site-header")
                .child(brand())
                .child(
                    el("nav")
                        .class("nav")
                        .attr("aria-label", "Primary navigation")
                        .child(el("a").attr("href", "#synopsis").child("Synopsis"))
                        .child(el("a").attr("href", "#credits").child("Credits"))
                        .child(el("a").attr("href", "#premiere").child("Premiere")),
                ),
        )
        .child(
            el("main")
                .id("content")
                .child(
                    el("section")
                        .class("scene")
                        .attr("aria-labelledby", "film-title")
                        .child(
                            el("picture").class("scene__media").child(
                                el("img")
                                    .class("scene__image")
                                    .attr("src", "/assets/afterlight-scene.jpg")
                                    .attr("width", "1672")
                                    .attr("height", "941")
                                    .attr("alt", "An illuminated organic vessel inside a flooded archive")
                                    .attr("fetchpriority", "high")
                                    .attr("decoding", "async"),
                            ),
                        )
                        .child(el("div").class("scene__veil").attr("aria-hidden", "true"))
                        .child(el("div").class("scene__scan").attr("aria-hidden", "true"))
                        .child(el("div").class("scene__signal").attr("aria-hidden", "true"))
                        .child(
                            el("div")
                                .class("scene__layout")
                                .child(
                                    el("div")
                                        .class("scene__content")
                                        .child(
                                            el("p")
                                                .class("eyebrow")
                                                .child("World premiere / Medellin / 12.10.26"),
                                        )
                                        .child(
                                            el("h1")
                                                .id("film-title")
                                                .child(el("span").child("After"))
                                                .child(el("em").child("light")),
                                        )
                                        .child(
                                            el("div")
                                                .class("scene__intro")
                                                .child(el("p").child(
                                                    "When the last city loses the night, an archivist follows a signal into the flooded rooms beneath it.",
                                                ))
                                                .child(
                                                    el("a")
                                                        .class("button")
                                                        .attr("href", "#synopsis")
                                                        .child("Enter the story"),
                                                ),
                                        ),
                                )
                                .child(
                                    el("aside")
                                        .class("scene__side")
                                        .attr("aria-label", "Film credits")
                                        .child(
                                            el("p")
                                                .class("scene__credits")
                                                .child(el("span").child("A film by Mara Venn"))
                                                .child("With Ilya Noor and Ren Ito. Original score by Sable Current."),
                                        )
                                        .child(el("p").class("cue").child("Scroll to descend")),
                                ),
                        ),
                )
                .child(
                    el("section")
                        .class("story")
                        .id("synopsis")
                        .attr("aria-labelledby", "synopsis-title")
                        .child(el("p").class("section-label").child("01 / Synopsis"))
                        .child(
                            el("div")
                                .class("story__body")
                                .child(
                                    el("h2")
                                        .id("synopsis-title")
                                        .child("The signal arrived seven years late."),
                                )
                                .child(
                                    el("div")
                                        .class("story__copy")
                                        .child(el("p").child(
                                            "Light has become permanent. Beneath the white sky, Iona preserves memories of darkness for a generation that has never seen the stars.",
                                        ))
                                        .child(el("p").child(
                                            "A transmission from the drowned archive carries her own voice. To answer it, she must enter a place abandoned before she was born and decide which histories deserve to survive.",
                                        )),
                                ),
                        ),
                )
                .child(
                    el("section")
                        .class("statement")
                        .attr("aria-label", "Director statement")
                        .child(
                            el("blockquote")
                                .child("We remember the dark because it taught us how to ")
                                .child(el("em").child("look.")),
                        ),
                )
                .child(
                    el("section")
                        .class("credits")
                        .id("credits")
                        .attr("aria-labelledby", "credits-title")
                        .child(
                            el("div")
                                .class("credits__header")
                                .child(
                                    el("div")
                                        .child(el("p").class("section-label").child("02 / The film"))
                                        .child(el("h2").id("credits-title").child("Built in the half-light.")),
                                )
                                .child(el("p").child(
                                    "A fictional production scaffold for films, studios, festivals, and visual storytellers.",
                                )),
                        )
                        .child(
                            el("div")
                                .class("crew")
                                .child(credit("Director", "Mara Venn"))
                                .child(credit("Photography", "Osei Kade"))
                                .child(credit("Production", "North Room"))
                                .child(credit("Score", "Sable Current"))
                                .child(credit("Runtime", "104 minutes"))
                                .child(credit("Format", "1.85 : 1 / Color"))
                                .child(credit("Language", "English / Spanish"))
                                .child(credit("Release", "Autumn 2026")),
                        ),
                )
                .child(
                    el("section")
                        .class("screening")
                        .id("premiere")
                        .attr("aria-labelledby", "premiere-title")
                        .child(
                            el("div")
                                .child(el("p").class("screening__meta").child("One night / Limited seats"))
                                .child(el("h2").id("premiere-title").child("See it before daylight.")),
                        )
                        .child(
                            el("div")
                                .class("screening__aside")
                                .child(el("p").child(
                                    "Teatro Metropolitano, Medellin. Doors at 19:30. Screening at 20:15, followed by a conversation with the director.",
                                ))
                                .child(
                                    el("a")
                                        .class("button")
                                        .attr("href", "mailto:premiere@example.com")
                                        .child("Request an invitation"),
                                ),
                        ),
                ),
        )
        .child(
            el("footer")
                .class("site-footer")
                .child(el("strong").child("AFTERLIGHT"))
                .child("Cinematic starter / Built with PliegoRS / 2026"),
        )
        .into_view()
}

fn credit(role: &str, name: &str) -> View {
    el("div")
        .child(
            el("dl")
                .child(el("dt").child(role.to_owned()))
                .child(el("dd").child(name.to_owned())),
        )
        .into_view()
}

fn not_found() -> View {
    el("main")
        .class("not-found")
        .child(
            el("section")
                .class("not-found__content")
                .child(el("p").class("not-found__code").child("404 / Signal lost"))
                .child(el("h1").child("This frame was never exposed."))
                .child(el("p").child(
                    "The page you requested is outside the archive. Return to the opening frame and begin again.",
                ))
                .child(el("a").class("button").attr("href", "/").child("Return to AFTERLIGHT")),
        )
        .into_view()
}

fn home_head() -> Head {
    let description = "AFTERLIGHT is a cinematic PliegoRS starter for films, studios, festivals, and visual storytellers.";
    Head::new("AFTERLIGHT | Cinematic PliegoRS Starter")
        .description(description)
        .canonical(format!("{SITE_URL}/"))
        .icon("/favicon.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/favicon.svg")
        .stylesheet("/assets/site.css")
        .meta("generator", "PliegoRS")
        .meta("theme-color", "#090c0d")
        .meta("robots", "index, follow, max-image-preview:large")
        .meta("author", "Celiums Solutions LLC")
        .meta("twitter:card", "summary_large_image")
        .meta("twitter:title", "AFTERLIGHT | A Cinematic PliegoRS Starter")
        .meta("twitter:description", description)
        .meta(
            "twitter:image",
            format!("{SITE_URL}/assets/afterlight-scene.jpg"),
        )
        .property_meta("og:type", "website")
        .property_meta("og:site_name", "AFTERLIGHT")
        .property_meta("og:title", "AFTERLIGHT | A Cinematic PliegoRS Starter")
        .property_meta("og:description", description)
        .property_meta("og:url", format!("{SITE_URL}/"))
        .property_meta(
            "og:image",
            format!("{SITE_URL}/assets/afterlight-scene.jpg"),
        )
        .property_meta("og:image:width", "1672")
        .property_meta("og:image:height", "941")
        .property_meta(
            "og:image:alt",
            "An illuminated organic vessel inside a flooded archive",
        )
        .json_ld(json!({
            "@context": "https://schema.org",
            "@type": "Movie",
            "name": "AFTERLIGHT",
            "description": description,
            "url": format!("{SITE_URL}/"),
            "image": format!("{SITE_URL}/assets/afterlight-scene.jpg"),
            "director": {"@type": "Person", "name": "Mara Venn"},
            "dateCreated": "2026",
            "inLanguage": ["en", "es"]
        }))
}

fn not_found_head() -> Head {
    let description = "The requested frame could not be found in the AFTERLIGHT archive.";
    Head::new("Page not found | AFTERLIGHT")
        .description(description)
        .canonical(format!("{SITE_URL}/404.html"))
        .icon("/favicon.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/favicon.svg")
        .stylesheet("/assets/site.css")
        .meta("generator", "PliegoRS")
        .meta("theme-color", "#090c0d")
        .meta("robots", "noindex, follow")
        .property_meta("og:type", "website")
        .property_meta("og:title", "Page not found | AFTERLIGHT")
        .property_meta("og:description", description)
        .property_meta("og:url", format!("{SITE_URL}/404.html"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));

    let report = Site::new()
        .page(Page::new("/", home_head(), home()).language("en"))
        .page(Page::new("/404.html", not_found_head(), not_found()).language("en"))
        .asset(Asset::new("assets/site.css", CSS.to_vec()))
        .asset(Asset::new(
            "assets/afterlight-scene.jpg",
            include_bytes!("../assets/afterlight-scene.jpg").to_vec(),
        ))
        .asset(Asset::new(
            "assets/fonts/instrument-sans-variable.woff2",
            include_bytes!("../assets/fonts/instrument-sans-variable.woff2").to_vec(),
        ))
        .asset(Asset::new(
            "assets/fonts/instrument-serif-regular.woff2",
            include_bytes!("../assets/fonts/instrument-serif-regular.woff2").to_vec(),
        ))
        .asset(Asset::new(
            "assets/fonts/instrument-serif-italic.woff2",
            include_bytes!("../assets/fonts/instrument-serif-italic.woff2").to_vec(),
        ))
        .asset(Asset::new(
            "assets/fonts/fragment-mono-regular.woff2",
            include_bytes!("../assets/fonts/fragment-mono-regular.woff2").to_vec(),
        ))
        .asset(Asset::new("favicon.svg", FAVICON.as_bytes().to_vec()))
        .asset(Asset::new("site.webmanifest", MANIFEST.as_bytes().to_vec()))
        .asset(Asset::new("robots.txt", ROBOTS.as_bytes().to_vec()))
        .asset(Asset::new("sitemap.xml", SITEMAP.as_bytes().to_vec()))
        .build(&output)?;

    println!(
        "PLIEGO cinematic build: {} files -> {}",
        report.receipt.outputs.files.len(),
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_dom::render_html;

    #[test]
    fn home_has_its_primary_story_and_navigation() {
        let html = render_html(&home());
        assert!(html.contains("id=\"film-title\""));
        assert!(html.contains("id=\"synopsis\""));
        assert!(html.contains("id=\"credits\""));
        assert!(html.contains("id=\"premiere\""));
    }

    #[test]
    fn metadata_is_complete_and_script_free() {
        let html = Page::new("/", home_head(), home()).render().unwrap();
        assert!(html.contains("rel=\"canonical\""));
        assert!(html.contains("property=\"og:image\""));
        assert!(html.contains("application/ld+json"));
        assert!(!html.contains("<script type=\"module\""));
    }

    #[test]
    fn not_found_is_not_indexed() {
        let html = Page::new("/404.html", not_found_head(), not_found())
            .render()
            .unwrap();
        assert!(html.contains("content=\"noindex, follow\""));
        assert!(html.contains("Return to AFTERLIGHT"));
    }
}
