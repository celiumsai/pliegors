use crate::content::{Catalog, House, JournalEntry, Ritual};
use pliego_content::Entry;
use pliego_dom::{IntoView, View, el};

pub fn layout(active: &str, main: View) -> View {
    el("div")
        .class("site-shell")
        .child(
            el("header").class("site-header").child(
                el("div")
                    .class("site-header__inner")
                    .child(
                        el("a")
                            .class("wordmark")
                            .attr("href", "/")
                            .attr("aria-label", "Native Content Ledger home")
                            .child(
                                el("span")
                                    .class("wordmark__mark")
                                    .attr("aria-hidden", "true"),
                            )
                            .child(el("span").child("NATIVE CONTENT LEDGER")),
                    )
                    .child(
                        el("nav")
                            .class("site-nav")
                            .attr("aria-label", "Collections")
                            .child(nav_link(active, "rituals", "/rituals", "Rituals"))
                            .child(nav_link(active, "houses", "/houses", "Houses"))
                            .child(nav_link(active, "journal", "/journal", "Journal")),
                    ),
            ),
        )
        .child(main)
        .child(
            el("footer").class("site-footer").child(
                el("div")
                    .class("site-footer__inner")
                    .child(el("p").child("A real corpus, folded into authored Rust views."))
                    .child(
                        el("p")
                            .class("signature")
                            .child("Built with PliegoRS")
                            .child(el("span").attr("aria-hidden", "true").child("P")),
                    ),
            ),
        )
        .into_view()
}

pub fn home(catalog: &Catalog) -> View {
    let ritual_fingerprint = fingerprint_prefix(&catalog.rituals);
    let house_fingerprint = fingerprint_prefix(&catalog.houses);
    let journal_fingerprint = fingerprint_prefix(&catalog.journal);
    el("main")
        .child(
            el("section")
                .class("ledger-hero")
                .child(
                    el("img")
                        .class("ledger-hero__image")
                        .attr("src", "/assets/reference/hero.jpg")
                        .attr(
                            "alt",
                            "Mineral bathhouse beneath a circular opening in the roof.",
                        )
                        .attr("width", "1536")
                        .attr("height", "1024")
                        .attr("fetchpriority", "high"),
                )
                .child(el("div").class("ledger-hero__veil"))
                .child(
                    el("div")
                        .class("ledger-hero__content")
                        .child(el("p").class("eyebrow").child("PLIEGORS / AU-01 PROOF"))
                        .child(el("h1").child("Content is a first-class Rust input."))
                        .child(el("p").class("ledger-hero__lede").child(
                            "A neutral authored corpus. Typed metadata, safe CommonMark, deterministic identity, and no generated page source.",
                        ))
                        .child(
                            el("a")
                                .class("text-link text-link--light")
                                .attr("href", "/rituals")
                                .child("Open the corpus")
                                .child(el("span").attr("aria-hidden", "true").child("->")),
                        ),
                )
                .child(
                    el("dl")
                        .class("ledger-hero__stats")
                        .child(stat("Entries", &catalog.total_entries().to_string()))
                        .child(stat("Formats", "MD + YAML"))
                        .child(stat("Renderer", "Rust")),
                ),
        )
        .child(
            el("section")
                .class("collection-band")
                .attr("aria-labelledby", "collections-heading")
                .child(
                    el("div")
                        .class("section-heading")
                        .child(el("p").class("eyebrow eyebrow--dark").child("LOADED AT BUILD TIME"))
                        .child(el("h2").id("collections-heading").child("Three typed collections"))
                        .child(el("p").child(
                            "The loader owns discovery and diagnostics. The application owns routes, layout, and visual meaning.",
                        )),
                )
                .child(
                    el("div")
                        .class("collection-ledger")
                        .child(collection_row(
                            "01",
                            "Rituals",
                            catalog.rituals.len(),
                            "Temperature, duration, sensory notes, and safety.",
                            &ritual_fingerprint,
                            "/rituals",
                        ))
                        .child(collection_row(
                            "02",
                            "Houses",
                            catalog.houses.len(),
                            "Setting, access, hours, amenities, and signatures.",
                            &house_fingerprint,
                            "/houses",
                        ))
                        .child(collection_row(
                            "03",
                            "Journal",
                            catalog.journal.len(),
                            "Publication metadata and safe Markdown bodies.",
                            &journal_fingerprint,
                            "/journal",
                        )),
                ),
        )
        .into_view()
}

pub fn collection_intro(index: &str, title: &str, description: &str, count: usize) -> View {
    el("section")
        .class("collection-intro")
        .child(
            el("div")
                .class("collection-intro__index")
                .child(index)
                .child(el("span").child(format!("{count:02} records"))),
        )
        .child(
            el("div")
                .class("collection-intro__copy")
                .child(
                    el("p")
                        .class("eyebrow eyebrow--dark")
                        .child("DIRECTORY-BACKED COLLECTION"),
                )
                .child(el("h1").child(title))
                .child(el("p").child(description)),
        )
        .into_view()
}

pub fn ritual_card(entry: &Entry<Ritual>) -> View {
    let ritual = entry.data();
    el("article")
        .class("record-card record-card--ritual")
        .child(
            el("a")
                .class("record-card__media")
                .attr("href", format!("/rituals/{}", entry.id()))
                .child(
                    el("img")
                        .attr("src", ritual.image.clone())
                        .attr("alt", ritual.image_alt.clone())
                        .attr("loading", "lazy")
                        .attr("decoding", "async"),
                )
                .child(
                    el("span")
                        .class("record-card__number")
                        .child(format!("{:02}", ritual.order)),
                ),
        )
        .child(
            el("div")
                .class("record-card__body")
                .child(
                    el("div")
                        .class("record-card__meta")
                        .child(el("span").child(ritual.kind.clone()))
                        .child(el("span").child(ritual.temperature.clone()))
                        .child(el("span").child(ritual.duration.clone())),
                )
                .child(
                    el("h2").child(
                        el("a")
                            .attr("href", format!("/rituals/{}", entry.id()))
                            .child(ritual.title.clone()),
                    ),
                )
                .child(el("p").child(ritual.excerpt.clone()))
                .child(
                    el("p")
                        .class("record-card__benefit")
                        .child(ritual.benefit.clone()),
                )
                .child(featured_flag(ritual.featured)),
        )
        .into_view()
}

pub fn house_card(entry: &Entry<House>) -> View {
    let house = entry.data();
    el("article")
        .class("record-card record-card--house")
        .child(
            el("a")
                .class("record-card__media")
                .attr("href", format!("/houses/{}", entry.id()))
                .child(
                    el("img")
                        .attr("src", house.image.clone())
                        .attr("alt", house.image_alt.clone())
                        .attr("loading", "lazy")
                        .attr("decoding", "async"),
                )
                .child(
                    el("span")
                        .class("record-card__number")
                        .child(format!("{:02}", house.order)),
                ),
        )
        .child(
            el("div")
                .class("record-card__body")
                .child(
                    el("p")
                        .class("record-card__kicker")
                        .child(house.setting.clone()),
                )
                .child(
                    el("h2").child(
                        el("a")
                            .attr("href", format!("/houses/{}", entry.id()))
                            .child(house.title.clone()),
                    ),
                )
                .child(el("p").child(house.descriptor.clone()))
                .child(
                    el("p")
                        .class("record-card__benefit")
                        .child("Signature / ")
                        .child(house.signature.clone()),
                ),
        )
        .into_view()
}

pub fn journal_card(entry: &Entry<JournalEntry>) -> View {
    let article = entry.data();
    el("article")
        .class("journal-row")
        .child(
            el("a")
                .class("journal-row__media")
                .attr("href", format!("/journal/{}", entry.id()))
                .child(
                    el("img")
                        .attr("src", article.image.clone())
                        .attr("alt", article.image_alt.clone())
                        .attr("loading", "lazy")
                        .attr("decoding", "async"),
                ),
        )
        .child(
            el("div")
                .class("journal-row__body")
                .child(
                    el("p")
                        .class("record-card__meta")
                        .child(el("span").child(article.publish_date.clone()))
                        .child(el("span").child(article.category.clone()))
                        .child(el("span").child(article.read_time.clone())),
                )
                .child(
                    el("h2").child(
                        el("a")
                            .attr("href", format!("/journal/{}", entry.id()))
                            .child(article.title.clone()),
                    ),
                )
                .child(el("p").child(article.description.clone()))
                .child(
                    el("p")
                        .class("journal-row__byline")
                        .child(format!("By {}", article.author)),
                )
                .child(featured_flag(article.featured)),
        )
        .into_view()
}

pub fn ritual_detail(entry: &Entry<Ritual>, body: View) -> View {
    let ritual = entry.data();
    let mut sensory = el("ul").class("detail-list");
    for value in &ritual.sensory {
        sensory = sensory.child(el("li").child(value.clone()));
    }
    let mut best_for = el("ul").class("detail-list");
    for value in &ritual.best_for {
        best_for = best_for.child(el("li").child(value.clone()));
    }
    detail_shell(
        "Ritual",
        &ritual.title,
        &ritual.excerpt,
        &ritual.image,
        &ritual.image_alt,
        el("div")
            .class("detail-facts")
            .child(fact("Type", &ritual.kind))
            .child(fact("Temperature", &ritual.temperature))
            .child(fact("Duration", &ritual.duration))
            .child(fact("Benefit", &ritual.benefit))
            .child(fact("Featured", if ritual.featured { "Yes" } else { "No" }))
            .into_view(),
        el("div")
            .class("detail-aside")
            .child(
                el("section")
                    .child(el("h2").child("Sensory register"))
                    .child(sensory),
            )
            .child(
                el("section")
                    .child(el("h2").child("Best for"))
                    .child(best_for),
            )
            .child(optional_note("Safety", ritual.safety.as_deref()))
            .into_view(),
        body,
    )
}

pub fn house_detail(entry: &Entry<House>, body: View) -> View {
    let house = entry.data();
    let mut hours = el("ul").class("detail-list");
    for value in &house.hours {
        hours = hours.child(el("li").child(value.clone()));
    }
    let mut amenities = el("ul").class("detail-list detail-list--columns");
    for value in &house.amenities {
        amenities = amenities.child(el("li").child(value.clone()));
    }
    detail_shell(
        "House",
        &house.title,
        &house.excerpt,
        &house.image,
        &house.image_alt,
        el("div")
            .class("detail-facts")
            .child(fact("Setting", &house.setting))
            .child(fact("Signature", &house.signature))
            .child(fact("Address", &house.address))
            .child(fact("Sequence", &format!("{:02}", house.order)))
            .into_view(),
        el("div")
            .class("detail-aside")
            .child(el("section").child(el("h2").child("Hours")).child(hours))
            .child(
                el("section")
                    .child(el("h2").child("Amenities"))
                    .child(amenities),
            )
            .child(optional_note("Access", Some(&house.accessibility)))
            .into_view(),
        body,
    )
}

pub fn journal_detail(entry: &Entry<JournalEntry>, body: View) -> View {
    let article = entry.data();
    detail_shell(
        "Journal",
        &article.title,
        &article.description,
        &article.image,
        &article.image_alt,
        el("div")
            .class("detail-facts")
            .child(fact("Published", &article.publish_date))
            .child(fact("Author", &article.author))
            .child(fact("Category", &article.category))
            .child(fact("Reading", &article.read_time))
            .child(fact("Featured", if article.featured { "Yes" } else { "No" }))
            .into_view(),
        el("div")
            .class("detail-aside detail-aside--statement")
            .child(el("p").child(
                "The body below is parsed into a framework-neutral CommonMark event stream, then authored into escaped PliegoRS DOM nodes.",
            ))
            .into_view(),
        body,
    )
}

pub fn not_found() -> View {
    el("main")
        .class("not-found")
        .child(
            el("p")
                .class("eyebrow eyebrow--dark")
                .child("404 / NO CONTENT ID"),
        )
        .child(el("h1").child("This record is outside the fold."))
        .child(el("p").child("The source tree contains no entry for this route."))
        .child(
            el("a")
                .class("text-link")
                .attr("href", "/")
                .child("Return to the ledger")
                .child(el("span").attr("aria-hidden", "true").child("->")),
        )
        .into_view()
}

fn nav_link(active: &str, id: &str, href: &str, label: &str) -> View {
    let mut link = el("a")
        .attr("href", href)
        .attr("data-route", id)
        .child(label);
    if id == active {
        link = link.attr("aria-current", "page");
    }
    link.into_view()
}

fn stat(label: &str, value: &str) -> View {
    el("div")
        .child(el("dt").child(label))
        .child(el("dd").child(value))
        .into_view()
}

fn collection_row(
    index: &str,
    title: &str,
    count: usize,
    description: &str,
    fingerprint: &str,
    href: &str,
) -> View {
    el("a")
        .class("collection-row")
        .attr("href", href)
        .child(el("span").class("collection-row__index").child(index))
        .child(
            el("span")
                .class("collection-row__title")
                .child(title)
                .child(el("small").child(format!("{count} entries"))),
        )
        .child(
            el("span")
                .class("collection-row__description")
                .child(description),
        )
        .child(el("code").child(fingerprint.to_owned()))
        .child(
            el("span")
                .class("collection-row__arrow")
                .attr("aria-hidden", "true")
                .child("->"),
        )
        .into_view()
}

fn fingerprint_prefix<T>(collection: &pliego_content::Collection<T>) -> String {
    collection
        .snapshot()
        .iter()
        .next()
        .map(|(_, fingerprint)| fingerprint.as_str()[..12].to_owned())
        .unwrap_or_else(|| "empty".to_owned())
}

fn featured_flag(featured: bool) -> View {
    if featured {
        el("span")
            .class("featured-flag")
            .child("Featured record")
            .into_view()
    } else {
        View::Fragment(Vec::new())
    }
}

fn fact(label: &str, value: &str) -> View {
    el("div")
        .child(el("dt").child(label))
        .child(el("dd").child(value.to_owned()))
        .into_view()
}

fn optional_note(title: &str, value: Option<&str>) -> View {
    value.map_or_else(
        || View::Fragment(Vec::new()),
        |value| {
            el("section")
                .class("detail-note")
                .child(el("h2").child(title))
                .child(el("p").child(value.to_owned()))
                .into_view()
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn detail_shell(
    collection: &str,
    title: &str,
    description: &str,
    image: &str,
    image_alt: &str,
    facts: View,
    aside: View,
    body: View,
) -> View {
    el("main")
        .class("detail-page")
        .child(
            el("section")
                .class("detail-hero")
                .child(
                    el("img")
                        .attr("src", image.to_owned())
                        .attr("alt", image_alt.to_owned())
                        .attr("width", "1536")
                        .attr("height", "1024"),
                )
                .child(
                    el("div")
                        .class("detail-hero__copy")
                        .child(
                            el("p")
                                .class("eyebrow")
                                .child(format!("{collection} / typed entry")),
                        )
                        .child(el("h1").child(title.to_owned()))
                        .child(el("p").child(description.to_owned())),
                ),
        )
        .child(
            el("section")
                .class("detail-content")
                .child(el("dl").class("detail-content__facts").child(facts))
                .child(el("article").class("prose").child(body))
                .child(aside),
        )
        .into_view()
}
