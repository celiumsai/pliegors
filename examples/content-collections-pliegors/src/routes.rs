use crate::components;
use crate::content::Catalog;
use crate::markdown_view;
use pliego_dom::{IntoView, el};
use pliego_ssg::{Head, Page};

const SITE_URL: &str = "https://content-ledger.pliegors.dev";

pub fn pages(catalog: &Catalog) -> Result<Vec<Page>, String> {
    let mut pages = vec![page(
        "/",
        "Native Content Ledger / PliegoRS",
        "A real typed-content corpus built into authored Rust routes with PliegoRS.",
        "home",
        components::home(catalog),
    )];

    let mut rituals = catalog.rituals.iter().collect::<Vec<_>>();
    rituals.sort_by_key(|entry| entry.data().order);
    let mut ritual_grid = el("div").class("record-grid");
    for entry in &rituals {
        ritual_grid = ritual_grid.child(components::ritual_card(entry));
    }
    pages.push(page(
        "/rituals",
        "Rituals / Native Content Ledger",
        "Typed ritual records loaded from Markdown and YAML frontmatter.",
        "rituals",
        el("main")
            .class("collection-page")
            .child(components::collection_intro(
                "01",
                "Rituals",
                "Warmth, cold, movement, and rest represented as typed Rust data while the authored body remains safe CommonMark.",
                rituals.len(),
            ))
            .child(ritual_grid)
            .into_view(),
    ));
    for entry in rituals {
        let body = markdown_view::render(entry.markdown().ok_or_else(|| {
            format!("ritual {} did not contain a Markdown document", entry.id())
        })?)?;
        pages.push(page(
            &format!("/rituals/{}", entry.id()),
            &format!("{} / Rituals", entry.data().title),
            &entry.data().excerpt,
            "rituals",
            components::ritual_detail(entry, body),
        ));
    }

    let mut houses = catalog.houses.iter().collect::<Vec<_>>();
    houses.sort_by_key(|entry| entry.data().order);
    let mut house_grid = el("div").class("record-grid record-grid--houses");
    for entry in &houses {
        house_grid = house_grid.child(components::house_card(entry));
    }
    pages.push(page(
        "/houses",
        "Houses / Native Content Ledger",
        "Typed place records loaded from a deterministic content collection.",
        "houses",
        el("main")
            .class("collection-page")
            .child(components::collection_intro(
                "02",
                "Houses",
                "Place records keep their own address, hours, amenities, access notes, imagery, and narrative body without route code duplication.",
                houses.len(),
            ))
            .child(house_grid)
            .into_view(),
    ));
    for entry in houses {
        let body =
            markdown_view::render(entry.markdown().ok_or_else(|| {
                format!("house {} did not contain a Markdown document", entry.id())
            })?)?;
        pages.push(page(
            &format!("/houses/{}", entry.id()),
            &format!("{} / Houses", entry.data().title),
            &entry.data().descriptor,
            "houses",
            components::house_detail(entry, body),
        ));
    }

    let mut journal = catalog.journal.iter().collect::<Vec<_>>();
    journal.sort_by(|left, right| right.data().publish_date.cmp(&left.data().publish_date));
    let mut journal_list = el("div").class("journal-list");
    for entry in &journal {
        journal_list = journal_list.child(components::journal_card(entry));
    }
    pages.push(page(
        "/journal",
        "Journal / Native Content Ledger",
        "Typed journal entries rendered through safe PliegoRS Markdown events.",
        "journal",
        el("main")
            .class("collection-page")
            .child(components::collection_intro(
                "03",
                "Journal",
                "Publication metadata and prose remain editable source files. Rust owns schema, ordering, routes, HTML, and the build ledger.",
                journal.len(),
            ))
            .child(journal_list)
            .into_view(),
    ));
    for entry in journal {
        let body = markdown_view::render(entry.markdown().ok_or_else(|| {
            format!("journal {} did not contain a Markdown document", entry.id())
        })?)?;
        pages.push(page(
            &format!("/journal/{}", entry.id()),
            &format!("{} / Journal", entry.data().title),
            &entry.data().description,
            "journal",
            components::journal_detail(entry, body),
        ));
    }

    pages.push(page(
        "/404.html",
        "Not found / Native Content Ledger",
        "The requested typed content record does not exist.",
        "none",
        components::not_found(),
    ));
    Ok(pages)
}

fn page(route: &str, title: &str, description: &str, active: &str, body: pliego_dom::View) -> Page {
    let canonical_route = if route == "/404.html" { "/" } else { route };
    let canonical = format!("{SITE_URL}{canonical_route}");
    let head = Head::new(title)
        .description(description)
        .canonical(canonical.clone())
        .icon("/favicon.svg")
        .manifest("/site.webmanifest")
        .apple_touch_icon("/app-icon.svg")
        .stylesheet("/assets/site.css")
        .meta("generator", "PliegoRS")
        .meta("theme-color", "#f0f0e8")
        .property_meta("og:type", "website")
        .property_meta("og:title", title)
        .property_meta("og:description", description)
        .property_meta("og:url", canonical)
        .property_meta("og:image", format!("{SITE_URL}/assets/reference/hero.jpg"));
    Page::new(route, head, components::layout(active, body)).language("en")
}
