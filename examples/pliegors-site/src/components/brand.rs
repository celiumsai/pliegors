use pliego_dom::{IntoView, View, el};

/// The open-fold mark used by PliegoRS.
pub fn brand_mark(size: u16, label: Option<&str>) -> View {
    let mut svg = el("svg")
        .class("brand-mark")
        .attr("width", size.to_string())
        .attr("height", size.to_string())
        .attr("viewBox", "0 0 24 24");
    if let Some(label) = label {
        svg = svg
            .attr("role", "img")
            .child(el("title").child(label.to_owned()));
    } else {
        svg = svg.attr("aria-hidden", "true");
    }
    svg.child(el("path").attr("d", "M10 3H21V10H14V14H3V21H14"))
        .into_view()
}
