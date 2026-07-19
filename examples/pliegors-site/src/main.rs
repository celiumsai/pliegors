mod components;
mod content;
mod docs;
mod pages;
mod routes;
mod seo;
mod sitemap;

use pliego_adapters::wasm_bootstrap;
use pliego_ssg::{Asset, Site};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));
    let client_pkg = std::env::current_dir()?.join("target/pliegors-site-client/pkg");

    let content = content::SiteContent::load()?;
    let authored_pages = routes::pages(&content)?;
    let sitemap_routes = routes::sitemap_routes(&content);
    let route_count = authored_pages.len();
    let mut site = Site::new();
    for page in authored_pages {
        site = site.page(page);
    }

    site = add_tree(
        site,
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public"),
    )?;
    site = site
        .asset(Asset::new(
            "capabilities.json",
            include_bytes!("../../../product.capabilities.json").to_vec(),
        ))
        .asset(Asset::new(
            "assets/pliegors.css",
            include_bytes!("native.css").to_vec(),
        ))
        .asset(Asset::new("sitemap-index.xml", sitemap::index()))
        .asset(Asset::new(
            "sitemap-0.xml",
            sitemap::localized(&sitemap_routes)?,
        ))
        .asset(Asset::new(
            "assets/pliegors_site_client.js",
            std::fs::read(client_pkg.join("pliegors_site_client.js"))?,
        ))
        .asset(Asset::new(
            "assets/pliegors_site_client_bg.wasm",
            std::fs::read(client_pkg.join("pliegors_site_client_bg.wasm"))?,
        ))
        .asset(Asset::new(
            "assets/pliegors_site_boot.js",
            wasm_bootstrap("./pliegors_site_client.js")?,
        ));

    let report = site.build(&output)?;
    println!(
        "PliegoRS site: {} routes and {} files -> {}",
        route_count,
        report.receipt.outputs.files.len(),
        output.display()
    );
    Ok(())
}

fn add_tree(mut site: Site, root: &Path) -> Result<Site, Box<dyn std::error::Error>> {
    fn walk(
        site: &mut Option<Site>,
        root: &Path,
        current: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(site, root, &path)?;
            } else {
                let relative = path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = std::fs::read(&path)?;
                *site = Some(
                    site.take()
                        .expect("site")
                        .asset(Asset::new(relative, bytes)),
                );
            }
        }
        Ok(())
    }
    let mut owned = Some(site);
    walk(&mut owned, root, root)?;
    site = owned.expect("site");
    Ok(site)
}
