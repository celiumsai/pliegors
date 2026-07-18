#![no_main]

use libfuzzer_sys::fuzz_target;
use pliego_dom::{IntoView, RenderLimits, try_el, try_render_adoptable_html};

const TAGS: &[&str] = &[
    "div", "p", "span", "a", "img", "table", "tr", "td", "select", "option", "svg", "circle",
    "script",
];
const ATTRS: &[&str] = &[
    "class",
    "id",
    "href",
    "src",
    "title",
    "onclick",
    "data-fuzz",
];

fuzz_target!(|data: &[u8]| {
    if data.len() > 16 * 1024 {
        return;
    }
    let tag = TAGS[usize::from(data.first().copied().unwrap_or_default()) % TAGS.len()];
    let attribute = ATTRS[usize::from(data.get(1).copied().unwrap_or_default()) % ATTRS.len()];
    let value = String::from_utf8_lossy(data.get(2..).unwrap_or_default()).into_owned();
    let Ok(element) = try_el(tag) else {
        return;
    };
    let Ok(element) = element.try_attr(attribute, value.clone()) else {
        return;
    };
    let view = element.child(value).into_view();
    let limits = RenderLimits::new(16, 64, 8_192).unwrap();
    if let Ok(html) = try_render_adoptable_html(&view, limits) {
        assert!(html.starts_with("<!--pliego:ssr:v1-->"));
        assert!(html.ends_with("<!--/pliego:ssr:v1-->"));
        assert!(html.len() <= limits.max_output_bytes());
    }
});
