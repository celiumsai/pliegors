// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_arch = "wasm32", test))]
fn normalize_search(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(any(target_arch = "wasm32", test))]
fn next_index(current: usize, count: usize, delta: i32) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some((current as i32 + delta).rem_euclid(count as i32) as usize)
}

#[cfg(any(target_arch = "wasm32", test))]
fn stage_index_after_key(current: usize, count: usize, key: &str) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match key {
        "ArrowRight" | "ArrowDown" => Some((current + 1) % count),
        "ArrowLeft" | "ArrowUp" => Some((current + count - 1) % count),
        "Home" => Some(0),
        "End" => Some(count - 1),
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::{next_index, normalize_search, stage_index_after_key};
    use js_sys::Array;
    use pliego_dom::MountScope;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{
        Document, Element, Event, HtmlElement, HtmlInputElement, IntersectionObserver,
        IntersectionObserverEntry, IntersectionObserverInit, KeyboardEvent, Node,
    };

    const THEME_KEY: &str = "pliegors:theme:v1";

    thread_local! {
        static CLIENT_SCOPE: RefCell<Option<MountScope>> = const { RefCell::new(None) };
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        let installed = CLIENT_SCOPE.with(|slot| {
            if slot.borrow().is_some() {
                return false;
            }
            *slot.borrow_mut() = Some(MountScope::new());
            true
        });
        if !installed {
            return;
        }
        init_theme();
        init_menu();
        init_progress();
        init_reveals();
        init_carousels();
        init_engine_labs();
        init_pipelines();
        init_docs_search();
        init_doc_copy();
    }

    fn window() -> web_sys::Window {
        web_sys::window().expect("window")
    }

    fn document() -> Document {
        window().document().expect("document")
    }

    fn root() -> Element {
        document().document_element().expect("document element")
    }

    fn elements(scope: &Element, selector: &str) -> Vec<Element> {
        let Ok(nodes) = scope.query_selector_all(selector) else {
            return Vec::new();
        };
        (0..nodes.length())
            .filter_map(|index| nodes.item(index))
            .filter_map(|node| node.dyn_into::<Element>().ok())
            .collect()
    }

    fn document_elements(selector: &str) -> Vec<Element> {
        elements(&root(), selector)
    }

    fn listen(target: &web_sys::EventTarget, event: &str, handler: impl FnMut(Event) + 'static) {
        let closure = Closure::<dyn FnMut(Event)>::new(handler);
        target
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .expect("event listener");
        let target = target.clone();
        let event = event.to_owned();
        own_cleanup(move || {
            target
                .remove_event_listener_with_callback(&event, closure.as_ref().unchecked_ref())
                .ok();
        });
    }

    fn own_cleanup(cleanup: impl FnOnce() + 'static) {
        CLIENT_SCOPE.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("site client lifecycle scope")
                .on_cleanup(cleanup)
                .expect("register site client cleanup");
        });
    }

    fn prefers_dark() -> bool {
        window()
            .match_media("(prefers-color-scheme: dark)")
            .ok()
            .flatten()
            .is_some_and(|query| query.matches())
    }

    fn prefers_reduced_motion() -> bool {
        window()
            .match_media("(prefers-reduced-motion: reduce)")
            .ok()
            .flatten()
            .is_some_and(|query| query.matches())
    }

    fn apply_theme(value: &str) {
        let theme = match value {
            "light" | "dark" => value,
            _ => "system",
        };
        let resolved = if theme == "system" {
            if prefers_dark() { "dark" } else { "light" }
        } else {
            theme
        };
        root().set_attribute("data-theme", theme).ok();
        root().set_attribute("data-resolved-theme", resolved).ok();
        for button in document_elements("[data-theme-choice]") {
            let active = button.get_attribute("data-theme-choice").as_deref() == Some(theme);
            button
                .set_attribute("aria-pressed", if active { "true" } else { "false" })
                .ok();
        }
    }

    fn init_theme() {
        let stored = window()
            .local_storage()
            .ok()
            .flatten()
            .and_then(|storage| storage.get_item(THEME_KEY).ok().flatten())
            .unwrap_or_else(|| "system".into());
        apply_theme(&stored);
        for button in document_elements("[data-theme-choice]") {
            let value = button
                .get_attribute("data-theme-choice")
                .unwrap_or_else(|| "system".into());
            listen(button.unchecked_ref(), "click", move |_| {
                if let Some(storage) = window().local_storage().ok().flatten() {
                    storage.set_item(THEME_KEY, &value).ok();
                }
                apply_theme(&value);
            });
        }
    }

    fn init_menu() {
        let Some(toggle) = document()
            .query_selector("[data-menu-toggle]")
            .ok()
            .flatten()
        else {
            return;
        };
        let Some(menu) = document()
            .query_selector("[data-mobile-menu]")
            .ok()
            .flatten()
        else {
            return;
        };
        let open = Rc::new(Cell::new(false));
        let background = Rc::new(document_elements("main, .site-footer"));
        let set_open: Rc<dyn Fn(bool)> = {
            let toggle = toggle.clone();
            let menu = menu.clone();
            let open = open.clone();
            let background = background.clone();
            Rc::new(move |value| {
                open.set(value);
                menu.class_list().toggle_with_force("is-open", value).ok();
                menu.set_attribute("aria-hidden", if value { "false" } else { "true" })
                    .ok();
                toggle
                    .set_attribute("aria-expanded", if value { "true" } else { "false" })
                    .ok();
                let label = toggle
                    .get_attribute(if value {
                        "data-menu-close"
                    } else {
                        "data-menu-open"
                    })
                    .unwrap_or_default();
                if let Some(node) = toggle.query_selector("[data-menu-label]").ok().flatten() {
                    node.set_text_content(Some(&label));
                }
                root()
                    .class_list()
                    .toggle_with_force("menu-is-open", value)
                    .ok();
                for element in background.iter() {
                    if value {
                        element.set_attribute("inert", "").ok();
                        element.set_attribute("aria-hidden", "true").ok();
                    } else {
                        element.remove_attribute("inert").ok();
                        element.remove_attribute("aria-hidden").ok();
                    }
                }
                let focus_target = if value {
                    menu.query_selector(
                        "a[href], button:not([disabled]), [tabindex]:not([tabindex='-1'])",
                    )
                    .ok()
                    .flatten()
                } else {
                    Some(toggle.clone())
                };
                if let Some(target) =
                    focus_target.and_then(|target| target.dyn_into::<HtmlElement>().ok())
                {
                    target.focus().ok();
                }
            })
        };
        {
            let set_open = set_open.clone();
            let open = open.clone();
            listen(toggle.unchecked_ref(), "click", move |_| {
                set_open(!open.get())
            });
        }
        for link in elements(&menu, "[data-menu-link]") {
            let set_open = set_open.clone();
            listen(link.unchecked_ref(), "click", move |_| set_open(false));
        }
        {
            let set_open = set_open.clone();
            let open = open.clone();
            let menu = menu.clone();
            listen(document().unchecked_ref(), "keydown", move |event| {
                let Some(event) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                if !open.get() {
                    return;
                }
                if event.key() == "Escape" {
                    event.prevent_default();
                    set_open(false);
                    return;
                }
                if event.key() != "Tab" {
                    return;
                }
                let focusable = elements(
                    &menu,
                    "a[href], button:not([disabled]), [tabindex]:not([tabindex='-1'])",
                );
                let (Some(first), Some(last)) = (focusable.first(), focusable.last()) else {
                    event.prevent_default();
                    return;
                };
                let active = document().active_element();
                let at_first = active
                    .as_ref()
                    .is_some_and(|element| element.is_same_node(Some(first)));
                let at_last = active
                    .as_ref()
                    .is_some_and(|element| element.is_same_node(Some(last)));
                let target = if event.shift_key() && at_first {
                    Some(last)
                } else if !event.shift_key() && at_last {
                    Some(first)
                } else {
                    None
                };
                if let Some(target) = target.and_then(|target| target.dyn_ref::<HtmlElement>()) {
                    event.prevent_default();
                    target.focus().ok();
                }
            });
        }
    }

    fn init_progress() {
        let Some(bar) = document()
            .query_selector("[data-page-progress]")
            .ok()
            .flatten()
            .and_then(|element| element.dyn_into::<HtmlElement>().ok())
        else {
            return;
        };
        let update = Rc::new(move || {
            let viewport = window()
                .inner_height()
                .ok()
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let total = root().scroll_height() as f64 - viewport;
            let progress = if total > 0.0 {
                (window().scroll_y().unwrap_or(0.0) / total).clamp(0.0, 1.0)
            } else {
                0.0
            };
            bar.style()
                .set_property("transform", &format!("scaleX({progress})"))
                .ok();
        });
        update();
        let update_scroll = update.clone();
        listen(window().unchecked_ref(), "scroll", move |_| update_scroll());
        listen(window().unchecked_ref(), "resize", move |_| update());
    }

    fn init_reveals() {
        let targets = document_elements("[data-reveal]");
        if targets.is_empty() {
            return;
        }
        if prefers_reduced_motion() {
            for target in targets {
                target.class_list().add_1("is-visible").ok();
            }
            return;
        }
        let callback = Closure::<dyn FnMut(Array, IntersectionObserver)>::new(
            move |entries: Array, observer: IntersectionObserver| {
                for value in entries.iter() {
                    let entry: IntersectionObserverEntry = value.unchecked_into();
                    if !entry.is_intersecting() {
                        continue;
                    }
                    let target: Element = entry.target().unchecked_into();
                    target.class_list().add_1("is-visible").ok();
                    target.class_list().remove_1("is-reveal-pending").ok();
                    observer.unobserve(&target);
                }
            },
        );
        let options = IntersectionObserverInit::new();
        options.set_root_margin("0px 0px -8% 0px");
        options.set_threshold(&JsValue::from_f64(0.08));
        if let Ok(observer) =
            IntersectionObserver::new_with_options(callback.as_ref().unchecked_ref(), &options)
        {
            for target in &targets {
                target.class_list().add_1("is-reveal-pending").ok();
                observer.observe(target);
            }
            let observed = targets.clone();
            if let Ok(Some(query)) = window().match_media("(prefers-reduced-motion: reduce)") {
                let observer_for_motion = observer.clone();
                let query_state = query.clone();
                listen(query.unchecked_ref(), "change", move |_| {
                    if query_state.matches() {
                        observer_for_motion.disconnect();
                        for target in &observed {
                            target.class_list().add_1("is-visible").ok();
                            target.class_list().remove_1("is-reveal-pending").ok();
                        }
                    }
                });
            }
            own_cleanup(move || {
                observer.disconnect();
                drop(callback);
            });
        }
    }

    fn init_carousels() {
        for carousel in document_elements("[data-hero-carousel]") {
            let slides = Rc::new(elements(&carousel, "[data-hero-slide]"));
            if slides.len() < 2 {
                continue;
            }
            let index = Rc::new(Cell::new(0usize));
            let user_paused = Rc::new(Cell::new(false));
            let motion_paused = Rc::new(Cell::new(prefers_reduced_motion()));
            let hover_paused = Rc::new(Cell::new(false));
            let focus_paused = Rc::new(Cell::new(false));
            carousel
                .class_list()
                .toggle_with_force("is-paused", user_paused.get() || motion_paused.get())
                .ok();
            let show: Rc<dyn Fn(i32)> = {
                let carousel = carousel.clone();
                let slides = slides.clone();
                let index = index.clone();
                Rc::new(move |delta| {
                    let Some(next) = next_index(index.get(), slides.len(), delta) else {
                        return;
                    };
                    if let Some(image) = slides[next].query_selector("img[data-src]").ok().flatten()
                    {
                        if let Some(source) = image.get_attribute("data-src") {
                            image.set_attribute("src", &source).ok();
                            image.remove_attribute("data-src").ok();
                        }
                    }
                    for (position, slide) in slides.iter().enumerate() {
                        let active = position == next;
                        slide
                            .class_list()
                            .toggle_with_force("is-active", active)
                            .ok();
                        slide
                            .set_attribute("aria-hidden", if active { "false" } else { "true" })
                            .ok();
                    }
                    if let Some(current) = carousel
                        .query_selector("[data-hero-current]")
                        .ok()
                        .flatten()
                    {
                        current.set_text_content(Some(&format!("{:02}", next + 1)));
                    }
                    if let Some(label) = carousel
                        .query_selector("[data-hero-current-label]")
                        .ok()
                        .flatten()
                    {
                        label.set_text_content(
                            slides[next].get_attribute("data-hero-label").as_deref(),
                        );
                    }
                    index.set(next);
                })
            };
            for (selector, delta) in [("[data-hero-previous]", -1), ("[data-hero-next]", 1)] {
                if let Some(button) = carousel.query_selector(selector).ok().flatten() {
                    let show = show.clone();
                    listen(button.unchecked_ref(), "click", move |_| show(delta));
                }
            }
            if let Some(button) = carousel.query_selector("[data-hero-pause]").ok().flatten() {
                let user_paused = user_paused.clone();
                let motion_paused = motion_paused.clone();
                let carousel = carousel.clone();
                let control = button.clone();
                control
                    .set_attribute(
                        "aria-pressed",
                        if user_paused.get() { "true" } else { "false" },
                    )
                    .ok();
                let initial_label = control
                    .get_attribute(if user_paused.get() {
                        "data-hero-resume-label"
                    } else {
                        "data-hero-pause-label"
                    })
                    .unwrap_or_default();
                control.set_attribute("aria-label", &initial_label).ok();
                listen(button.unchecked_ref(), "click", move |_| {
                    user_paused.set(!user_paused.get());
                    carousel
                        .class_list()
                        .toggle_with_force("is-paused", user_paused.get() || motion_paused.get())
                        .ok();
                    control
                        .set_attribute(
                            "aria-pressed",
                            if user_paused.get() { "true" } else { "false" },
                        )
                        .ok();
                    let label = control
                        .get_attribute(if user_paused.get() {
                            "data-hero-resume-label"
                        } else {
                            "data-hero-pause-label"
                        })
                        .unwrap_or_default();
                    control.set_attribute("aria-label", &label).ok();
                });
            }
            let interval = carousel
                .get_attribute("data-hero-interval")
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(6200);
            let show_interval = show.clone();
            let user_paused_interval = user_paused.clone();
            let motion_paused_interval = motion_paused.clone();
            let hover_paused_interval = hover_paused.clone();
            let focus_paused_interval = focus_paused.clone();
            let closure = Closure::<dyn FnMut()>::new(move || {
                if !user_paused_interval.get()
                    && !motion_paused_interval.get()
                    && !hover_paused_interval.get()
                    && !focus_paused_interval.get()
                    && !document_hidden()
                {
                    show_interval(1);
                }
            });
            if let Ok(interval_handle) = window()
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    interval,
                )
            {
                own_cleanup(move || {
                    window().clear_interval_with_handle(interval_handle);
                    drop(closure);
                });
            }

            if let Ok(Some(query)) = window().match_media("(prefers-reduced-motion: reduce)") {
                let carousel = carousel.clone();
                let motion_paused = motion_paused.clone();
                let user_paused = user_paused.clone();
                let query_state = query.clone();
                listen(query.unchecked_ref(), "change", move |_| {
                    motion_paused.set(query_state.matches());
                    carousel
                        .class_list()
                        .toggle_with_force("is-paused", user_paused.get() || motion_paused.get())
                        .ok();
                });
            }

            let enter_paused = hover_paused.clone();
            listen(carousel.unchecked_ref(), "pointerenter", move |_| {
                enter_paused.set(true)
            });
            let leave_paused = hover_paused.clone();
            listen(carousel.unchecked_ref(), "pointerleave", move |_| {
                leave_paused.set(false);
            });
            let focus_carousel = carousel.clone();
            let focus_paused_out = focus_paused.clone();
            listen(carousel.unchecked_ref(), "focusout", move |event| {
                let remains_inside =
                    js_sys::Reflect::get(event.as_ref(), &JsValue::from_str("relatedTarget"))
                        .ok()
                        .and_then(|target| target.dyn_into::<Node>().ok())
                        .is_some_and(|target| focus_carousel.contains(Some(&target)));
                focus_paused_out.set(remains_inside);
            });
            let focus_paused_in = focus_paused.clone();
            listen(carousel.unchecked_ref(), "focusin", move |_| {
                focus_paused_in.set(true)
            });
        }
    }

    fn document_hidden() -> bool {
        js_sys::Reflect::get(document().as_ref(), &JsValue::from_str("hidden"))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn init_engine_labs() {
        for lab in document_elements("[data-engine-lab]") {
            let buttons = Rc::new(elements(&lab, "[data-engine-stage]"));
            let panels = Rc::new(elements(&lab, "[data-engine-panel]"));
            if buttons.is_empty() || panels.is_empty() {
                continue;
            }
            let activate: Rc<dyn Fn(usize, bool)> = {
                let buttons = buttons.clone();
                let panels = panels.clone();
                Rc::new(move |requested, move_focus| {
                    let index = requested.min(buttons.len() - 1);
                    let stage = buttons[index]
                        .get_attribute("data-engine-stage")
                        .unwrap_or_default();
                    for (position, button) in buttons.iter().enumerate() {
                        let active = position == index;
                        button
                            .class_list()
                            .toggle_with_force("is-active", active)
                            .ok();
                        button
                            .set_attribute("aria-selected", if active { "true" } else { "false" })
                            .ok();
                        button
                            .set_attribute("tabindex", if active { "0" } else { "-1" })
                            .ok();
                    }
                    for panel in panels.iter() {
                        let active = panel.get_attribute("data-engine-panel").as_deref()
                            == Some(stage.as_str());
                        panel
                            .class_list()
                            .toggle_with_force("is-active", active)
                            .ok();
                        if active {
                            panel.remove_attribute("hidden").ok();
                        } else {
                            panel.set_attribute("hidden", "").ok();
                        }
                    }
                    if move_focus {
                        buttons[index]
                            .dyn_ref::<HtmlElement>()
                            .and_then(|button| button.focus().ok());
                    }
                })
            };
            for (index, button) in buttons.iter().enumerate() {
                let activate_click = activate.clone();
                listen(button.unchecked_ref(), "click", move |_| {
                    activate_click(index, false)
                });

                let activate_key = activate.clone();
                let count = buttons.len();
                listen(button.unchecked_ref(), "keydown", move |event| {
                    let Some(event) = event.dyn_ref::<KeyboardEvent>() else {
                        return;
                    };
                    let Some(next) = stage_index_after_key(index, count, &event.key()) else {
                        return;
                    };
                    event.prevent_default();
                    activate_key(next, true);
                });
            }
        }
    }

    fn init_pipelines() {
        for pipeline in document_elements("[data-pipeline]") {
            let steps = Rc::new(elements(&pipeline, "[data-pipeline-step]"));
            if steps.is_empty() {
                continue;
            }
            let progress = pipeline
                .query_selector("[data-pipeline-progress]")
                .ok()
                .flatten()
                .and_then(|element| element.dyn_into::<HtmlElement>().ok());
            let glyph = pipeline
                .query_selector(".rs-pipeline__glyph")
                .ok()
                .flatten()
                .and_then(|element| element.dyn_into::<HtmlElement>().ok());
            let set_active: Rc<dyn Fn(usize)> = {
                let steps = steps.clone();
                let progress = progress.clone();
                let glyph = glyph.clone();
                Rc::new(move |requested| {
                    let index = requested.min(steps.len() - 1);
                    for (position, step) in steps.iter().enumerate() {
                        step.class_list()
                            .toggle_with_force("is-active", position == index)
                            .ok();
                    }
                    if let Some(progress) = &progress {
                        progress
                            .style()
                            .set_property(
                                "transform",
                                &format!("scaleY({})", (index + 1) as f64 / steps.len() as f64),
                            )
                            .ok();
                    }
                    if let Some(glyph) = &glyph {
                        glyph
                            .style()
                            .set_property("transform", &format!("rotate({}deg)", index * 90))
                            .ok();
                    }
                })
            };
            set_active(0);
            if prefers_reduced_motion() {
                continue;
            }
            let callback = {
                let set_active = set_active.clone();
                Closure::<dyn FnMut(Array, IntersectionObserver)>::new(
                    move |entries: Array, _observer: IntersectionObserver| {
                        for value in entries.iter() {
                            let entry: IntersectionObserverEntry = value.unchecked_into();
                            if !entry.is_intersecting() {
                                continue;
                            }
                            let target: Element = entry.target().unchecked_into();
                            let index = target
                                .get_attribute("data-pipeline-index")
                                .and_then(|value| value.parse::<usize>().ok())
                                .unwrap_or(0);
                            set_active(index);
                        }
                    },
                )
            };
            let options = IntersectionObserverInit::new();
            options.set_root_margin("-24% 0px -42% 0px");
            options.set_threshold(&JsValue::from_f64(0.35));
            if let Ok(observer) =
                IntersectionObserver::new_with_options(callback.as_ref().unchecked_ref(), &options)
            {
                for step in steps.iter() {
                    observer.observe(step);
                }
                if let Ok(Some(query)) = window().match_media("(prefers-reduced-motion: reduce)") {
                    let observer_for_motion = observer.clone();
                    let query_state = query.clone();
                    listen(query.unchecked_ref(), "change", move |_| {
                        if query_state.matches() {
                            observer_for_motion.disconnect();
                        }
                    });
                }
                own_cleanup(move || {
                    observer.disconnect();
                    drop(callback);
                });
            }
        }
    }

    fn init_docs_search() {
        for page in document_elements("[data-docs-page]") {
            let Some(input) = page
                .query_selector("[data-docs-search]")
                .ok()
                .flatten()
                .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
            else {
                continue;
            };
            let render: Rc<dyn Fn()> = {
                let page = page.clone();
                let input = input.clone();
                Rc::new(move || {
                    let query = normalize_search(&input.value());
                    for item in elements(&page, "[data-docs-item]") {
                        let haystack = item.get_attribute("data-search").unwrap_or_default();
                        item.dyn_ref::<HtmlElement>()
                            .expect("docs item")
                            .set_hidden(!query.is_empty() && !haystack.contains(&query));
                    }
                })
            };
            let render_input = render.clone();
            listen(input.unchecked_ref(), "input", move |_| render_input());
            if let Some(clear) = page.query_selector("[data-docs-clear]").ok().flatten() {
                let render = render.clone();
                let input = input.clone();
                listen(clear.unchecked_ref(), "click", move |_| {
                    input.set_value("");
                    input.focus().ok();
                    render();
                });
            }
        }
    }

    fn init_doc_copy() {
        for button in document_elements("[data-doc-copy]") {
            let target_id = button.get_attribute("data-copy-target").unwrap_or_default();
            let copied_label = button
                .get_attribute("data-copied-label")
                .unwrap_or_else(|| "Copied".into());
            let failed_label = button
                .get_attribute("data-copy-failed-label")
                .unwrap_or_else(|| "Copy failed".into());
            let copy_button = button.clone();
            listen(button.unchecked_ref(), "click", move |_| {
                let Some(code) = document().get_element_by_id(&target_id) else {
                    return;
                };
                let text = code.text_content().unwrap_or_default();
                let promise = window().navigator().clipboard().write_text(&text);
                let button = copy_button.clone();
                let copied_label = copied_label.clone();
                let failed_label = failed_label.clone();
                spawn_local(async move {
                    let label = if JsFuture::from(promise).await.is_ok() {
                        copied_label
                    } else {
                        failed_label
                    };
                    button.set_text_content(Some(&label));
                    button.set_attribute("aria-label", &label).ok();
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_normalization_is_stable() {
        assert_eq!(normalize_search("  Rust   WASM "), "rust wasm");
    }

    #[test]
    fn carousel_index_wraps_in_both_directions() {
        assert_eq!(next_index(0, 3, -1), Some(2));
        assert_eq!(next_index(2, 3, 1), Some(0));
        assert_eq!(next_index(0, 0, 1), None);
    }

    #[test]
    fn engine_tabs_follow_keyboard_conventions() {
        assert_eq!(stage_index_after_key(0, 3, "ArrowLeft"), Some(2));
        assert_eq!(stage_index_after_key(2, 3, "ArrowRight"), Some(0));
        assert_eq!(stage_index_after_key(1, 3, "Home"), Some(0));
        assert_eq!(stage_index_after_key(1, 3, "End"), Some(2));
        assert_eq!(stage_index_after_key(1, 3, "Escape"), None);
    }
}
