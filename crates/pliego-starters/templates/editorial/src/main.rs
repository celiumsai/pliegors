// SPDX-License-Identifier: Apache-2.0

use pliego_dom::{Element, IntoView, View, el};
use pliego_ssg::{Asset, Head, Page, Site};
use serde_json::json;
use std::path::PathBuf;

const SITE_URL: &str = "https://editorial.pliegors.dev";
const DESCRIPTION: &str = "A field review of matter, memory, and the people preserving both.";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));

    Site::new()
        .page(Page::new("/", home_head(), home()).language("en"))
        .page(Page::new("/404.html", not_found_head(), not_found()).language("en"))
        .asset(source_asset(
            "assets/site.css",
            include_bytes!("../assets/site.css"),
        ))
        .asset(source_asset(
            "assets/fonts/instrument-sans-variable.woff2",
            include_bytes!("../assets/fonts/instrument-sans-variable.woff2"),
        ))
        .asset(source_asset(
            "assets/fonts/instrument-serif-regular.woff2",
            include_bytes!("../assets/fonts/instrument-serif-regular.woff2"),
        ))
        .asset(source_asset(
            "assets/fonts/instrument-serif-italic.woff2",
            include_bytes!("../assets/fonts/instrument-serif-italic.woff2"),
        ))
        .asset(source_asset(
            "assets/fonts/fragment-mono-regular.woff2",
            include_bytes!("../assets/fonts/fragment-mono-regular.woff2"),
        ))
        .asset(source_asset(
            "assets/images/hero.jpg",
            include_bytes!("../assets/images/hero.jpg"),
        ))
        .asset(source_asset(
            "assets/images/study.jpg",
            include_bytes!("../assets/images/study.jpg"),
        ))
        .asset(source_asset(
            "assets/images/archive.jpg",
            include_bytes!("../assets/images/archive.jpg"),
        ))
        .asset(source_asset(
            "favicon.svg",
            include_bytes!("../assets/favicon.svg"),
        ))
        .asset(source_asset(
            "site.webmanifest",
            include_bytes!("../assets/site.webmanifest"),
        ))
        .asset(source_asset(
            "robots.txt",
            include_bytes!("../assets/robots.txt"),
        ))
        .asset(source_asset(
            "sitemap.xml",
            include_bytes!("../assets/sitemap.xml"),
        ))
        .build(output)?;

    Ok(())
}

fn source_asset(path: &str, bytes: &[u8]) -> Asset {
    Asset::new(path, bytes.to_vec())
}

fn home_head() -> Head {
    base_head(
        "The Observed Field | Independent material culture review",
        DESCRIPTION,
        SITE_URL,
    )
    .meta("robots", "index,follow,max-image-preview:large")
    .json_ld(json!({
        "@context": "https://schema.org",
        "@type": "WebSite",
        "name": "The Observed Field",
        "url": SITE_URL,
        "description": DESCRIPTION,
        "publisher": {
            "@type": "Organization",
            "name": "The Observed Field",
            "url": SITE_URL
        }
    }))
    .json_ld(json!({
        "@context": "https://schema.org",
        "@type": "Periodical",
        "name": "The Observed Field",
        "issn": "0000-0000",
        "inLanguage": "en",
        "url": SITE_URL
    }))
}

fn not_found_head() -> Head {
    base_head(
        "Page not found | The Observed Field",
        "The requested page could not be found.",
        &format!("{SITE_URL}/404.html"),
    )
    .meta("robots", "noindex,follow")
}

fn base_head(title: &str, description: &str, canonical: &str) -> Head {
    Head::new(title)
        .description(description)
        .canonical(canonical)
        .icon("/favicon.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/favicon.svg")
        .stylesheet("/assets/site.css")
        .meta("generator", "PliegoRS")
        .meta("theme-color", "#f1eee4")
        .meta("color-scheme", "light dark")
        .meta("twitter:card", "summary_large_image")
        .meta("twitter:title", title)
        .meta("twitter:description", description)
        .meta(
            "twitter:image",
            format!("{SITE_URL}/assets/images/hero.jpg"),
        )
        .property_meta("og:type", "website")
        .property_meta("og:site_name", "The Observed Field")
        .property_meta("og:title", title)
        .property_meta("og:description", description)
        .property_meta("og:url", canonical)
        .property_meta("og:image", format!("{SITE_URL}/assets/images/hero.jpg"))
        .property_meta(
            "og:image:alt",
            "A conservator measuring specimens at an illuminated field bench",
        )
        .property_meta("og:image:width", "1536")
        .property_meta("og:image:height", "1024")
}

fn home() -> View {
    el("div")
        .class("site-shell")
        .child(
            el("a")
                .class("skip-link")
                .attr("href", "#main")
                .child("Skip to the journal"),
        )
        .child(issue_bar())
        .child(site_header())
        .child(
            el("main")
                .id("main")
                .child(hero())
                .child(editorial_note())
                .child(dispatches())
                .child(interlude())
                .child(issue_index())
                .child(field_note())
                .child(colophon()),
        )
        .child(site_footer())
        .into_view()
}

fn issue_bar() -> Element {
    el("div")
        .class("issue-bar")
        .child(el("p").child("Independent review of material culture"))
        .child(el("p").child("Issue 07 / Autumn 2026"))
}

fn site_header() -> Element {
    el("header")
        .class("site-header")
        .child(
            el("a")
                .class("wordmark")
                .attr("href", "/")
                .attr("aria-label", "The Observed Field, home")
                .child(brand_mark())
                .child(el("span").child("The Observed Field")),
        )
        .child(
            el("nav")
                .attr("aria-label", "Primary")
                .child(el("a").attr("href", "#dispatches").child("Dispatches"))
                .child(el("a").attr("href", "#issue").child("Issue 07"))
                .child(el("a").attr("href", "#colophon").child("Colophon")),
        )
}

fn brand_mark() -> View {
    el("svg")
        .attr("viewBox", "0 0 48 48")
        .attr("aria-hidden", "true")
        .child(el("path").attr("d", "M5 5h38v38H5z"))
        .child(
            el("circle")
                .attr("cx", "24")
                .attr("cy", "24")
                .attr("r", "9"),
        )
        .child(el("path").attr("d", "M24 5v10M24 33v10M5 24h10M33 24h10"))
        .into_view()
}

fn hero() -> Element {
    el("section")
        .class("hero")
        .attr("aria-labelledby", "hero-title")
        .child(
            el("div")
                .class("hero-copy")
                .child(el("p").class("kicker").child("Essay / Methods of keeping"))
                .child(
                    el("h1")
                        .id("hero-title")
                        .child(el("span").child("The distance"))
                        .child(el("span").class("serif-line").child("between an object"))
                        .child(el("span").class("offset-line").child("and its memory.")),
                )
                .child(
                    el("div")
                        .class("hero-deck")
                        .child(el("p").child(
                            "Inside a disappearing archive, care is measured in millimeters, humidity, and time.",
                        ))
                        .child(
                            el("a")
                                .class("read-link")
                                .attr("href", "#field-note")
                                .child("Read the cover story")
                                .child(el("span").attr("aria-hidden", "true").child("↘")),
                        ),
                ),
        )
        .child(
            el("figure")
                .class("hero-figure")
                .child(
                    el("img")
                        .attr("src", "/assets/images/hero.jpg")
                        .attr("width", "1536")
                        .attr("height", "1024")
                        .attr("fetchpriority", "high")
                        .attr("alt", "A conservator measures translucent specimens at an illuminated field bench"),
                )
                .child(
                    el("figcaption")
                        .child(el("span").child("Field Station 12, Lower Delta"))
                        .child(el("span").child("Photograph / Archive 6-14")),
                ),
        )
}

fn editorial_note() -> Element {
    el("section")
        .class("editorial-note")
        .attr("aria-labelledby", "editor-note-title")
        .child(
            el("div")
                .class("section-number")
                .attr("aria-hidden", "true")
                .child("00"),
        )
        .child(
            el("div")
                .class("note-heading")
                .child(el("p").class("kicker").child("From the editor"))
                .child(el("h2").id("editor-note-title").child("Evidence has a texture.")),
        )
        .child(
            el("div")
                .class("prose-columns")
                .child(el("p").child(
                    "This issue studies the quiet systems behind preservation: drawers, labels, rituals, and the stubborn human instinct to keep a record.",
                ))
                .child(el("p").child(
                    "We visited six archives where objects are not treated as inert things, but as witnesses whose stories change with every pair of hands.",
                )),
        )
}

fn dispatches() -> Element {
    el("section")
        .id("dispatches")
        .class("dispatches")
        .attr("aria-labelledby", "dispatch-title")
        .child(section_heading("01", "Latest dispatches", "dispatch-title"))
        .child(
            el("div")
                .class("dispatch-grid")
                .child(dispatch(
                    "01",
                    "Preservation",
                    "The glass root",
                    "A seed library holds the last viable specimen from a vanished coastline.",
                    "/assets/images/study.jpg",
                    "A preserved seed and its roots inside a brass and glass vessel",
                ))
                .child(dispatch(
                    "02",
                    "Indexing",
                    "A grammar for dust",
                    "What catalog numbers reveal, and what they quietly remove from an object's history.",
                    "/assets/images/archive.jpg",
                    "An overhead archive table with field documents, plant specimens, and instruments",
                ))
                .child(
                    el("article")
                        .class("dispatch dispatch-text")
                        .child(el("p").class("dispatch-number").child("03"))
                        .child(el("p").class("kicker").child("Oral history"))
                        .child(el("h3").child("The last room with a key"))
                        .child(el("p").child(
                            "Three custodians describe the daily work of maintaining a collection nobody owns.",
                        ))
                        .child(el("a").attr("href", "#issue").child("9 minute read")),
                ),
        )
}

fn dispatch(
    number: &str,
    category: &str,
    title: &str,
    summary: &str,
    image: &str,
    alt: &str,
) -> Element {
    el("article")
        .class("dispatch")
        .child(
            el("figure")
                .child(
                    el("img")
                        .attr("src", image)
                        .attr("width", "1536")
                        .attr("height", "1024")
                        .attr("loading", "lazy")
                        .attr("alt", alt),
                )
                .child(el("figcaption").child(format!("Plate {number} / Issue 07"))),
        )
        .child(
            el("div")
                .class("dispatch-copy")
                .child(el("p").class("kicker").child(category.to_owned()))
                .child(el("h3").child(title.to_owned()))
                .child(el("p").child(summary.to_owned()))
                .child(el("a").attr("href", "#issue").child("Read essay")),
        )
}

fn interlude() -> Element {
    el("aside")
        .class("interlude")
        .attr("aria-label", "Editorial statement")
        .child(el("p").class("kicker").child("Field principle 04"))
        .child(
            el("blockquote")
                .child("An archive is not where memory rests.")
                .child(el("em").child("It is where memory is negotiated.")),
        )
        .child(
            el("p")
                .class("interlude-credit")
                .child("— Notes from Station 12"),
        )
}

fn issue_index() -> Element {
    el("section")
        .id("issue")
        .class("issue-index")
        .attr("aria-labelledby", "issue-title")
        .child(section_heading("02", "Inside issue 07", "issue-title"))
        .child(
            el("ol")
                .class("story-list")
                .child(story_row(
                    "018",
                    "Essay",
                    "Against perfect records",
                    "Nora Vale",
                    "12 min",
                ))
                .child(story_row(
                    "036",
                    "Visit",
                    "A room calibrated for rain",
                    "Ilan Mori",
                    "8 min",
                ))
                .child(story_row(
                    "052",
                    "Portfolio",
                    "Six instruments for listening",
                    "M. Okafor",
                    "14 min",
                ))
                .child(story_row(
                    "074",
                    "Conversation",
                    "Who inherits the catalog?",
                    "E. Noor",
                    "11 min",
                ))
                .child(story_row(
                    "096",
                    "Field note",
                    "The glass root",
                    "Sofia Calder",
                    "7 min",
                )),
        )
}

fn story_row(page: &str, kind: &str, title: &str, author: &str, length: &str) -> Element {
    el("li")
        .child(el("span").class("story-page").child(page.to_owned()))
        .child(el("span").class("story-kind").child(kind.to_owned()))
        .child(el("h3").child(el("a").attr("href", "#field-note").child(title.to_owned())))
        .child(el("span").class("story-author").child(author.to_owned()))
        .child(el("span").class("story-length").child(length.to_owned()))
}

fn field_note() -> Element {
    el("article")
        .id("field-note")
        .class("field-note")
        .attr("aria-labelledby", "field-note-title")
        .child(
            el("div")
                .class("field-image")
                .child(
                    el("img")
                        .attr("src", "/assets/images/archive.jpg")
                        .attr("width", "1536")
                        .attr("height", "1024")
                        .attr("loading", "lazy")
                        .attr("alt", "An archive worktable arranged with documents, maps, specimens, and measuring instruments"),
                )
                .child(el("p").child("Inventory before transfer / Table 4B")),
        )
        .child(
            el("div")
                .class("field-copy")
                .child(el("p").class("kicker").child("Field note / Lower Delta"))
                .child(el("h2").id("field-note-title").child("Every label is an argument."))
                .child(el("p").class("drop-cap").child(
                    "At seven in the morning the archive is already warmer than the street. Mara unlocks the steel cabinet and waits before touching anything. The pause is part of the protocol.",
                ))
                .child(el("p").child(
                    "Objects enter as evidence, but the catalog asks them to become facts. A date. A place. A material. Each field closes one question and opens another: whose date, which place, named by whom?",
                ))
                .child(el("p").child(
                    "The work here is not to make a perfect record. It is to leave enough room in the record for the next witness.",
                ))
                .child(
                    el("div")
                        .class("byline")
                        .child(el("span").child("Words / Sofia Calder"))
                        .child(el("span").child("Research / Ana Meza")),
                ),
        )
}

fn colophon() -> Element {
    el("section")
        .id("colophon")
        .class("colophon")
        .attr("aria-labelledby", "colophon-title")
        .child(
            el("div")
                .class("colophon-title")
                .child(el("p").class("kicker").child("Independent since 2019"))
                .child(el("h2").id("colophon-title").child("Receive the next field letter.")),
        )
        .child(
            el("div")
                .class("colophon-action")
                .child(el("p").child(
                    "One considered dispatch each month. No feed, no noise, and no tracking pixels.",
                ))
                .child(
                    el("a")
                        .class("subscribe-link")
                        .attr("href", "mailto:letters@example.com?subject=Field%20letter")
                        .child("Join the field letter")
                        .child(el("span").attr("aria-hidden", "true").child("↗")),
                ),
        )
}

fn section_heading(number: &str, title: &str, id: &str) -> Element {
    el("header")
        .class("section-heading")
        .child(el("p").attr("aria-hidden", "true").child(number.to_owned()))
        .child(el("h2").id(id.to_owned()).child(title.to_owned()))
}

fn site_footer() -> Element {
    el("footer")
        .class("site-footer")
        .child(
            el("div")
                .class("footer-mark")
                .child(brand_mark())
                .child(el("p").child("The Observed Field")),
        )
        .child(
            el("div")
                .class("footer-meta")
                .child(el("p").child("Medellin / Worldwide"))
                .child(el("p").child("Built with PliegoRS")),
        )
        .child(el("p").class("footer-year").child("2026 / 07"))
}

fn not_found() -> View {
    el("div")
        .class("not-found")
        .child(
            el("header")
                .class("not-found-header")
                .child(
                    el("a")
                        .class("wordmark")
                        .attr("href", "/")
                        .child(brand_mark())
                        .child(el("span").child("The Observed Field")),
                )
                .child(el("p").child("Error / 404")),
        )
        .child(
            el("main")
                .child(
                    el("p")
                        .class("error-code")
                        .attr("aria-hidden", "true")
                        .child("404"),
                )
                .child(el("h1").child("This record is missing."))
                .child(
                    el("p")
                        .child("The page may have been moved, renamed, or returned to the field."),
                )
                .child(
                    el("a")
                        .class("read-link")
                        .attr("href", "/")
                        .child("Return to issue 07")
                        .child(el("span").attr("aria-hidden", "true").child("↖")),
                ),
        )
        .into_view()
}
