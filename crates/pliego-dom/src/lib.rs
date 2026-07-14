// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! pliego-dom — PliegoRS's renderer (M4, docs/00 §2.3).
//!
//! The tachys lesson, our shape: **views are data**, and two walkers consume the
//! same tree:
//!
//! - [`render_html`] (native + wasm): walk the view once, evaluate dynamic parts
//!   untracked, and emit an HTML string. This is the **SSR seed** (M6) and what
//!   makes the renderer natively testable — no browser in the loop.
//! - `mount` (wasm only): build real DOM once; every dynamic part mounts its own
//!   [`pliego_reactive::Effect`] that patches **exactly its node** (a dynamic
//!   text patches one `Text` node; a dynamic attribute sets one attribute; a
//!   dynamic subtree rebuilds only between its two comment markers). Components
//!   never re-run; there is no virtual DOM.
//!
//! Deliberately NOT generic over a renderer trait — the Leptos study showed the
//! generic-renderer path exploding compile times; two concrete walkers over one
//! data tree give the same result without the cost.

use std::rc::Rc;

use pliego_reactive::untrack;

// On the browser target listeners receive the real event; natively (SSR/tests)
// they are inert and never called.
#[cfg(target_arch = "wasm32")]
pub type DomEvent = web_sys::Event;
#[cfg(not(target_arch = "wasm32"))]
pub type DomEvent = ();

type Listener = Rc<dyn Fn(DomEvent)>;

/// An attribute value: fixed, or a reactive closure (one effect per binding).
#[derive(Clone)]
pub enum AttrValue {
    Static(String),
    Dyn(Rc<dyn Fn() -> String>),
}

/// A view — the data both walkers consume.
#[derive(Clone)]
pub enum View {
    /// Static text.
    Text(String),
    /// Reactive text: mounts one effect patching one DOM `Text` node.
    DynText(Rc<dyn Fn() -> String>),
    /// An element with attributes, listeners and children.
    Element(Element),
    /// A sequence of sibling views.
    Fragment(Vec<View>),
    /// A reactive subtree: re-built (only between its markers) when its
    /// dependencies change. `<Show>`/`<For>` sugar composes on this.
    DynView(Rc<dyn Fn() -> View>),
}

/// An element under construction / in the tree.
#[derive(Clone)]
pub struct Element {
    tag: String,
    attrs: Vec<(String, AttrValue)>,
    listeners: Vec<(String, Listener)>,
    children: Vec<View>,
}

/// Start building an element: `el("div").class("x").child(...)`.
pub fn el(tag: impl Into<String>) -> Element {
    Element {
        tag: tag.into(),
        attrs: Vec::new(),
        listeners: Vec::new(),
        children: Vec::new(),
    }
}

/// A static text view.
pub fn text(s: impl Into<String>) -> View {
    View::Text(s.into())
}

/// A reactive text view — reads inside subscribe; the produced effect patches
/// exactly one DOM text node.
pub fn dyn_text(f: impl Fn() -> String + 'static) -> View {
    View::DynText(Rc::new(f))
}

/// A reactive subtree — rebuilt between its markers when dependencies change.
pub fn dyn_view(f: impl Fn() -> View + 'static) -> View {
    View::DynView(Rc::new(f))
}

/// Conditional sugar: `show(when, then, otherwise)`.
pub fn show(
    when: impl Fn() -> bool + 'static,
    then: impl Fn() -> View + 'static,
    otherwise: impl Fn() -> View + 'static,
) -> View {
    dyn_view(move || if when() { then() } else { otherwise() })
}

/// Anything that can become a `View` (what `.child()` accepts).
pub trait IntoView {
    fn into_view(self) -> View;
}
impl IntoView for View {
    fn into_view(self) -> View {
        self
    }
}
impl IntoView for Element {
    fn into_view(self) -> View {
        View::Element(self)
    }
}
impl IntoView for &str {
    fn into_view(self) -> View {
        View::Text(self.to_string())
    }
}
impl IntoView for String {
    fn into_view(self) -> View {
        View::Text(self)
    }
}
impl IntoView for Vec<View> {
    fn into_view(self) -> View {
        View::Fragment(self)
    }
}

impl Element {
    /// Set a static attribute.
    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs
            .push((name.into(), AttrValue::Static(value.into())));
        self
    }

    /// Bind a reactive attribute (one effect; sets exactly this attribute).
    #[must_use]
    pub fn attr_dyn(mut self, name: impl Into<String>, f: impl Fn() -> String + 'static) -> Self {
        self.attrs.push((name.into(), AttrValue::Dyn(Rc::new(f))));
        self
    }

    /// `class="…"` shorthand.
    #[must_use]
    pub fn class(self, value: impl Into<String>) -> Self {
        self.attr("class", value)
    }

    /// `id="…"` shorthand.
    #[must_use]
    pub fn id(self, value: impl Into<String>) -> Self {
        self.attr("id", value)
    }

    /// Append a child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.children.push(child.into_view());
        self
    }

    /// Attach an event listener (inert in SSR; real on the browser target).
    #[must_use]
    pub fn on(mut self, event: impl Into<String>, handler: impl Fn(DomEvent) + 'static) -> Self {
        self.listeners.push((event.into(), Rc::new(handler)));
        self
    }
}

// ───────────────────────── walker 1: HTML string (native + wasm; the SSR seed) ─────────────────────────

/// Escape text content.
fn esc_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape an attribute value (double-quoted).
fn esc_attr(s: &str) -> String {
    esc_text(s).replace('"', "&quot;")
}

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Render a view to an HTML string. Dynamic parts are evaluated **once,
/// untracked** — a deterministic fold of the current state into markup, which is
/// exactly why SSR output and a later client mount agree by construction
/// (docs/00 §2, hydration by determinism).
pub fn render_html(view: &View) -> String {
    let mut out = String::new();
    write_html(view, &mut out);
    out
}

fn write_html(view: &View, out: &mut String) {
    match view {
        View::Text(s) => out.push_str(&esc_text(s)),
        View::DynText(f) => out.push_str(&esc_text(&untrack(|| f()))),
        View::Fragment(children) => {
            for c in children {
                write_html(c, out);
            }
        }
        View::DynView(f) => {
            let inner = untrack(|| f());
            write_html(&inner, out);
        }
        View::Element(e) => {
            out.push('<');
            out.push_str(&e.tag);
            for (name, value) in &e.attrs {
                let v = match value {
                    AttrValue::Static(s) => s.clone(),
                    AttrValue::Dyn(f) => untrack(|| f()),
                };
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                out.push_str(&esc_attr(&v));
                out.push('"');
            }
            out.push('>');
            if VOID_TAGS.contains(&e.tag.as_str()) {
                return;
            }
            for c in &e.children {
                write_html(c, out);
            }
            out.push_str("</");
            out.push_str(&e.tag);
            out.push('>');
        }
    }
}

// ───────────────────────── walker 2: surgical DOM mount (wasm only) ─────────────────────────

#[cfg(target_arch = "wasm32")]
mod dom {
    use super::{AttrValue, View};
    use pliego_reactive::Effect;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    fn document() -> web_sys::Document {
        web_sys::window()
            .expect("window")
            .document()
            .expect("document")
    }

    /// Mount a view under `parent`. Builds real DOM once; every dynamic part
    /// installs an effect that patches exactly its node/attribute/segment.
    pub fn mount(view: &View, parent: &web_sys::Node) {
        match view {
            View::Text(s) => {
                let node = document().create_text_node(s);
                parent.append_child(&node).expect("append text");
            }
            View::DynText(f) => {
                let node = document().create_text_node("");
                parent.append_child(&node).expect("append dyn text");
                let f = f.clone();
                // surgical: this effect owns exactly this text node
                Effect::new(move || {
                    node.set_data(&f());
                });
            }
            View::Fragment(children) => {
                for c in children {
                    mount(c, parent);
                }
            }
            View::Element(e) => {
                let elem = document().create_element(&e.tag).expect("create element");
                for (name, value) in &e.attrs {
                    match value {
                        AttrValue::Static(s) => {
                            elem.set_attribute(name, s).expect("set attr");
                        }
                        AttrValue::Dyn(f) => {
                            let elem2 = elem.clone();
                            let (name, f) = (name.clone(), f.clone());
                            // surgical: this effect owns exactly this attribute
                            Effect::new(move || {
                                elem2.set_attribute(&name, &f()).expect("set dyn attr");
                            });
                        }
                    }
                }
                for (event, handler) in &e.listeners {
                    let handler = handler.clone();
                    let cb =
                        Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
                            handler(ev);
                        });
                    elem.add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())
                        .expect("listen");
                    cb.forget(); // listener lives as long as the element (M4 scope)
                }
                for c in &e.children {
                    mount(c, &elem);
                }
                parent.append_child(&elem).expect("append element");
            }
            View::DynView(f) => {
                // two comment markers bound the segment this effect owns
                let start = document().create_comment("pliego:dyn");
                let end = document().create_comment("/pliego:dyn");
                parent.append_child(&start).expect("append start marker");
                parent.append_child(&end).expect("append end marker");
                let f = f.clone();
                Effect::new(move || {
                    let fresh = f(); // tracked: reads inside subscribe
                    // clear everything between the markers
                    while let Some(n) = start.next_sibling() {
                        if n.is_same_node(Some(&end)) {
                            break;
                        }
                        if let Some(p) = n.parent_node() {
                            p.remove_child(&n).expect("remove stale");
                        }
                    }
                    // build the fresh segment into a detached container, then
                    // move it before the end marker
                    let staging = document().create_element("div").expect("staging");
                    mount(&fresh, &staging);
                    if let Some(p) = end.parent_node() {
                        while let Some(n) = staging.first_child() {
                            p.insert_before(&n, Some(&end)).expect("insert fresh");
                        }
                    }
                });
            }
        }
    }

    /// Mount into `<body>`.
    pub fn mount_to_body(view: &View) {
        let body = document().body().expect("body");
        mount(view, &body);
    }

    /// Mount into the element with `id`.
    pub fn mount_to(id: &str, view: &View) {
        let host = document().get_element_by_id(id).expect("mount host");
        mount(view, &host);
    }
}

#[cfg(target_arch = "wasm32")]
pub use dom::{mount, mount_to, mount_to_body};

// ───────────────────────── tests (the M4 gate, native) ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_reactive::Signal;

    /// THE M4 GATE 1: the builder renders correct HTML (the SSR seed).
    #[test]
    fn gate_builder_renders_html() {
        let v = el("section")
            .class("card")
            .id("hero")
            .child(el("h1").child("Pliego"))
            .child(el("p").child(text("folds & logs")))
            .child(el("br"))
            .into_view();
        assert_eq!(
            render_html(&v),
            r#"<section class="card" id="hero"><h1>Pliego</h1><p>folds &amp; logs</p><br></section>"#
        );
    }

    /// THE M4 GATE 2: dynamic parts fold CURRENT reactive state into markup —
    /// the same tree renders differently as the state advances (SSR of a live
    /// fold, deterministic).
    #[test]
    fn gate_dynamic_html_tracks_state() {
        let count = Signal::new(1);
        let v = el("div")
            .attr_dyn("data-count", move || count.get().to_string())
            .child(dyn_text(move || format!("count is {}", count.get())))
            .into_view();
        assert_eq!(render_html(&v), r#"<div data-count="1">count is 1</div>"#);
        count.set(7);
        assert_eq!(render_html(&v), r#"<div data-count="7">count is 7</div>"#);
    }

    /// THE M4 GATE 3: `show` composes over the same reactive state.
    #[test]
    fn gate_show_branches() {
        let logged_in = Signal::new(false);
        let v = show(
            move || logged_in.get(),
            || el("main").child("welcome back").into_view(),
            || el("a").attr("href", "/login").child("log in").into_view(),
        );
        assert_eq!(render_html(&v), r#"<a href="/login">log in</a>"#);
        logged_in.set(true);
        assert_eq!(render_html(&v), "<main>welcome back</main>");
    }

    /// Escaping: content and attributes are safe by default.
    #[test]
    fn escaping_is_on_by_default() {
        let v = el("p")
            .attr("title", r#"a "quote" & <tag>"#)
            .child(text("<script>alert(1)</script>"))
            .into_view();
        assert_eq!(
            render_html(&v),
            r#"<p title="a &quot;quote&quot; &amp; &lt;tag&gt;">&lt;script&gt;alert(1)&lt;/script&gt;</p>"#
        );
    }

    /// SSR renders untracked: rendering must not subscribe (no stray edges).
    #[test]
    fn render_html_does_not_subscribe() {
        use pliego_reactive::{Effect, Memo};
        use std::cell::Cell;
        use std::rc::Rc;

        let s = Signal::new(0);
        let v = el("span")
            .child(dyn_text(move || s.get().to_string()))
            .into_view();

        // render inside a memo: if render_html tracked, the memo would depend on s
        let renders = Rc::new(Cell::new(0));
        let m = {
            let renders = renders.clone();
            let v = v.clone();
            Memo::new(move || {
                renders.set(renders.get() + 1);
                render_html(&v)
            })
        };
        Effect::new(move || {
            let _ = m.get();
        });
        assert_eq!(renders.get(), 1);
        s.set(99);
        assert_eq!(
            renders.get(),
            1,
            "render_html must not create reactive edges"
        );
    }
}
