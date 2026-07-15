// SPDX-License-Identifier: Apache-2.0
#![cfg(target_arch = "wasm32")]
#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pliego_dom::{
    DomError, ElementNamespace, IntoView, MountError, MountStructureViolation, RenderError, View,
    dyn_text, dyn_view, el, mount, text, try_el,
};
use pliego_reactive::Signal;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function definePliegoDisconnectMover() {
  const name = "pliego-disconnect-mover";
  if (customElements.get(name)) return;
  customElements.define(name, class extends HTMLElement {
    disconnectedCallback() {
      const targetId = this.getAttribute("data-target");
      const external = targetId && document.getElementById(targetId);
      const candidate = document.querySelector('[data-role="poison-new"]');
      if (external && candidate) {
        external.setAttribute("data-candidate-moves", "1");
        external.appendChild(candidate);
      }
    }
  });
}

export function definePliegoDisconnectReinserter() {
  const name = "pliego-disconnect-reinserter";
  if (customElements.get(name)) return;
  customElements.define(name, class extends HTMLElement {
    disconnectedCallback() {
      if (this.__pliegoReinserted) return;
      this.__pliegoReinserted = true;

      const targetId = this.getAttribute("data-target");
      const external = targetId && document.getElementById(targetId);
      if (!external) return;

      const count = Number(external.getAttribute("data-reinsertions") || "0") + 1;
      external.setAttribute("data-reinsertions", String(count));
      external.appendChild(this);
    }
  });
}

export function definePliegoDisconnectClicker() {
  const name = "pliego-disconnect-clicker";
  if (customElements.get(name)) return;
  customElements.define(name, class extends HTMLElement {
    disconnectedCallback() {
      if (this.__pliegoClicked) return;
      this.__pliegoClicked = true;

      if (typeof this.__pliegoDisconnectCallback === "function") {
        this.__pliegoDisconnectCallback();
        return;
      }

      const targetId = this.getAttribute("data-target");
      const target = targetId && document.getElementById(targetId);
      if (target) target.click();
    }
  });
}

export function installPliegoDisconnectCallback(element, callback) {
  element.__pliegoDisconnectCallback = callback;
}

export function definePliegoPersistentDisconnectReinserter() {
  const name = "pliego-persistent-disconnect-reinserter";
  if (customElements.get(name)) return;
  customElements.define(name, class extends HTMLElement {
    disconnectedCallback() {
      const targetId = this.getAttribute("data-target");
      const external = targetId && document.getElementById(targetId);
      if (!external) return;

      const count = Number(external.getAttribute("data-reinsertions") || "0") + 1;
      external.setAttribute("data-reinsertions", String(count));
      external.appendChild(this);
    }
  });
}

export function definePliegoDisconnectBoundaryMover() {
  const name = "pliego-disconnect-boundary-mover";
  if (customElements.get(name)) return;
  customElements.define(name, class extends HTMLElement {
    disconnectedCallback() {
      if (this.__pliegoMovedBoundary) return;
      this.__pliegoMovedBoundary = true;

      const targetId = this.getAttribute("data-target");
      const external = targetId && document.getElementById(targetId);
      const candidate = document.querySelector('[data-role="boundary-new"]');
      const boundary = candidate && candidate.previousSibling;
      if (!external || !boundary || boundary.nodeType !== Node.COMMENT_NODE) return;

      external.setAttribute("data-boundary-moves", "1");
      external.appendChild(boundary);
    }
  });
}

export function definePliegoDisconnectOuterBoundaryMover() {
  const name = "pliego-disconnect-outer-boundary-mover";
  if (customElements.get(name)) return;
  customElements.define(name, class extends HTMLElement {
    disconnectedCallback() {
      if (this.__pliegoMovedOuterBoundary) return;
      this.__pliegoMovedOuterBoundary = true;

      const hostId = this.getAttribute("data-host");
      const targetId = this.getAttribute("data-target");
      const host = hostId && document.getElementById(hostId);
      const external = targetId && document.getElementById(targetId);
      const boundary = host && Array.from(host.childNodes).find(
        node => node.nodeType === Node.COMMENT_NODE && node.nodeValue === "pliego:dyn"
      );
      if (!external || !boundary) return;

      external.setAttribute("data-outer-boundary-moves", "1");
      external.appendChild(boundary);
    }
  });
}

let pliegoRemoveChildTrap;

export function installPliegoRemoveChildTrap(target, externalId, persistent) {
  if (pliegoRemoveChildTrap) throw new Error("Pliego removeChild trap is already installed");
  const original = Node.prototype.removeChild;
  let reinsertionCount = 0;
  const wrapped = function(child) {
    const removed = original.call(this, child);
    if (child === target && (persistent || reinsertionCount === 0)) {
      reinsertionCount += 1;
      const external = document.getElementById(externalId);
      if (external) {
        external.setAttribute("data-marker-reinsertions", String(reinsertionCount));
        external.appendChild(child);
      }
    }
    return removed;
  };
  Node.prototype.removeChild = wrapped;
  pliegoRemoveChildTrap = { original, wrapped };
}

export function restorePliegoRemoveChildTrap() {
  if (!pliegoRemoveChildTrap) return;
  if (Node.prototype.removeChild === pliegoRemoveChildTrap.wrapped) {
    Node.prototype.removeChild = pliegoRemoveChildTrap.original;
  }
  pliegoRemoveChildTrap = undefined;
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = definePliegoDisconnectMover)]
    fn define_disconnect_mover();

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = definePliegoDisconnectReinserter)]
    fn define_disconnect_reinserter();

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = definePliegoDisconnectClicker)]
    fn define_disconnect_clicker();

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = installPliegoDisconnectCallback)]
    fn install_disconnect_callback(element: &web_sys::Element, callback: &wasm_bindgen::JsValue);

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = definePliegoPersistentDisconnectReinserter)]
    fn define_persistent_disconnect_reinserter();

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = definePliegoDisconnectBoundaryMover)]
    fn define_disconnect_boundary_mover();

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = definePliegoDisconnectOuterBoundaryMover)]
    fn define_disconnect_outer_boundary_mover();

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = installPliegoRemoveChildTrap)]
    fn install_remove_child_trap(target: &web_sys::Node, external_id: &str, persistent: bool);

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = restorePliegoRemoveChildTrap)]
    fn restore_remove_child_trap();
}

fn has_ownership_mismatch(error: &MountError) -> bool {
    match error {
        MountError::DynamicUpdatePoisoned { cause } => has_ownership_mismatch(cause),
        MountError::Structure { violation, .. } => {
            *violation == MountStructureViolation::BoundaryOwnershipMismatch
        }
        _ => false,
    }
}

fn is_poisoned_by_ownership(error: &MountError) -> bool {
    matches!(
        error,
        MountError::DynamicUpdatePoisoned { cause } if has_ownership_mismatch(cause)
    )
}

fn is_recoverable_ownership(error: &MountError) -> bool {
    matches!(
        error,
        MountError::Structure {
            violation: MountStructureViolation::BoundaryOwnershipMismatch,
            ..
        }
    )
}

fn has_boundary_structure_error(error: &MountError) -> bool {
    match error {
        MountError::DynamicUpdatePoisoned { cause } => has_boundary_structure_error(cause),
        MountError::Structure { violation, .. } => matches!(
            violation,
            MountStructureViolation::BoundaryDetached
                | MountStructureViolation::BoundaryParentsDiffer
        ),
        _ => false,
    }
}

fn is_poisoned_by_boundary_structure(error: &MountError) -> bool {
    matches!(
        error,
        MountError::DynamicUpdatePoisoned { cause } if has_boundary_structure_error(cause)
    )
}

fn has_cleanup_non_convergence(error: &MountError) -> bool {
    match error {
        MountError::DynamicUpdatePoisoned { cause } => has_cleanup_non_convergence(cause),
        MountError::CleanupDidNotConverge { .. } => true,
        _ => false,
    }
}

fn document() -> web_sys::Document {
    web_sys::window()
        .expect("browser window")
        .document()
        .expect("browser document")
}

fn test_host() -> web_sys::Element {
    let document = document();
    let host = document.create_element("div").expect("create test host");
    document
        .body()
        .expect("document body")
        .append_child(&host)
        .expect("attach test host");
    host
}

fn remove_test_host(host: &web_sys::Element) {
    if let Some(parent) = host.parent_node() {
        parent.remove_child(host).expect("remove test host");
    }
}

fn query(host: &web_sys::Element, selector: &str) -> web_sys::Element {
    host.query_selector(selector)
        .expect("valid test selector")
        .unwrap_or_else(|| panic!("missing element for selector {selector:?}"))
}

fn count_direct_comment(host: &web_sys::Element, value: &str) -> usize {
    let mut count = 0;
    let mut cursor = host.first_child();
    while let Some(node) = cursor {
        if node.node_type() == web_sys::Node::COMMENT_NODE
            && node.node_value().as_deref() == Some(value)
        {
            count += 1;
        }
        cursor = node.next_sibling();
    }
    count
}

fn direct_comment(host: &web_sys::Element, value: &str) -> web_sys::Node {
    let mut cursor = host.first_child();
    while let Some(node) = cursor {
        if node.node_type() == web_sys::Node::COMMENT_NODE
            && node.node_value().as_deref() == Some(value)
        {
            return node;
        }
        cursor = node.next_sibling();
    }
    panic!("missing direct comment {value:?}")
}

#[wasm_bindgen_test]
fn mounted_root_disposal_is_idempotent_and_removes_its_entire_range() {
    let host = test_host();
    let root =
        mount(&el("p").child("mounted").into_view(), host.as_ref()).expect("mount static view");

    assert_eq!(host.text_content().as_deref(), Some("mounted"));
    assert!(!root.is_disposed());

    root.dispose();
    root.dispose();

    assert!(root.is_disposed());
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn disposal_preserves_foreign_dom_inserted_inside_mount_boundaries() {
    let host = test_host();
    let root = mount(
        &el("p")
            .attr("data-role", "pliego-owned")
            .child("owned")
            .into_view(),
        host.as_ref(),
    )
    .expect("mount owned view");
    let end_marker = host.last_child().expect("mount end marker");
    let foreign = document()
        .create_element("aside")
        .expect("create foreign node");
    foreign
        .set_attribute("data-role", "foreign")
        .expect("mark foreign node");
    foreign.set_text_content(Some("foreign"));
    host.insert_before(&foreign, Some(&end_marker))
        .expect("insert foreign node inside boundaries");

    root.dispose();

    assert!(
        host.query_selector("[data-role=pliego-owned]")
            .expect("valid selector")
            .is_none()
    );
    assert!(
        host.first_child()
            .expect("foreign first child")
            .is_same_node(Some(&foreign))
    );
    assert!(
        host.last_child()
            .expect("foreign last child")
            .is_same_node(Some(&foreign)),
        "Pliego markers or owned nodes survived disposal"
    );

    host.remove_child(&foreign).expect("remove foreign node");
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn listener_is_removed_with_the_exact_callback_on_dispose() {
    let host = test_host();
    let calls = Rc::new(Cell::new(0_u32));
    let observed = Rc::clone(&calls);
    let view = el("button")
        .attr("data-role", "trigger")
        .on("click", move |_| observed.set(observed.get() + 1))
        .child("Run")
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount listener view");
    let button = query(&host, "[data-role=trigger]");

    button
        .dispatch_event(&web_sys::Event::new("click").expect("create event"))
        .expect("dispatch mounted event");
    assert_eq!(calls.get(), 1);

    root.dispose();
    button
        .dispatch_event(&web_sys::Event::new("click").expect("create event"))
        .expect("dispatch detached event");
    assert_eq!(calls.get(), 1, "disposed listener still fired");
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn listener_can_dispose_its_own_root_without_reentry_or_residue() {
    let host = test_host();
    let calls = Rc::new(Cell::new(0_u32));
    let root_slot: Rc<RefCell<Option<pliego_dom::MountedRoot>>> = Rc::new(RefCell::new(None));
    let callback_calls = Rc::clone(&calls);
    let callback_root = Rc::clone(&root_slot);
    let view = el("button")
        .attr("data-role", "self-dispose")
        .on("click", move |_| {
            callback_calls.set(callback_calls.get() + 1);
            callback_root
                .borrow()
                .as_ref()
                .expect("mounted root is installed")
                .dispose();
        })
        .child("Dispose")
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount self-disposing listener");
    let button = query(&host, "[data-role=self-dispose]");
    *root_slot.borrow_mut() = Some(root);

    button
        .dispatch_event(&web_sys::Event::new("click").expect("create event"))
        .expect("dispatch self-disposing event");
    assert_eq!(calls.get(), 1);
    assert!(
        root_slot
            .borrow()
            .as_ref()
            .expect("root remains observable")
            .is_disposed()
    );
    assert!(!host.has_child_nodes());

    button
        .dispatch_event(&web_sys::Event::new("click").expect("create event"))
        .expect("dispatch detached event");
    assert_eq!(calls.get(), 1, "disposed callback re-entered");

    drop(root_slot.borrow_mut().take());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn dynamic_view_self_dispose_prevents_candidate_commit_and_future_runs() {
    let host = test_host();
    let dispose_now = Signal::new(false);
    let runs = Rc::new(Cell::new(0_u32));
    let root_slot: Rc<RefCell<Option<pliego_dom::MountedRoot>>> = Rc::new(RefCell::new(None));
    let callback_runs = Rc::clone(&runs);
    let callback_root = Rc::clone(&root_slot);
    let view = dyn_view(move || {
        callback_runs.set(callback_runs.get() + 1);
        if dispose_now.get() {
            callback_root
                .borrow()
                .as_ref()
                .expect("dynamic root installed")
                .dispose();
            el("span")
                .attr("data-role", "view-forbidden-commit")
                .child("forbidden")
                .into_view()
        } else {
            el("span")
                .attr("data-role", "view-stable")
                .child("stable")
                .into_view()
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount self-disposing DynView");
    *root_slot.borrow_mut() = Some(root);
    assert_eq!(runs.get(), 1);

    dispose_now.set(true);

    assert_eq!(runs.get(), 2);
    assert!(!host.has_child_nodes());
    assert!(
        host.query_selector("[data-role=view-forbidden-commit]")
            .expect("valid selector")
            .is_none()
    );
    dispose_now.set(false);
    assert_eq!(runs.get(), 2, "disposed DynView ran again");
    assert!(
        root_slot
            .borrow()
            .as_ref()
            .expect("root retained for assertions")
            .is_disposed()
    );

    drop(root_slot.borrow_mut().take());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn dynamic_text_self_dispose_prevents_write_and_future_runs() {
    let host = test_host();
    let version = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let root_slot: Rc<RefCell<Option<pliego_dom::MountedRoot>>> = Rc::new(RefCell::new(None));
    let callback_runs = Rc::clone(&runs);
    let callback_root = Rc::clone(&root_slot);
    let view = el("p")
        .attr("data-role", "text-owner")
        .child(dyn_text(move || {
            callback_runs.set(callback_runs.get() + 1);
            let current = version.get();
            if current == 1 {
                callback_root
                    .borrow()
                    .as_ref()
                    .expect("dynamic text root installed")
                    .dispose();
            }
            format!("text-{current}")
        }))
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount self-disposing DynText");
    let owner = query(&host, "[data-role=text-owner]");
    let text_node = owner.first_child().expect("dynamic text node");
    *root_slot.borrow_mut() = Some(root);
    assert_eq!(runs.get(), 1);

    version.set(1);

    assert_eq!(text_node.text_content().as_deref(), Some("text-0"));
    assert_eq!(runs.get(), 2);
    assert!(!host.has_child_nodes());
    version.set(2);
    assert_eq!(runs.get(), 2, "disposed DynText ran again");

    drop(root_slot.borrow_mut().take());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn dynamic_attribute_self_dispose_prevents_write_and_future_runs() {
    let host = test_host();
    let version = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let root_slot: Rc<RefCell<Option<pliego_dom::MountedRoot>>> = Rc::new(RefCell::new(None));
    let callback_runs = Rc::clone(&runs);
    let callback_root = Rc::clone(&root_slot);
    let view = el("div")
        .attr("data-role", "attr-owner")
        .attr_dyn("data-state", move || {
            callback_runs.set(callback_runs.get() + 1);
            let current = version.get();
            if current == 1 {
                callback_root
                    .borrow()
                    .as_ref()
                    .expect("dynamic attribute root installed")
                    .dispose();
            }
            format!("attr-{current}")
        })
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount self-disposing DynAttr");
    let element = query(&host, "[data-role=attr-owner]");
    *root_slot.borrow_mut() = Some(root);
    assert_eq!(runs.get(), 1);

    version.set(1);

    assert_eq!(
        element.get_attribute("data-state").as_deref(),
        Some("attr-0")
    );
    assert_eq!(runs.get(), 2);
    assert!(!host.has_child_nodes());
    version.set(2);
    assert_eq!(runs.get(), 2, "disposed DynAttr ran again");

    drop(root_slot.borrow_mut().take());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn dynamic_text_and_attribute_patch_in_place_and_reject_unsafe_updates() {
    let host = test_host();
    let label = Signal::new(String::from("first"));
    let href = Signal::new(String::from("/safe"));
    let view = el("a")
        .attr("data-role", "link")
        .attr_dyn("href", move || href.get())
        .child(dyn_text(move || label.get()))
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount reactive view");
    let anchor = query(&host, "[data-role=link]");
    let text_node = anchor.first_child().expect("dynamic text node");

    label.set(String::from("second"));
    href.set(String::from("/next"));
    let anchor_after_update = query(&host, "[data-role=link]");
    assert!(anchor_after_update.is_same_node(Some(&anchor)));
    assert!(
        anchor_after_update
            .first_child()
            .expect("updated text node")
            .is_same_node(Some(&text_node)),
        "dynamic text replaced its node"
    );
    assert_eq!(anchor.text_content().as_deref(), Some("second"));
    assert_eq!(anchor.get_attribute("href").as_deref(), Some("/next"));

    href.set(String::from("java\nscript:alert(1)"));
    assert_eq!(
        anchor.get_attribute("href").as_deref(),
        Some("/next"),
        "invalid dynamic value replaced the last stable attribute"
    );
    assert!(matches!(
        root.last_error(),
        Some(MountError::InvalidView(
            DomError::InvalidAttributeValue { .. }
        ))
    ));

    root.dispose();
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn dynamic_view_keeps_the_last_stable_range_when_staging_fails() {
    let host = test_host();
    let invalid = Signal::new(false);
    let view = dyn_view(move || {
        if invalid.get() {
            el("img")
                .child("void elements cannot have children")
                .into_view()
        } else {
            el("span")
                .attr("data-role", "stable")
                .child("stable")
                .into_view()
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount dynamic view");
    let stable = query(&host, "[data-role=stable]");

    invalid.set(true);

    let retained = query(&host, "[data-role=stable]");
    assert!(retained.is_same_node(Some(&stable)));
    assert_eq!(retained.text_content().as_deref(), Some("stable"));
    assert!(matches!(
        root.last_error(),
        Some(MountError::Structure {
            violation: MountStructureViolation::VoidElementHasChildren,
            ..
        })
    ));

    root.dispose();
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn nested_dynamic_view_commits_inner_then_outer_without_stale_topology() {
    let host = test_host();
    let inner_version = Signal::new(0_u32);
    let replace_outer = Signal::new(false);
    let view = dyn_view(move || {
        if replace_outer.get() {
            el("section")
                .attr("data-role", "outer-replacement")
                .child("replacement")
                .into_view()
        } else {
            View::Fragment(vec![
                el("div")
                    .attr("data-role", "outer-stable")
                    .child("outer")
                    .into_view(),
                dyn_view(move || {
                    el("span")
                        .attr("data-role", "inner-dynamic")
                        .child(format!("inner-{}", inner_version.get()))
                        .into_view()
                }),
            ])
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount nested dynamic views");
    let outer_before = query(&host, "[data-role=outer-stable]");
    let inner_before = query(&host, "[data-role=inner-dynamic]");

    inner_version.set(1);

    let outer_after_inner = query(&host, "[data-role=outer-stable]");
    let inner_after_inner = query(&host, "[data-role=inner-dynamic]");
    assert!(outer_after_inner.is_same_node(Some(&outer_before)));
    assert!(
        !inner_after_inner.is_same_node(Some(&inner_before)),
        "DynView update unexpectedly retained its replaced element"
    );
    assert!(inner_before.parent_node().is_none());
    assert_eq!(inner_after_inner.text_content().as_deref(), Some("inner-1"));
    assert_eq!(root.last_error(), None);

    replace_outer.set(true);

    let replacement = query(&host, "[data-role=outer-replacement]");
    assert_eq!(replacement.text_content().as_deref(), Some("replacement"));
    assert!(outer_before.parent_node().is_none());
    assert!(inner_after_inner.parent_node().is_none());
    assert_eq!(root.last_error(), None);

    root.dispose();
    assert_eq!(root.last_error(), None);
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn normal_dynamic_topology_updates_and_disposes_without_diagnostics() {
    let host = test_host();
    let generation = Signal::new(0_u32);
    let view = dyn_view(move || {
        el("output")
            .attr("data-role", "normal-dynamic")
            .child(format!("generation-{}", generation.get()))
            .into_view()
    });
    let root = mount(&view, host.as_ref()).expect("mount normal dynamic topology");

    generation.set(1);
    generation.set(2);

    assert_eq!(
        query(&host, "[data-role=normal-dynamic]")
            .text_content()
            .as_deref(),
        Some("generation-2")
    );
    assert_eq!(root.last_error(), None);
    root.dispose();
    assert_eq!(root.last_error(), None);
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn disconnected_callback_poison_is_terminal_and_cleanup_preserves_foreign_dom() {
    define_disconnect_mover();
    let host = test_host();
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-custom-external")
        .expect("identify custom element external host");
    let foreign = document()
        .create_element("aside")
        .expect("create preserved foreign node");
    foreign
        .set_attribute("data-role", "poison-foreign")
        .expect("mark preserved foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach preserved foreign node");

    let generation = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let callback_runs = Rc::clone(&runs);
    let view = dyn_view(move || {
        callback_runs.set(callback_runs.get() + 1);
        match generation.get() {
            0 => el("pliego-disconnect-mover")
                .attr("data-role", "poison-old")
                .attr("data-target", "pliego-custom-external")
                .into_view(),
            1 => el("article")
                .attr("data-role", "poison-new")
                .child("new")
                .into_view(),
            value => el("article")
                .attr("data-role", "poison-unexpected")
                .child(format!("unexpected-{value}"))
                .into_view(),
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount disconnect poison view");
    assert_eq!(runs.get(), 1);

    generation.set(1);

    assert_eq!(runs.get(), 2);
    assert_eq!(
        external_host
            .get_attribute("data-candidate-moves")
            .as_deref(),
        Some("1"),
        "disconnected callback did not move the selected candidate"
    );
    let error_after_update = root.last_error();
    assert!(
        error_after_update
            .as_ref()
            .is_some_and(is_poisoned_by_ownership),
        "after update: error={error_after_update:?}, host={:?}, external={:?}, runs={}",
        host.inner_html(),
        external_host.inner_html(),
        runs.get()
    );
    assert!(
        external_host
            .query_selector("[data-role=poison-new]")
            .expect("valid selector")
            .is_none(),
        "corrupted selected candidate survived terminal retirement"
    );
    assert!(
        host.query_selector("[data-role=poison-new]")
            .expect("valid selector")
            .is_none(),
        "corrupted selected candidate remained inside the dynamic slot"
    );
    assert!(
        external_host
            .first_child()
            .expect("foreign node survives candidate retirement")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("foreign node is sole survivor after candidate retirement")
                .is_same_node(Some(&foreign))
    );
    let stable_host = host.inner_html();
    let stable_external = external_host.inner_html();

    generation.set(2);

    let error_after_retry = root.last_error();
    assert_eq!(runs.get(), 2, "poisoned DynView evaluated again");
    assert_eq!(host.inner_html(), stable_host);
    assert_eq!(external_host.inner_html(), stable_external);
    assert!(
        host.query_selector("[data-role=poison-unexpected]")
            .expect("valid selector")
            .is_none()
    );
    assert!(
        error_after_retry
            .as_ref()
            .is_some_and(is_poisoned_by_ownership),
        "after retry: error={error_after_retry:?}, host={:?}, external={:?}, runs={}",
        host.inner_html(),
        external_host.inner_html(),
        runs.get()
    );

    root.dispose();

    let error_after_dispose = root.last_error();
    assert!(
        error_after_dispose
            .as_ref()
            .is_some_and(is_poisoned_by_ownership),
        "after dispose: error={error_after_dispose:?}, host={:?}, external={:?}, runs={}",
        host.inner_html(),
        external_host.inner_html(),
        runs.get()
    );
    assert!(!host.has_child_nodes());
    assert!(
        external_host
            .first_child()
            .expect("foreign node survives poisoned cleanup")
            .is_same_node(Some(&foreign))
    );
    assert!(
        external_host
            .last_child()
            .expect("foreign node is the only survivor")
            .is_same_node(Some(&foreign))
    );
    external_host
        .remove_child(&foreign)
        .expect("remove preserved foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn disconnected_callback_moving_candidate_boundary_is_terminal_and_cleanup_is_exact() {
    define_disconnect_boundary_mover();
    let host = test_host();
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-boundary-move-external")
        .expect("identify boundary external host");
    let foreign = document()
        .create_element("aside")
        .expect("create boundary foreign node");
    foreign
        .set_attribute("data-role", "boundary-foreign")
        .expect("mark boundary foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach boundary foreign node");
    let generation = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let callback_runs = Rc::clone(&runs);
    let view = dyn_view(move || {
        callback_runs.set(callback_runs.get() + 1);
        match generation.get() {
            0 => el("pliego-disconnect-boundary-mover")
                .attr("data-role", "boundary-old")
                .attr("data-target", "pliego-boundary-move-external")
                .into_view(),
            1 => el("article")
                .attr("data-role", "boundary-new")
                .child("new")
                .into_view(),
            value => el("article")
                .attr("data-role", "boundary-unexpected")
                .child(format!("unexpected-{value}"))
                .into_view(),
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount boundary mover view");

    generation.set(1);

    let error_after_update = root.last_error();
    assert_eq!(runs.get(), 2);
    assert_eq!(
        external_host
            .get_attribute("data-boundary-moves")
            .as_deref(),
        Some("1"),
        "disconnected callback did not move the candidate boundary"
    );
    assert!(
        error_after_update
            .as_ref()
            .is_some_and(is_poisoned_by_boundary_structure),
        "boundary update: error={error_after_update:?}, host={:?}, external={:?}",
        host.inner_html(),
        external_host.inner_html()
    );
    assert!(
        external_host
            .first_child()
            .expect("boundary foreign node survives rollback")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("moved boundary is cleaned")
                .is_same_node(Some(&foreign)),
        "moved boundary survived cleanup: external={:?}",
        external_host.inner_html()
    );
    let stable_host = host.inner_html();

    generation.set(2);

    assert_eq!(runs.get(), 2, "poisoned boundary slot evaluated again");
    assert_eq!(host.inner_html(), stable_host);
    assert!(
        host.query_selector("[data-role=boundary-unexpected]")
            .expect("valid selector")
            .is_none()
    );

    root.dispose();

    let error_after_dispose = root.last_error();
    assert!(
        error_after_dispose
            .as_ref()
            .is_some_and(is_poisoned_by_boundary_structure),
        "boundary dispose masked poison: error={error_after_dispose:?}, host={:?}, external={:?}",
        host.inner_html(),
        external_host.inner_html()
    );
    assert!(!host.has_child_nodes());
    assert!(
        external_host
            .first_child()
            .expect("boundary foreign survives root disposal")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("boundary foreign is sole survivor")
                .is_same_node(Some(&foreign))
    );
    external_host
        .remove_child(&foreign)
        .expect("remove boundary foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn disconnected_callback_moving_outer_dynamic_boundary_retires_the_entire_slot() {
    define_disconnect_outer_boundary_mover();
    let host = test_host();
    host.set_attribute("id", "pliego-outer-boundary-host")
        .expect("identify outer boundary host");
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-outer-boundary-external")
        .expect("identify outer boundary external host");
    let foreign = document()
        .create_element("aside")
        .expect("create outer boundary foreign node");
    foreign
        .set_attribute("data-role", "outer-boundary-foreign")
        .expect("mark outer boundary foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach outer boundary foreign node");
    let generation = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let callback_runs = Rc::clone(&runs);
    let view = dyn_view(move || {
        callback_runs.set(callback_runs.get() + 1);
        match generation.get() {
            0 => el("pliego-disconnect-outer-boundary-mover")
                .attr("data-role", "outer-boundary-old")
                .attr("data-host", "pliego-outer-boundary-host")
                .attr("data-target", "pliego-outer-boundary-external")
                .into_view(),
            1 => el("article")
                .attr("data-role", "outer-boundary-new")
                .child("new")
                .into_view(),
            value => el("article")
                .attr("data-role", "outer-boundary-unexpected")
                .child(format!("unexpected-{value}"))
                .into_view(),
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount outer boundary mover view");
    assert_eq!(count_direct_comment(&host, "pliego:dyn"), 1);
    assert_eq!(count_direct_comment(&host, "/pliego:dyn"), 1);

    generation.set(1);

    let error_after_update = root.last_error();
    assert_eq!(runs.get(), 2);
    assert_eq!(
        external_host
            .get_attribute("data-outer-boundary-moves")
            .as_deref(),
        Some("1"),
        "disconnected callback did not move the outer dynamic boundary"
    );
    assert!(
        error_after_update
            .as_ref()
            .is_some_and(is_poisoned_by_boundary_structure),
        "outer boundary update: error={error_after_update:?}, host={:?}, external={:?}",
        host.inner_html(),
        external_host.inner_html()
    );
    for selector in [
        "[data-role=outer-boundary-old]",
        "[data-role=outer-boundary-new]",
    ] {
        assert!(
            host.query_selector(selector)
                .expect("valid selector")
                .is_none()
                && external_host
                    .query_selector(selector)
                    .expect("valid selector")
                    .is_none(),
            "selected or retired content survived outer boundary corruption: selector={selector}, host={:?}, external={:?}",
            host.inner_html(),
            external_host.inner_html()
        );
    }
    assert_eq!(count_direct_comment(&host, "pliego:dyn"), 0);
    assert_eq!(count_direct_comment(&host, "/pliego:dyn"), 0);
    assert_eq!(count_direct_comment(&external_host, "pliego:dyn"), 0);
    assert_eq!(count_direct_comment(&external_host, "/pliego:dyn"), 0);
    assert!(
        external_host
            .first_child()
            .expect("outer boundary foreign survives immediate cleanup")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("outer boundary foreign is sole survivor")
                .is_same_node(Some(&foreign))
    );
    let stable_host = host.inner_html();
    let stable_external = external_host.inner_html();

    generation.set(2);

    assert_eq!(
        runs.get(),
        2,
        "poisoned outer boundary slot evaluated again"
    );
    assert_eq!(host.inner_html(), stable_host);
    assert_eq!(external_host.inner_html(), stable_external);
    assert!(
        host.query_selector("[data-role=outer-boundary-unexpected]")
            .expect("valid selector")
            .is_none()
    );

    root.dispose();

    let error_after_dispose = root.last_error();
    assert!(
        error_after_dispose
            .as_ref()
            .is_some_and(is_poisoned_by_boundary_structure),
        "outer boundary dispose masked poison: error={error_after_dispose:?}, host={:?}, external={:?}",
        host.inner_html(),
        external_host.inner_html()
    );
    assert!(!host.has_child_nodes());
    assert!(
        external_host
            .first_child()
            .expect("outer boundary foreign survives root disposal")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("outer boundary foreign remains sole survivor")
                .is_same_node(Some(&foreign))
    );
    external_host
        .remove_child(&foreign)
        .expect("remove outer boundary foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn poisoned_outer_marker_reinsertion_is_drained_before_registry_is_forgotten() {
    define_disconnect_outer_boundary_mover();
    let host = test_host();
    host.set_attribute("id", "pliego-marker-once-host")
        .expect("identify one-shot marker host");
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-marker-once-external")
        .expect("identify one-shot marker external host");
    let foreign = document()
        .create_element("aside")
        .expect("create one-shot marker foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach one-shot marker foreign node");
    let generation = Signal::new(0_u32);
    let view = dyn_view(move || match generation.get() {
        0 => el("pliego-disconnect-outer-boundary-mover")
            .attr("data-host", "pliego-marker-once-host")
            .attr("data-target", "pliego-marker-once-external")
            .into_view(),
        _ => el("article")
            .attr("data-role", "marker-once-candidate")
            .child("candidate")
            .into_view(),
    });
    let root = mount(&view, host.as_ref()).expect("mount one-shot marker trap view");
    let trapped_marker = direct_comment(&host, "pliego:dyn");
    install_remove_child_trap(&trapped_marker, "pliego-marker-once-external", false);

    generation.set(1);
    restore_remove_child_trap();

    let error = root.last_error();
    assert_eq!(
        external_host
            .get_attribute("data-marker-reinsertions")
            .as_deref(),
        Some("1")
    );
    assert!(
        error
            .as_ref()
            .is_some_and(is_poisoned_by_boundary_structure),
        "one-shot marker poison missing: {error:?}"
    );
    assert_eq!(count_direct_comment(&host, "pliego:dyn"), 0);
    assert_eq!(count_direct_comment(&host, "/pliego:dyn"), 0);
    assert_eq!(count_direct_comment(&external_host, "pliego:dyn"), 0);
    assert_eq!(count_direct_comment(&external_host, "/pliego:dyn"), 0);
    assert!(
        external_host
            .first_child()
            .expect("one-shot foreign survives")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("one-shot foreign is sole survivor")
                .is_same_node(Some(&foreign))
    );

    root.dispose();

    assert!(!host.has_child_nodes());
    assert!(
        external_host
            .first_child()
            .expect("one-shot foreign survives root dispose")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("one-shot foreign remains sole survivor")
                .is_same_node(Some(&foreign))
    );
    external_host
        .remove_child(&foreign)
        .expect("remove one-shot marker foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn persistent_outer_marker_reinsertion_is_bounded_and_retained_for_root_cleanup() {
    define_disconnect_outer_boundary_mover();
    let host = test_host();
    host.set_attribute("id", "pliego-marker-persistent-host")
        .expect("identify persistent marker host");
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-marker-persistent-external")
        .expect("identify persistent marker external host");
    let foreign = document()
        .create_element("aside")
        .expect("create persistent marker foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach persistent marker foreign node");
    let generation = Signal::new(0_u32);
    let view = dyn_view(move || match generation.get() {
        0 => el("pliego-disconnect-outer-boundary-mover")
            .attr("data-host", "pliego-marker-persistent-host")
            .attr("data-target", "pliego-marker-persistent-external")
            .into_view(),
        _ => el("article")
            .attr("data-role", "marker-persistent-candidate")
            .child("candidate")
            .into_view(),
    });
    let root = mount(&view, host.as_ref()).expect("mount persistent marker trap view");
    let trapped_marker = direct_comment(&host, "pliego:dyn");
    install_remove_child_trap(&trapped_marker, "pliego-marker-persistent-external", true);

    generation.set(1);
    restore_remove_child_trap();

    let poison = root.take_error().expect("outer marker poison");
    let non_convergence = root
        .take_error()
        .expect("marker non-convergence diagnostic");
    assert!(
        is_poisoned_by_boundary_structure(&poison),
        "persistent marker poison: {poison:?}"
    );
    assert!(matches!(
        non_convergence,
        MountError::CleanupDidNotConverge {
            remaining_owned_nodes: 1,
            passes: 64,
            ..
        }
    ));
    assert_eq!(
        external_host
            .get_attribute("data-marker-reinsertions")
            .as_deref(),
        Some("64")
    );
    assert!(
        trapped_marker.is_connected()
            && trapped_marker
                .parent_node()
                .is_some_and(|parent| parent.is_same_node(Some(external_host.as_ref())))
    );
    assert!(
        external_host
            .first_child()
            .expect("persistent marker foreign survives")
            .is_same_node(Some(&foreign))
    );

    root.dispose();

    assert!(!host.has_child_nodes());
    assert!(!trapped_marker.is_connected());
    assert!(
        external_host
            .first_child()
            .expect("persistent marker foreign survives retry")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("persistent marker foreign is sole survivor")
                .is_same_node(Some(&foreign))
    );
    external_host
        .remove_child(&foreign)
        .expect("remove persistent marker foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn nested_replacement_cleans_custom_element_reinserted_during_delegated_retire() {
    define_disconnect_reinserter();
    let host = test_host();
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-reinsert-replacement-external")
        .expect("identify replacement external host");
    let foreign = document()
        .create_element("aside")
        .expect("create replacement foreign node");
    foreign
        .set_attribute("data-role", "replacement-foreign")
        .expect("mark replacement foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach replacement foreign node");

    let generation = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let callback_runs = Rc::clone(&runs);
    let view = dyn_view(move || {
        let nested_runs = Rc::clone(&callback_runs);
        View::Fragment(vec![
            el("header")
                .attr("data-role", "nested-owner-stable")
                .child("stable")
                .into_view(),
            dyn_view(move || {
                nested_runs.set(nested_runs.get() + 1);
                match generation.get() {
                    0 => el("section")
                        .attr("data-role", "nested-retired-owner")
                        .child(
                            el("pliego-disconnect-reinserter")
                                .attr("data-role", "nested-reinserted-owned")
                                .attr("data-target", "pliego-reinsert-replacement-external"),
                        )
                        .into_view(),
                    value => el("article")
                        .attr("data-role", "nested-replacement")
                        .child(format!("replacement-{value}"))
                        .into_view(),
                }
            }),
        ])
    });
    let root = mount(&view, host.as_ref()).expect("mount nested reinsert replacement view");
    assert_eq!(runs.get(), 1);

    generation.set(1);

    let error = root.last_error();
    assert_eq!(
        external_host.get_attribute("data-reinsertions").as_deref(),
        Some("1"),
        "custom element callback did not run exactly once"
    );
    assert_eq!(runs.get(), 2);
    assert!(
        host.query_selector("[data-role=nested-reinserted-owned]")
            .expect("valid selector")
            .is_none()
    );
    assert!(
        external_host
            .query_selector("[data-role=nested-reinserted-owned]")
            .expect("valid selector")
            .is_none(),
        "delegated retirement left a reinserted owned node outside its range"
    );
    assert!(
        external_host
            .first_child()
            .expect("replacement foreign node survives retirement")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("replacement foreign node is sole survivor")
                .is_same_node(Some(&foreign))
    );
    assert!(
        error.as_ref().is_some_and(is_recoverable_ownership),
        "nested retire: error={error:?}, host={:?}, external={:?}, runs={}",
        host.inner_html(),
        external_host.inner_html(),
        runs.get()
    );

    generation.set(2);

    assert_eq!(
        runs.get(),
        3,
        "recoverable nested DynView did not run again"
    );
    assert_eq!(
        query(&host, "[data-role=nested-replacement]")
            .text_content()
            .as_deref(),
        Some("replacement-2")
    );
    assert_eq!(
        external_host.get_attribute("data-reinsertions").as_deref(),
        Some("1")
    );
    assert!(
        root.last_error()
            .as_ref()
            .is_some_and(is_recoverable_ownership)
    );

    root.dispose();

    assert!(!host.has_child_nodes());
    assert!(
        external_host
            .first_child()
            .expect("replacement foreign node survives root disposal")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("replacement foreign node remains sole survivor")
                .is_same_node(Some(&foreign))
    );
    external_host
        .remove_child(&foreign)
        .expect("remove replacement foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn root_dispose_cleans_custom_element_reinserted_during_delegated_cleanup() {
    define_disconnect_reinserter();
    let host = test_host();
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-reinsert-dispose-external")
        .expect("identify disposal external host");
    let foreign = document()
        .create_element("aside")
        .expect("create disposal foreign node");
    foreign
        .set_attribute("data-role", "dispose-reinsert-foreign")
        .expect("mark disposal foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach disposal foreign node");

    let view = el("section")
        .attr("data-role", "dispose-reinsert-owner")
        .child(
            el("pliego-disconnect-reinserter")
                .attr("data-role", "dispose-reinserted-owned")
                .attr("data-target", "pliego-reinsert-dispose-external"),
        )
        .child(
            el("span")
                .attr("data-role", "dispose-owned-sibling")
                .child("owned"),
        )
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount root disposal reinsert view");

    root.dispose();

    let error = root.take_error();
    assert!(root.is_disposed());
    assert_eq!(
        external_host.get_attribute("data-reinsertions").as_deref(),
        Some("1"),
        "custom element callback did not run exactly once"
    );
    assert!(
        !host.has_child_nodes()
            && external_host
                .query_selector("[data-role=dispose-reinserted-owned]")
                .expect("valid selector")
                .is_none(),
        "root cleanup left owned DOM behind: host={:?}, external={:?}",
        host.inner_html(),
        external_host.inner_html()
    );
    assert!(
        external_host
            .first_child()
            .expect("disposal foreign node survives cleanup")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("disposal foreign node is sole survivor")
                .is_same_node(Some(&foreign))
    );
    assert!(
        error.as_ref().is_some_and(is_recoverable_ownership),
        "root cleanup: error={error:?}, host={:?}, external={:?}",
        host.inner_html(),
        external_host.inner_html()
    );
    external_host
        .remove_child(&foreign)
        .expect("remove disposal foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn persistent_custom_element_reinsertion_terminates_with_explicit_survivor_diagnostic() {
    define_persistent_disconnect_reinserter();
    let host = test_host();
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-persistent-reinsert-external")
        .expect("identify persistent reinsert external host");
    let foreign = document()
        .create_element("aside")
        .expect("create persistent reinsert foreign node");
    foreign
        .set_attribute("data-role", "persistent-reinsert-foreign")
        .expect("mark persistent reinsert foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach persistent reinsert foreign node");
    let view = el("section")
        .attr("data-role", "persistent-reinsert-owner")
        .child(
            el("pliego-persistent-disconnect-reinserter")
                .attr("data-role", "persistent-reinsert-owned")
                .attr("data-target", "pliego-persistent-reinsert-external"),
        )
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount persistent reinsert view");
    let owned = query(&host, "[data-role=persistent-reinsert-owned]");

    root.dispose();

    let error = root.take_error();
    let reinsertion_count = external_host
        .get_attribute("data-reinsertions")
        .expect("persistent callback count")
        .parse::<usize>()
        .expect("numeric persistent callback count");
    assert!(root.is_disposed());
    assert!(!host.has_child_nodes());
    assert_eq!(reinsertion_count, 65, "unexpected bounded drain count");
    assert!(
        external_host
            .first_child()
            .expect("persistent foreign node survives cleanup")
            .is_same_node(Some(&foreign))
    );
    assert!(
        external_host
            .last_child()
            .expect("persistent owned survivor is retained")
            .is_same_node(Some(&owned))
    );
    assert!(matches!(
        error,
        Some(MountError::CleanupDidNotConverge {
            remaining_owned_nodes: 1,
            passes: 64,
            ..
        })
    ));
    assert_eq!(root.take_error(), None);

    owned
        .remove_attribute("data-target")
        .expect("disable persistent callback for test teardown");
    external_host
        .remove_child(&owned)
        .expect("remove persistent owned survivor");
    external_host
        .remove_child(&foreign)
        .expect("remove persistent foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn nested_terminal_cleanup_propagates_to_outer_origin_and_preserves_survivor_ownership() {
    define_persistent_disconnect_reinserter();
    let host = test_host();
    let external_host = test_host();
    external_host
        .set_attribute("id", "pliego-nested-terminal-external")
        .expect("identify nested terminal external host");
    let foreign = document()
        .create_element("aside")
        .expect("create nested terminal foreign node");
    external_host
        .append_child(&foreign)
        .expect("attach nested terminal foreign node");
    let generation = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let observed_runs = Rc::clone(&runs);
    let view = dyn_view(move || {
        observed_runs.set(observed_runs.get() + 1);
        match generation.get() {
            0 => el("section")
                .attr("data-role", "nested-terminal-old")
                .child(dyn_view(|| {
                    el("pliego-persistent-disconnect-reinserter")
                        .attr("data-role", "nested-terminal-owned")
                        .attr("data-target", "pliego-nested-terminal-external")
                        .into_view()
                }))
                .into_view(),
            value => el("article")
                .attr("data-role", "nested-terminal-candidate")
                .child(format!("candidate-{value}"))
                .into_view(),
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount nested terminal view");
    let owned = query(&host, "[data-role=nested-terminal-owned]");
    external_host
        .append_child(&owned)
        .expect("move nested owned node before outer retirement");
    let reinsertion_baseline = external_host
        .get_attribute("data-reinsertions")
        .unwrap_or_else(|| "0".to_string())
        .parse::<usize>()
        .expect("numeric nested reinsertion baseline");

    generation.set(1);

    let error = root.last_error();
    let reinsertion_after = external_host
        .get_attribute("data-reinsertions")
        .expect("nested terminal callback count")
        .parse::<usize>()
        .expect("numeric nested terminal callback count");
    assert_eq!(runs.get(), 2);
    assert!(
        error.as_ref().is_some_and(|error| matches!(
            error,
            MountError::DynamicUpdatePoisoned { cause }
                if has_cleanup_non_convergence(cause)
        )),
        "outer slot did not inherit nested terminal: {error:?}"
    );
    assert!(
        reinsertion_after >= reinsertion_baseline + 64,
        "nested cleanup did not exhaust its bounded drain"
    );
    assert_eq!(
        query(&host, "[data-role=nested-terminal-candidate]")
            .text_content()
            .as_deref(),
        Some("candidate-1")
    );
    assert!(
        owned.is_connected()
            && owned
                .parent_node()
                .is_some_and(|parent| parent.is_same_node(Some(external_host.as_ref()))),
        "nested survivor identity was lost"
    );

    generation.set(2);

    assert_eq!(
        runs.get(),
        2,
        "outer slot returned to Ready after terminal cleanup"
    );
    assert_eq!(
        query(&host, "[data-role=nested-terminal-candidate]")
            .text_content()
            .as_deref(),
        Some("candidate-1")
    );

    owned
        .remove_attribute("data-target")
        .expect("disable nested persistent callback for final cleanup");
    root.dispose();

    assert!(!host.has_child_nodes());
    assert!(
        !owned.is_connected(),
        "ancestor did not retain nested survivor ownership"
    );
    assert!(
        external_host
            .first_child()
            .expect("nested foreign survives root cleanup")
            .is_same_node(Some(&foreign))
            && external_host
                .last_child()
                .expect("nested foreign is sole survivor")
                .is_same_node(Some(&foreign))
    );
    let mut saw_outer_poison = false;
    let mut saw_non_convergence = false;
    while let Some(error) = root.take_error() {
        saw_outer_poison |= matches!(
            &error,
            MountError::DynamicUpdatePoisoned { cause }
                if has_cleanup_non_convergence(cause)
        );
        saw_non_convergence |= has_cleanup_non_convergence(&error);
    }
    assert!(saw_outer_poison && saw_non_convergence);
    external_host
        .remove_child(&foreign)
        .expect("remove nested terminal foreign node");
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn cross_slot_recoverable_error_does_not_poison_the_retiring_dynamic_slot() {
    define_disconnect_clicker();
    let host = test_host();
    let generation_a = Signal::new(0_u32);
    let invalidate_b = Signal::new(false);
    let runs_a = Rc::new(Cell::new(0_u32));
    let runs_b = Rc::new(Cell::new(0_u32));
    let disconnect_runs = Rc::new(Cell::new(0_u32));
    let observed_a = Rc::clone(&runs_a);
    let observed_b = Rc::clone(&runs_b);
    let callback_runs = Rc::clone(&disconnect_runs);
    let update_b = invalidate_b;
    let view = View::Fragment(vec![
        dyn_view(move || {
            observed_a.set(observed_a.get() + 1);
            match generation_a.get() {
                0 => el("section")
                    .attr("data-role", "cross-slot-a-retired")
                    .child(
                        el("pliego-disconnect-clicker")
                            .attr("id", "pliego-cross-slot-clicker")
                            .attr("data-role", "cross-slot-clicker")
                            .attr("data-target", "unused-direct-callback"),
                    )
                    .into_view(),
                value => el("article")
                    .attr("data-role", "cross-slot-a-replacement")
                    .child(format!("a-{value}"))
                    .into_view(),
            }
        }),
        dyn_view(move || {
            observed_b.set(observed_b.get() + 1);
            if invalidate_b.get() {
                el("img").child("invalid staged child").into_view()
            } else {
                el("span")
                    .attr("data-role", "cross-slot-b-stable")
                    .child("b-stable")
                    .into_view()
            }
        }),
    ]);
    let root = mount(&view, host.as_ref()).expect("mount cross-slot provenance view");
    let a_owner = query(&host, "[data-role=cross-slot-a-retired]");
    let clicker = query(&host, "[data-role=cross-slot-clicker]");
    let b_stable = query(&host, "[data-role=cross-slot-b-stable]");
    assert!(clicker.is_connected());
    assert!(
        document()
            .get_element_by_id("pliego-cross-slot-clicker")
            .is_some_and(|element| element.is_same_node(Some(&clicker))),
        "custom element id is not visible from document"
    );
    assert!(
        !a_owner.contains(Some(b_stable.as_ref())),
        "slot B unexpectedly lives inside the subtree retired by slot A"
    );
    let disconnect_callback = Closure::<dyn FnMut()>::new(move || {
        callback_runs.set(callback_runs.get() + 1);
        update_b.set(true);
    });
    install_disconnect_callback(&clicker, disconnect_callback.as_ref());

    generation_a.set(1);

    assert_eq!(
        disconnect_runs.get(),
        1,
        "disconnected callback did not invoke Rust"
    );
    assert_eq!(runs_a.get(), 2);
    assert_eq!(runs_b.get(), 2);
    assert_eq!(
        query(&host, "[data-role=cross-slot-a-replacement]")
            .text_content()
            .as_deref(),
        Some("a-1"),
        "slot A did not commit despite retaining valid topology"
    );
    assert!(b_stable.is_connected());
    let b_error = root.take_error().expect("slot B staging diagnostic");
    assert!(
        matches!(
            b_error,
            MountError::Structure {
                violation: MountStructureViolation::VoidElementHasChildren,
                ..
            }
        ),
        "cross-slot error provenance: error={b_error:?}, host={:?}",
        host.inner_html()
    );
    assert_eq!(
        root.take_error(),
        None,
        "slot A recorded a cross-slot poison"
    );

    generation_a.set(2);

    assert_eq!(
        runs_a.get(),
        3,
        "slot A was poisoned by slot B's diagnostic"
    );
    assert_eq!(
        query(&host, "[data-role=cross-slot-a-replacement]")
            .text_content()
            .as_deref(),
        Some("a-2")
    );
    assert_eq!(root.last_error(), None);

    root.dispose();

    assert!(!host.has_child_nodes());
    drop(disconnect_callback);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn foreign_node_in_dynamic_gap_blocks_commit_and_survives_cleanup() {
    let host = test_host();
    let replace = Signal::new(false);
    let view = dyn_view(move || {
        if replace.get() {
            el("strong")
                .attr("data-role", "gap-replacement")
                .child("replacement")
                .into_view()
        } else {
            el("span")
                .attr("data-role", "gap-stable")
                .child("stable")
                .into_view()
        }
    });
    let root = mount(&view, host.as_ref()).expect("mount dynamic gap view");
    let stable = query(&host, "[data-role=gap-stable]");
    let candidate_end = stable.next_sibling().expect("candidate end marker");
    let foreign = document()
        .create_element("aside")
        .expect("create foreign gap node");
    foreign
        .set_attribute("data-role", "gap-foreign")
        .expect("mark foreign gap node");
    candidate_end
        .parent_node()
        .expect("candidate parent")
        .insert_before(&foreign, Some(&candidate_end))
        .expect("insert foreign node into candidate gap");

    replace.set(true);

    assert!(query(&host, "[data-role=gap-stable]").is_same_node(Some(&stable)));
    assert!(
        host.query_selector("[data-role=gap-replacement]")
            .expect("valid selector")
            .is_none()
    );
    assert!(matches!(
        root.last_error(),
        Some(MountError::Structure {
            violation: MountStructureViolation::BoundaryOwnershipMismatch,
            ..
        })
    ));

    root.dispose();
    assert!(
        host.first_child()
            .expect("foreign node survives cleanup")
            .is_same_node(Some(&foreign))
    );
    assert!(
        host.last_child()
            .expect("foreign node is sole survivor")
            .is_same_node(Some(&foreign))
    );
    host.remove_child(&foreign)
        .expect("remove foreign gap node");
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn moved_owned_descendant_is_diagnosed_and_cleaned_on_candidate_retire() {
    let host = test_host();
    let external_host = test_host();
    let generation = Signal::new(0_u32);
    let runs = Rc::new(Cell::new(0_u32));
    let callback_runs = Rc::clone(&runs);
    let view = dyn_view(move || {
        callback_runs.set(callback_runs.get() + 1);
        let current = generation.get();
        el("div")
            .attr("data-role", format!("candidate-{current}"))
            .child(
                el("span")
                    .attr("data-role", format!("moved-{current}"))
                    .child(format!("owned-{current}")),
            )
            .into_view()
    });
    let root = mount(&view, host.as_ref()).expect("mount movable descendant view");
    let first_descendant = query(&host, "[data-role=moved-0]");
    external_host
        .append_child(&first_descendant)
        .expect("move first owned descendant to external host");

    generation.set(1);

    let external_clean = !external_host.has_child_nodes();
    let error = root.last_error();
    let ownership_error = error.as_ref().is_some_and(is_poisoned_by_ownership);
    assert!(
        external_clean && ownership_error,
        "retire contract: external_clean={external_clean}, error={error:?}, host={:?}, runs={}",
        host.inner_html(),
        runs.get()
    );
    let runs_after_poison = runs.get();
    let stable_host = host.inner_html();

    generation.set(2);

    assert_eq!(runs.get(), runs_after_poison);
    assert_eq!(host.inner_html(), stable_host);
    assert!(
        root.last_error()
            .as_ref()
            .is_some_and(is_poisoned_by_ownership)
    );

    root.dispose();
    assert!(!host.has_child_nodes());
    assert!(!external_host.has_child_nodes());
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn moved_owned_descendant_is_diagnosed_and_cleaned_on_root_dispose() {
    let host = test_host();
    let external_host = test_host();
    let view = dyn_view(|| {
        el("div")
            .attr("data-role", "dispose-candidate")
            .child(el("span").attr("data-role", "dispose-moved").child("owned"))
            .into_view()
    });
    let root = mount(&view, host.as_ref()).expect("mount disposable descendant view");
    let moved_descendant = query(&host, "[data-role=dispose-moved]");
    external_host
        .append_child(&moved_descendant)
        .expect("move owned descendant before root dispose");

    root.dispose();

    let host_clean = !host.has_child_nodes();
    let external_clean = !external_host.has_child_nodes();
    let error = root.take_error();
    let ownership_error = error.as_ref().is_some_and(has_ownership_mismatch);
    assert!(
        host_clean && external_clean && ownership_error,
        "root dispose contract: host_clean={host_clean}, external_clean={external_clean}, error={error:?}"
    );
    remove_test_host(&external_host);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn independent_dynamic_poison_errors_are_both_recoverable() {
    let host = test_host();
    let external_a = test_host();
    let external_b = test_host();
    let generation_a = Signal::new(0_u32);
    let generation_b = Signal::new(0_u32);
    let view = View::Fragment(vec![
        dyn_view(move || {
            el("div")
                .attr("data-role", format!("poison-a-{}", generation_a.get()))
                .child(el("span").attr("data-role", "poison-a-moved"))
                .into_view()
        }),
        dyn_view(move || {
            el("div")
                .attr("data-role", format!("poison-b-{}", generation_b.get()))
                .child(el("span").attr("data-role", "poison-b-moved"))
                .into_view()
        }),
    ]);
    let root = mount(&view, host.as_ref()).expect("mount independent dynamic slots");
    external_a
        .append_child(&query(&host, "[data-role=poison-a-moved]"))
        .expect("move slot A descendant");
    external_b
        .append_child(&query(&host, "[data-role=poison-b-moved]"))
        .expect("move slot B descendant");

    generation_a.set(1);
    generation_b.set(1);

    assert!(!external_a.has_child_nodes());
    assert!(!external_b.has_child_nodes());
    let first = root.take_error().expect("first terminal poison");
    let second = root.take_error().expect("second terminal poison");
    assert!(
        is_poisoned_by_ownership(&first) && is_poisoned_by_ownership(&second),
        "independent errors: first={first:?}, second={second:?}, host={:?}, external_a={:?}, external_b={:?}",
        host.inner_html(),
        external_a.inner_html(),
        external_b.inner_html()
    );
    assert_eq!(root.take_error(), None);

    root.dispose();
    assert!(!host.has_child_nodes());
    remove_test_host(&external_b);
    remove_test_host(&external_a);
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn invalid_initial_dynamic_view_returns_error_without_committing_boundaries() {
    let host = test_host();
    let view = dyn_view(|| {
        el("img")
            .child("void elements cannot have children")
            .into_view()
    });

    let result = mount(&view, host.as_ref());

    assert!(matches!(
        result,
        Err(MountError::Structure {
            violation: MountStructureViolation::VoidElementHasChildren,
            ..
        })
    ));
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn invalid_initial_dynamic_attribute_returns_error_without_committing_nodes() {
    let host = test_host();
    let unsafe_href = Signal::new(String::from("java\nscript:alert(1)"));
    let view = el("a")
        .attr_dyn("href", move || unsafe_href.get())
        .child("unsafe")
        .into_view();

    let result = mount(&view, host.as_ref());

    assert!(matches!(
        result,
        Err(MountError::InvalidView(
            DomError::InvalidAttributeValue { .. }
        ))
    ));
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn authored_html_tag_case_is_normalized_by_the_html_dom() {
    let host = test_host();
    let view = el("BR").attr("data-role", "break").into_view();
    let root = mount(&view, host.as_ref()).expect("mount uppercase HTML tag");
    let line_break = query(&host, "[data-role=break]");

    assert_eq!(line_break.local_name(), "br");
    assert_eq!(
        line_break.namespace_uri().as_deref(),
        Some(ElementNamespace::HTML_URI)
    );

    root.dispose();
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn svg_namespace_integration_points_and_qualified_attributes_are_exact() {
    let host = test_host();
    let view = el("svg")
        .attr("data-role", "svg")
        .attr("xmlns", ElementNamespace::SVG_URI)
        .attr("xmlns:xlink", "http://www.w3.org/1999/xlink")
        .child(
            el("g").attr("data-role", "group").child(
                el("use")
                    .attr("data-role", "use")
                    .attr("xlink:href", "#shape"),
            ),
        )
        .child(
            el("foreignObject")
                .attr("data-role", "foreign")
                .child(el("div").attr("data-role", "html-child")),
        )
        .into_view();
    let root = mount(&view, host.as_ref()).expect("mount SVG view");

    let svg = query(&host, "[data-role=svg]");
    let group = query(&host, "[data-role=group]");
    let use_node = query(&host, "[data-role=use]");
    let foreign = query(&host, "[data-role=foreign]");
    let html_child = query(&host, "[data-role=html-child]");
    for node in [&svg, &group, &use_node, &foreign] {
        assert_eq!(
            node.namespace_uri().as_deref(),
            Some(ElementNamespace::SVG_URI)
        );
    }
    assert_eq!(
        html_child.namespace_uri().as_deref(),
        Some(ElementNamespace::HTML_URI)
    );
    assert_eq!(
        use_node
            .get_attribute_ns(Some("http://www.w3.org/1999/xlink"), "href")
            .as_deref(),
        Some("#shape")
    );
    assert_eq!(
        svg.get_attribute_ns(Some("http://www.w3.org/2000/xmlns/"), "xmlns")
            .as_deref(),
        Some(ElementNamespace::SVG_URI)
    );
    assert_eq!(
        svg.get_attribute_ns(Some("http://www.w3.org/2000/xmlns/"), "xlink")
            .as_deref(),
        Some("http://www.w3.org/1999/xlink")
    );

    root.dispose();
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn existing_svg_html_integration_points_mount_html_children() {
    let document = document();
    let host = test_host();
    let svg = document
        .create_element_ns(Some(ElementNamespace::SVG_URI), "svg")
        .expect("create existing SVG host");
    host.append_child(&svg).expect("attach existing SVG host");

    for local_name in ["desc", "title", "foreignObject"] {
        let integration_point = document
            .create_element_ns(Some(ElementNamespace::SVG_URI), local_name)
            .expect("create integration point");
        svg.append_child(&integration_point)
            .expect("attach integration point");
        let view = el("span")
            .attr("data-mounted-under", local_name)
            .child(local_name)
            .into_view();
        let root = mount(&view, integration_point.as_ref()).expect("mount integration child");
        let mounted = query(
            &integration_point,
            &format!("[data-mounted-under={local_name}]"),
        );

        assert_eq!(
            mounted.namespace_uri().as_deref(),
            Some(ElementNamespace::HTML_URI),
            "wrong child namespace under existing {local_name}"
        );

        root.dispose();
        assert!(!integration_point.has_child_nodes());
        svg.remove_child(&integration_point)
            .expect("remove integration point");
    }

    host.remove_child(&svg).expect("remove existing SVG host");
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn existing_mathml_host_is_rejected_without_partial_dom() {
    const MATHML_URI: &str = "http://www.w3.org/1998/Math/MathML";

    let document = document();
    let math = document
        .create_element_ns(Some(MATHML_URI), "math")
        .expect("create MathML host");
    document
        .body()
        .expect("document body")
        .append_child(&math)
        .expect("attach MathML host");

    let result = mount(&el("span").child("unsupported").into_view(), math.as_ref());

    assert!(matches!(
        result,
        Err(MountError::UnsupportedNamespace { ref namespace })
            if namespace.preview == MATHML_URI
    ));
    assert!(!math.has_child_nodes());
    math.parent_node()
        .expect("MathML parent")
        .remove_child(&math)
        .expect("remove MathML host");
}

#[wasm_bindgen_test]
fn parser_repairing_transparent_ancestor_topologies_fail_transactionally() {
    for (outer, inner, expected_parent) in [
        ("p", "div", "p"),
        ("button", "button", "button"),
        ("a", "a", "a"),
        ("form", "form", "form"),
    ] {
        let host = test_host();
        let view = el(outer)
            .child(el("span").child(el(inner).child("invalid")))
            .into_view();

        let result = mount(&view, host.as_ref());

        assert!(
            matches!(
                result,
                Err(MountError::InvalidRender(RenderError::ParserRepair {
                    ref parent,
                    ..
                })) if parent == expected_parent
            ),
            "accepted parser-repaired {outer} > span > {inner}"
        );
        assert!(
            !host.has_child_nodes(),
            "partial DOM committed for {outer} > span > {inner}"
        );
        remove_test_host(&host);
    }
}

#[wasm_bindgen_test]
fn nested_ruby_text_segment_fails_transactionally() {
    let host = test_host();
    let view = el("ruby")
        .child(el("rt").child(el("rt").child("nested")))
        .into_view();

    let result = mount(&view, host.as_ref());

    assert!(matches!(
        result,
        Err(MountError::InvalidRender(RenderError::ParserRepair {
            ref parent,
            ..
        })) if parent == "ruby"
    ));
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn ruby_segments_with_transparent_descendants_preserve_authored_topology() {
    for (index, outer, inner) in [
        (0, "rb", "rt"),
        (1, "rt", "rt"),
        (2, "rt", "rb"),
        (3, "rtc", "rtc"),
    ] {
        let host = test_host();
        let segment_role = format!("ruby-segment-{index}");
        let bridge_role = format!("ruby-bridge-{index}");
        let descendant_role = format!("ruby-descendant-{index}");
        let view = el("ruby")
            .child(
                el(outer).attr("data-role", segment_role.clone()).child(
                    el("span").attr("data-role", bridge_role.clone()).child(
                        el(inner)
                            .attr("data-role", descendant_role.clone())
                            .child("preserved"),
                    ),
                ),
            )
            .into_view();
        let root = mount(&view, host.as_ref()).unwrap_or_else(|error| {
            panic!("rejected preserved ruby topology {outer} > span > {inner}: {error}")
        });
        let segment = query(&host, &format!("[data-role={segment_role}]"));
        let bridge = query(&host, &format!("[data-role={bridge_role}]"));
        let descendant = query(&host, &format!("[data-role={descendant_role}]"));

        assert!(
            bridge
                .parent_element()
                .is_some_and(|parent| parent.is_same_node(Some(&segment)))
        );
        assert!(
            descendant
                .parent_element()
                .is_some_and(|parent| parent.is_same_node(Some(&bridge)))
        );
        assert_eq!(descendant.text_content().as_deref(), Some("preserved"));
        assert_eq!(root.last_error(), None);

        root.dispose();
        assert_eq!(root.last_error(), None);
        assert!(!host.has_child_nodes());
        remove_test_host(&host);
    }
}

#[wasm_bindgen_test]
fn legacy_void_like_elements_are_rejected_by_the_builder() {
    for tag in ["basefont", "bgsound", "keygen"] {
        assert!(matches!(
            try_el(tag),
            Err(DomError::ForbiddenElement { tag: rejected })
                if rejected.eq_ignore_ascii_case(tag)
        ));
    }
}

#[wasm_bindgen_test]
fn existing_paragraph_host_rejects_block_child_without_partial_dom() {
    let document = document();
    let paragraph = document
        .create_element("p")
        .expect("create existing paragraph host");
    document
        .body()
        .expect("document body")
        .append_child(&paragraph)
        .expect("attach paragraph host");

    let result = mount(
        &el("div").attr("data-role", "invalid-block").into_view(),
        paragraph.as_ref(),
    );

    assert!(matches!(
        result,
        Err(MountError::InvalidRender(RenderError::ParserRepair {
            ref parent,
            ..
        })) if parent == "p"
    ));
    assert!(!paragraph.has_child_nodes());
    paragraph.remove();
}

#[wasm_bindgen_test]
fn paragraph_button_scope_allows_block_descendant() {
    let host = test_host();
    let view = el("p")
        .child(el("button").child(el("div").attr("data-role", "allowed-block").child("stable")))
        .into_view();

    let root = mount(&view, host.as_ref()).expect("mount scoped paragraph topology");

    assert_eq!(
        query(&host, "[data-role=allowed-block]")
            .text_content()
            .as_deref(),
        Some("stable")
    );
    assert_eq!(root.last_error(), None);
    root.dispose();
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn outer_form_survives_svg_scope_and_rejects_form_in_foreign_object() {
    let host = test_host();
    let view = el("form")
        .child(
            el("svg").child(el("foreignObject").child(el("form").attr("data-role", "nested-form"))),
        )
        .into_view();

    let result = mount(&view, host.as_ref());

    assert!(matches!(
        result,
        Err(MountError::InvalidRender(RenderError::ParserRepair {
            ref parent,
            ..
        })) if parent == "form"
    ));
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn invalid_initial_mount_is_transactional() {
    let host = test_host();
    let result = mount(
        &el("input").child("invalid child").into_view(),
        host.as_ref(),
    );

    assert!(matches!(
        result,
        Err(MountError::Structure {
            violation: MountStructureViolation::VoidElementHasChildren,
            ..
        })
    ));
    assert!(!host.has_child_nodes());
    remove_test_host(&host);
}

#[wasm_bindgen_test]
fn ten_thousand_mount_dispose_cycles_leave_no_dom_residue() {
    let host = test_host();
    let view: View = text("plateau");

    for cycle in 0..10_000 {
        let root = mount(&view, host.as_ref()).expect("mount plateau iteration");
        root.dispose();
        assert!(
            !host.has_child_nodes(),
            "DOM residue after mount/dispose cycle {cycle}"
        );
    }

    remove_test_host(&host);
}
