// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use pliego_dom::{IntoView, el};
use pliego_ssg::{Asset, Head, Page, Site};
use std::path::{Path, PathBuf};

const BOOTSTRAP: &[u8] = br#"import init from "./minimal_next_client.js";
await init();
"#;
const CSS: &[u8] = include_bytes!("../style.css");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/site"));
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let bindgen = fixture_root.join("target/minimal-next-client/pkg");

    let body =
        el("main")
            .child(el("p").class("eyebrow").child("PLIEGORS NEXT / PHASE 0"))
            .child(el("h1").child("Minimal causal UI"))
            .child(el("p").child(
                "Typed events, projection replay, owned effects, and explicit mount disposal.",
            ))
            .child(el("div").id("fixture-root").attr("aria-live", "polite"))
            .into_view();
    let head = Head::new("Minimal Next fixture")
        .description("Minimal executable PliegoRS causal browser fixture.")
        .stylesheet("/assets/style.css")
        .module_script("/assets/boot.js")
        .meta("generator", "PliegoRS");

    Site::new()
        .page(Page::new("/", head, body))
        .asset(Asset::new("assets/style.css", CSS.to_vec()).source("site/style.css"))
        .asset(Asset::new("assets/boot.js", BOOTSTRAP.to_vec()))
        .asset(Asset::new(
            "assets/minimal_next_client.js",
            std::fs::read(bindgen.join("minimal_next_client.js"))?,
        ))
        .asset(Asset::new(
            "assets/minimal_next_client_bg.wasm",
            std::fs::read(bindgen.join("minimal_next_client_bg.wasm"))?,
        ))
        .build(output)?;
    Ok(())
}
