#![no_main]

use libfuzzer_sys::fuzz_target;
use pliego_adapters::{AdapterCapability, AdapterIsland, MotionPolicy};
use pliego_dom::{RenderLimits, try_render_adoptable_html};

fn identifier(data: &[u8]) -> String {
    let tail = data
        .iter()
        .take(48)
        .map(|byte| match byte % 38 {
            0..=25 => char::from(b'a' + (byte % 26)),
            26..=35 => char::from(b'0' + (byte % 10)),
            36 => '-',
            _ => '_',
        })
        .collect::<String>();
    format!("a{tail}")
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 32 * 1024 {
        return;
    }
    let valid_shape = data.first().copied().unwrap_or_default() & 1 == 0;
    let id = if valid_shape {
        identifier(data)
    } else {
        String::from_utf8_lossy(data).into_owned()
    };
    let module = if valid_shape {
        format!(
            "/assets/{}.js",
            identifier(data.get(1..).unwrap_or_default())
        )
    } else {
        String::from_utf8_lossy(data.get(1..).unwrap_or_default()).into_owned()
    };
    let Ok(island) = AdapterIsland::new(id, module) else {
        return;
    };
    let key = identifier(data.get(2..).unwrap_or_default());
    let Ok(island) = island
        .capability(AdapterCapability::Dom)
        .motion_policy(MotionPolicy::SkipWhenReduced)
        .prop(key, String::from_utf8_lossy(data).into_owned())
    else {
        return;
    };
    let Ok(view) = island.into_view() else {
        return;
    };
    let limits = RenderLimits::new(16, 64, 64 * 1024).unwrap();
    let html = try_render_adoptable_html(&view, limits)
        .expect("admitted adapter manifest must render as an adoptable seed");
    assert!(html.contains("data-pliego-api=\"1\""));
    assert!(html.contains("data-pliego-motion=\"skip\""));
    assert!(html.len() <= limits.max_output_bytes());
});
