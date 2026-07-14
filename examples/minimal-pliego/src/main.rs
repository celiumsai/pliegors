// SPDX-License-Identifier: Apache-2.0

use pliego_dom::{IntoView, el};
use pliego_ssg::{Asset, Head, Page, Site};
use std::path::PathBuf;

const CSS: &str = r#"
:root{font-family:system-ui,sans-serif;color:#171916;background:#f3f4ee}
*{box-sizing:border-box}body{margin:0}main{min-height:100svh;display:grid;place-content:center;padding:2rem}
h1{max-width:12ch;margin:0;font-size:clamp(3rem,10vw,8rem);line-height:.86;letter-spacing:0}
p{max-width:52ch;font-size:1.1rem}.mark{color:#c23a30;font-weight:800}
"#;

const ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="6" fill="#171916"/><path d="M28 8H56V28H40V36H8V56H40" fill="none" stroke="#f3f4ee" stroke-width="10"/><rect x="52" y="52" width="8" height="8" fill="#c23a30"/></svg>"##;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));
    let body =
        el("main")
            .child(
                el("section")
                    .child(el("p").class("mark").child("PLIEGORS / MINIMAL"))
                    .child(el("h1").child("Built from Rust."))
                    .child(el("p").child(
                        "Routes, documents, assets, and the build ledger belong to PliegoRS.",
                    )),
            )
            .into_view();
    let head = Head::new("Minimal PliegoRS")
        .description("A minimal PliegoRS project.")
        .icon("/favicon.svg")
        .stylesheet("/assets/site.css")
        .meta("generator", "PliegoRS");
    Site::new()
        .page(Page::new("/", head, body))
        .asset(Asset::new("assets/site.css", CSS.as_bytes().to_vec()))
        .asset(Asset::new("favicon.svg", ICON.as_bytes().to_vec()))
        .build(output)?;
    Ok(())
}
