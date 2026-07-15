// SPDX-License-Identifier: Apache-2.0

mod components;
mod content;
mod markdown_view;
mod routes;

use content::Catalog;
use pliego_ssg::{Asset, Site};
use std::path::PathBuf;

const MANIFEST: &str = r##"{
  "name": "Native Content Ledger",
  "short_name": "Content Ledger",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#f0f0e8",
  "theme_color": "#f0f0e8",
  "icons": [{"src":"/app-icon.svg","sizes":"any","type":"image/svg+xml"}]
}
"##;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog = Catalog::load(&workspace.join("fixtures/content/reference"))?;
    let pages = routes::pages(&catalog).map_err(std::io::Error::other)?;
    let sitemap = sitemap(&pages);
    let route_count = pages.len();

    let mut site = Site::new();
    for page in pages {
        site = site.page(page);
    }
    site = site
        .asset(Asset::new(
            "assets/site.css",
            include_bytes!("../assets/site.css").to_vec(),
        ))
        .asset(Asset::new(
            "favicon.svg",
            std::fs::read(workspace.join("brand/pliegors-symbol.svg"))?,
        ))
        .asset(Asset::new(
            "app-icon.svg",
            std::fs::read(workspace.join("brand/pliegors-app-icon.svg"))?,
        ))
        .asset(Asset::new(
            "assets/reference/hero.jpg",
            std::fs::read(
                workspace.join("crates/pliego-starters/templates/editorial/assets/images/hero.jpg"),
            )?,
        ))
        .asset(Asset::new("site.webmanifest", MANIFEST.as_bytes().to_vec()))
        .asset(Asset::new(
            "robots.txt",
            b"User-agent: *\nAllow: /\nSitemap: https://content-ledger.pliegors.dev/sitemap.xml\n"
                .to_vec(),
        ))
        .asset(Asset::new("sitemap.xml", sitemap.into_bytes()));
    let report = site.build(&output)?;
    println!(
        "Native Content Ledger: {} typed entries / {route_count} routes / {} files -> {}",
        catalog.total_entries(),
        report.receipt.outputs.files.len(),
        output.display()
    );
    Ok(())
}

fn sitemap(pages: &[pliego_ssg::Page]) -> String {
    let mut output = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for page in pages.iter().filter(|page| page.route != "/404.html") {
        output.push_str("  <url><loc>https://content-ledger.pliegors.dev");
        output.push_str(&page.route);
        output.push_str("</loc></url>\n");
    }
    output.push_str("</urlset>\n");
    output
}
