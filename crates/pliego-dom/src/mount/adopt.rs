// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

use super::*;

const ROOT_START: &str = "pliego:ssr:v1";
const ROOT_END: &str = "/pliego:ssr:v1";
const TEXT_START: &str = "pliego:text";
const TEXT_END: &str = "/pliego:text";
const DYN_TEXT_START: &str = "pliego:dyn-text";
const DYN_TEXT_END: &str = "/pliego:dyn-text";
const DYN_START: &str = "pliego:dyn";
const DYN_END: &str = "/pliego:dyn";
const MOUNT_START: &str = "pliego:mount";
const MOUNT_END: &str = "/pliego:mount";
const KEYED_START: &str = "pliego:keyed";
const KEYED_END: &str = "/pliego:keyed";
const ROW_START: &str = "pliego:row";
const ROW_END: &str = "/pliego:row";

struct DynamicAttributePlan {
    name: super::super::AttributeName,
    source: Rc<dyn Fn() -> Result<String, DomError>>,
    initial: String,
    namespace: ElementNamespace,
}

enum AdoptionPlan {
    Text {
        start: web_sys::Comment,
        end: web_sys::Comment,
        node: Option<web_sys::Text>,
        value: String,
        dynamic: Option<Rc<dyn Fn() -> String>>,
        parent_context: Option<MountParentContext>,
        guaranteed_prefix_content: bool,
        path: String,
    },
    Element {
        node: web_sys::Element,
        authored: Element,
        dynamic_attributes: Vec<DynamicAttributePlan>,
        children: Vec<AdoptionPlan>,
        rcdata_text: Option<web_sys::Text>,
        path: String,
    },
    Fragment(Vec<AdoptionPlan>),
    Dynamic {
        start: web_sys::Comment,
        end: web_sys::Comment,
        child_start: web_sys::Comment,
        child_end: web_sys::Comment,
        source: Rc<dyn Fn() -> View>,
        child: Box<AdoptionPlan>,
        namespace: ElementNamespace,
        parent_context: Option<MountParentContext>,
        guaranteed_prefix_content: bool,
        path: String,
    },
    Keyed {
        start: web_sys::Comment,
        end: web_sys::Comment,
        spec: Rc<KeyedSpec>,
        rows: Vec<AdoptedRowPlan>,
        namespace: ElementNamespace,
        parent_context: Option<MountParentContext>,
        guaranteed_prefix_content: bool,
        path: String,
    },
}

struct AdoptedRowPlan {
    key: KeyedKey,
    start: web_sys::Comment,
    end: web_sys::Comment,
    child: AdoptionPlan,
}

struct Preflight {
    limits: RenderLimits,
    visited: usize,
}

impl Preflight {
    fn new(limits: RenderLimits) -> Self {
        Self { limits, visited: 0 }
    }

    fn visit(&mut self, depth: usize) -> Result<(), MountError> {
        if depth > self.limits.max_depth() {
            return Err(RenderError::LimitExceeded {
                kind: super::super::RenderLimitKind::Depth,
                limit: self.limits.max_depth(),
            }
            .into());
        }
        self.visited = self
            .visited
            .checked_add(1)
            .ok_or(RenderError::LimitExceeded {
                kind: super::super::RenderLimitKind::Nodes,
                limit: self.limits.max_nodes(),
            })?;
        if self.visited > self.limits.max_nodes() {
            return Err(RenderError::LimitExceeded {
                kind: super::super::RenderLimitKind::Nodes,
                limit: self.limits.max_nodes(),
            }
            .into());
        }
        Ok(())
    }
}

fn mismatch(path: &str, expected: &str, actual: impl AsRef<str>) -> MountError {
    MountError::AdoptionMismatch {
        path: MountDiagnostic::new(path),
        expected: MountDiagnostic::new(expected),
        actual: MountDiagnostic::new(actual.as_ref()),
    }
}

fn describe_node(node: Option<&web_sys::Node>) -> String {
    let Some(node) = node else {
        return "<missing>".to_owned();
    };
    if let Some(comment) = node.dyn_ref::<web_sys::Comment>() {
        let length = comment.length();
        let preview = comment
            .substring_data(0, length.min(128))
            .unwrap_or_else(|_| "<unavailable>".to_owned());
        return format!("comment {preview:?} ({length} UTF-16 units)");
    }
    if let Some(text) = node.dyn_ref::<web_sys::Text>() {
        let length = text.length();
        let preview = text
            .substring_data(0, length.min(128))
            .unwrap_or_else(|_| "<unavailable>".to_owned());
        return format!("text {preview:?} ({length} UTF-16 units)");
    }
    if let Some(element) = node.dyn_ref::<web_sys::Element>() {
        return format!(
            "element <{}> in {:?}",
            element.local_name(),
            element.namespace_uri()
        );
    }
    format!("DOM node type {}", node.node_type())
}

fn expect_comment(
    cursor: Option<web_sys::Node>,
    marker: &str,
    path: &str,
    _preflight: &mut Preflight,
) -> Result<(web_sys::Comment, Option<web_sys::Node>), MountError> {
    let Some(node) = cursor else {
        return Err(mismatch(path, &format!("comment {marker:?}"), "<missing>"));
    };
    let actual = describe_node(Some(&node));
    let comment = node
        .dyn_into::<web_sys::Comment>()
        .map_err(|_| mismatch(path, &format!("comment {marker:?}"), actual))?;
    if comment.length() as usize != marker.encode_utf16().count() || comment.data() != marker {
        return Err(mismatch(
            path,
            &format!("comment {marker:?}"),
            describe_node(Some(comment.as_ref())),
        ));
    }
    let next = comment.next_sibling();
    Ok((comment, next))
}

fn expect_text(
    cursor: Option<web_sys::Node>,
    expected: &str,
    path: &str,
    _preflight: &mut Preflight,
) -> Result<(web_sys::Text, Option<web_sys::Node>), MountError> {
    let expected_diagnostic = MountDiagnostic::new(expected);
    let expected_description = format!(
        "text {:?} ({} bytes)",
        expected_diagnostic.preview, expected_diagnostic.input_bytes
    );
    let Some(node) = cursor else {
        return Err(mismatch(path, &expected_description, "<missing>"));
    };
    let actual = describe_node(Some(&node));
    let text = node
        .dyn_into::<web_sys::Text>()
        .map_err(|_| mismatch(path, &expected_description, actual))?;
    if text.length() as usize != expected.encode_utf16().count() || text.data() != expected {
        return Err(mismatch(
            path,
            &expected_description,
            describe_node(Some(text.as_ref())),
        ));
    }
    let next = text.next_sibling();
    Ok((text, next))
}

fn mount_context(
    parent: Option<&MountParentContext>,
    element: &Element,
    namespace: ElementNamespace,
) -> MountParentContext {
    let child_namespace = namespace.for_children(&element.tag);
    MountParentContext {
        tag: element.tag.clone(),
        namespace,
        parser: parent
            .map(|context| context.parser)
            .unwrap_or_default()
            .descend(&element.tag, namespace, child_namespace),
    }
}

fn resolve_rcdata(
    view: &View,
    parent: &MountParentContext,
    direct: &mut DirectChildState,
    depth: usize,
    path: &str,
    preflight: &mut Preflight,
    output: &mut String,
) -> Result<(), MountError> {
    preflight.visit(depth)?;
    match view {
        View::Text(value) => {
            validate_mount_text(value, Some(parent), direct)?;
            output.push_str(value);
            if !value.is_empty() {
                direct.has_serialized_content = true;
            }
            Ok(())
        }
        View::Fragment(children) => {
            for (index, child) in children.iter().enumerate() {
                resolve_rcdata(
                    child,
                    parent,
                    direct,
                    depth + 1,
                    &format!("{path}/fragment[{index}]"),
                    preflight,
                    output,
                )?;
            }
            Ok(())
        }
        View::DynText(_) => Err(RenderError::AdoptionUnsupported {
            parent: parent.tag.to_string(),
            view: "dynamic text",
        }
        .into()),
        View::DynView(_) => Err(RenderError::AdoptionUnsupported {
            parent: parent.tag.to_string(),
            view: "dynamic view",
        }
        .into()),
        View::Keyed(_) => Err(KeyedError::UnsupportedParent {
            tag: parent.tag.to_string(),
        }
        .into()),
        View::Element(element) => Err(mismatch(
            path,
            "static RCDATA text",
            format!("element <{}>", element.tag),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn preflight_view(
    view: &View,
    cursor: Option<web_sys::Node>,
    inherited_namespace: ElementNamespace,
    parent_context: Option<MountParentContext>,
    direct: &mut DirectChildState,
    depth: usize,
    path: &str,
    preflight: &mut Preflight,
) -> Result<(AdoptionPlan, Option<web_sys::Node>), MountError> {
    preflight.visit(depth)?;
    match view {
        View::Text(_) | View::DynText(_) => {
            let (dynamic, value, start_marker, end_marker) = match view {
                View::Text(value) => (None, value.clone(), TEXT_START, TEXT_END),
                View::DynText(source) => (
                    Some(Rc::clone(source)),
                    untrack(|| source()),
                    DYN_TEXT_START,
                    DYN_TEXT_END,
                ),
                _ => unreachable!(),
            };
            let guaranteed_prefix_content = direct.has_serialized_content;
            validate_mount_text(&value, parent_context.as_ref(), direct)?;
            let (start, cursor) = expect_comment(cursor, start_marker, path, preflight)?;
            let (node, cursor) = if value.is_empty() {
                (None, cursor)
            } else {
                let (node, cursor) =
                    expect_text(cursor, &value, &format!("{path}/text"), preflight)?;
                (Some(node), cursor)
            };
            let (end, cursor) = expect_comment(cursor, end_marker, path, preflight)?;
            if !value.is_empty() {
                direct.has_serialized_content = true;
            }
            Ok((
                AdoptionPlan::Text {
                    start,
                    end,
                    node,
                    value,
                    dynamic,
                    parent_context,
                    guaranteed_prefix_content,
                    path: path.to_owned(),
                },
                cursor,
            ))
        }
        View::Fragment(children) => {
            let mut cursor = cursor;
            let mut plans = Vec::with_capacity(children.len());
            for (index, child) in children.iter().enumerate() {
                let (plan, next) = preflight_view(
                    child,
                    cursor,
                    inherited_namespace,
                    parent_context.clone(),
                    direct,
                    depth + 1,
                    &format!("{path}/fragment[{index}]"),
                    preflight,
                )?;
                plans.push(plan);
                cursor = next;
            }
            Ok((AdoptionPlan::Fragment(plans), cursor))
        }
        View::Element(authored) => {
            validate_mount_element(
                parent_context.as_ref(),
                &authored.tag,
                inherited_namespace.for_element(&authored.tag),
            )?;
            let Some(node) = cursor else {
                return Err(mismatch(
                    path,
                    &format!("element <{}>", authored.tag),
                    "<missing>",
                ));
            };
            let actual = describe_node(Some(&node));
            let element = node
                .clone()
                .dyn_into::<web_sys::Element>()
                .map_err(|_| mismatch(path, &format!("element <{}>", authored.tag), actual))?;
            let namespace = inherited_namespace.for_element(&authored.tag);
            if element.namespace_uri().as_deref() != Some(namespace.uri())
                || match namespace {
                    ElementNamespace::Html => !element
                        .local_name()
                        .eq_ignore_ascii_case(authored.tag.as_str()),
                    ElementNamespace::Svg => element.local_name() != authored.tag.as_str(),
                }
            {
                return Err(mismatch(
                    path,
                    &format!("element <{}> in {:?}", authored.tag, namespace.uri()),
                    describe_node(Some(&node)),
                ));
            }

            let mut dynamic_attributes = Vec::new();
            if element.attributes().length() as usize != authored.attrs.len() {
                return Err(mismatch(
                    &format!("{path}/@attributes"),
                    &format!("exactly {} authored attributes", authored.attrs.len()),
                    format!("{} DOM attributes", element.attributes().length()),
                ));
            }
            for (name, value) in &authored.attrs {
                validate_parser_adjusted_svg_attribute(&authored.tag, name, namespace)?;
                let resolved = match value {
                    AttrValue::Static(value) => value.clone(),
                    AttrValue::Dyn(source) => {
                        let value = untrack(|| source()).map_err(RenderError::InvalidAttribute)?;
                        dynamic_attributes.push(DynamicAttributePlan {
                            name: name.clone(),
                            source: Rc::clone(source),
                            initial: value.clone(),
                            namespace,
                        });
                        value
                    }
                };
                validate_attribute_value(name, &resolved)?;
                let expected_namespace = attribute_namespace(namespace, name.as_str());
                let local_name = name
                    .as_str()
                    .split_once(':')
                    .map_or(name.as_str(), |(_, local)| local);
                let attribute = match expected_namespace {
                    Some(namespace) => element.get_attribute_node_ns(Some(namespace), local_name),
                    None => element.get_attribute_node(name.as_str()),
                };
                let expected_units = resolved.encode_utf16().count();
                let (actual_value, actual_units) = match attribute.as_ref() {
                    Some(attribute) => {
                        let raw = js_sys::Reflect::get(
                            attribute.as_ref(),
                            &wasm_bindgen::JsValue::from_str("value"),
                        )
                        .map_err(|error| {
                            dom_error(
                                MountOperation::SetAttribute,
                                "read adopted attribute",
                                error,
                            )
                        })?;
                        let value = js_sys::JsString::from(raw);
                        let units = value.length() as usize;
                        let value = (units == expected_units)
                            .then(|| wasm_bindgen::JsValue::from(value).as_string())
                            .flatten();
                        (value, units)
                    }
                    None => (None, 0),
                };
                let spelling_matches =
                    attribute.as_ref().is_some_and(|attribute| match namespace {
                        ElementNamespace::Html => {
                            attribute.name().eq_ignore_ascii_case(name.as_str())
                        }
                        ElementNamespace::Svg => attribute.name() == name.as_str(),
                    });
                let namespace_matches = attribute.as_ref().is_some_and(|attribute| {
                    attribute.namespace_uri().as_deref() == expected_namespace
                });
                if actual_value.as_deref() != Some(resolved.as_str())
                    || !spelling_matches
                    || !namespace_matches
                {
                    return Err(mismatch(
                        &format!("{path}/@{}", name.as_str()),
                        &format!(
                            "attribute {:?} in {expected_namespace:?} with value {:?}",
                            name.as_str(),
                            MountDiagnostic::new(&resolved).preview
                        ),
                        attribute.map_or_else(
                            || "<missing attribute>".to_owned(),
                            |attribute| {
                                format!(
                                    "attribute {:?} in {:?} with {actual_units} UTF-16 value units",
                                    attribute.name(),
                                    attribute.namespace_uri()
                                )
                            },
                        ),
                    ));
                }
            }

            direct.has_serialized_content = true;
            if authored.tag.is_void_in(namespace) {
                if !authored.children.is_empty() {
                    return Err(MountError::Structure {
                        violation: MountStructureViolation::VoidElementHasChildren,
                        subject: MountDiagnostic::new(authored.tag.as_str()),
                    });
                }
                if element.has_child_nodes() {
                    return Err(mismatch(
                        &format!("{path}/children"),
                        "no children",
                        describe_node(element.first_child().as_ref()),
                    ));
                }
                return Ok((
                    AdoptionPlan::Element {
                        node: element,
                        authored: authored.clone(),
                        dynamic_attributes,
                        children: Vec::new(),
                        rcdata_text: None,
                        path: path.to_owned(),
                    },
                    node.next_sibling(),
                ));
            }

            let child_context = mount_context(parent_context.as_ref(), authored, namespace);
            let is_rcdata = namespace == ElementNamespace::Html
                && ["textarea", "title"]
                    .iter()
                    .any(|tag| authored.tag.as_str().eq_ignore_ascii_case(tag));
            let mut children = Vec::new();
            let mut rcdata_text = None;
            if is_rcdata {
                let mut expected = String::new();
                let mut child_direct = DirectChildState::default();
                for (index, child) in authored.children.iter().enumerate() {
                    resolve_rcdata(
                        child,
                        &child_context,
                        &mut child_direct,
                        depth + 1,
                        &format!("{path}/rcdata[{index}]"),
                        preflight,
                        &mut expected,
                    )?;
                }
                match (expected.is_empty(), element.first_child()) {
                    (true, None) => {}
                    (false, cursor) => {
                        let (text, next) =
                            expect_text(cursor, &expected, &format!("{path}/rcdata"), preflight)?;
                        if next.is_some() {
                            return Err(mismatch(
                                &format!("{path}/rcdata"),
                                "one text node",
                                describe_node(next.as_ref()),
                            ));
                        }
                        rcdata_text = Some(text);
                    }
                    (true, Some(extra)) => {
                        return Err(mismatch(
                            &format!("{path}/rcdata"),
                            "empty text",
                            describe_node(Some(&extra)),
                        ));
                    }
                }
            } else {
                let child_namespace = namespace.for_children(&authored.tag);
                let mut child_cursor = element.first_child();
                let mut child_direct = DirectChildState::default();
                for (index, child) in authored.children.iter().enumerate() {
                    let (plan, next) = preflight_view(
                        child,
                        child_cursor,
                        child_namespace,
                        Some(child_context.clone()),
                        &mut child_direct,
                        depth + 1,
                        &format!("{path}/{}[{index}]", authored.tag),
                        preflight,
                    )?;
                    children.push(plan);
                    child_cursor = next;
                }
                if child_cursor.is_some() {
                    return Err(mismatch(
                        &format!("{path}/children"),
                        "no additional child",
                        describe_node(child_cursor.as_ref()),
                    ));
                }
            }

            Ok((
                AdoptionPlan::Element {
                    node: element,
                    authored: authored.clone(),
                    dynamic_attributes,
                    children,
                    rcdata_text,
                    path: path.to_owned(),
                },
                node.next_sibling(),
            ))
        }
        View::DynView(source) => {
            let guaranteed_prefix_content = direct.has_serialized_content;
            let (start, cursor) = expect_comment(cursor, DYN_START, path, preflight)?;
            let (child_start, cursor) = expect_comment(cursor, MOUNT_START, path, preflight)?;
            let initial = untrack(|| source());
            let (child, cursor) = preflight_view(
                &initial,
                cursor,
                inherited_namespace,
                parent_context.clone(),
                direct,
                depth + 1,
                &format!("{path}/dynamic"),
                preflight,
            )?;
            let (child_end, cursor) = expect_comment(cursor, MOUNT_END, path, preflight)?;
            let (end, cursor) = expect_comment(cursor, DYN_END, path, preflight)?;
            Ok((
                AdoptionPlan::Dynamic {
                    start,
                    end,
                    child_start,
                    child_end,
                    source: Rc::clone(source),
                    child: Box::new(child),
                    namespace: inherited_namespace,
                    parent_context,
                    guaranteed_prefix_content,
                    path: path.to_owned(),
                },
                cursor,
            ))
        }
        View::Keyed(spec) => {
            if let Some(parent) = parent_context.as_ref() {
                if ["pre", "textarea", "title"]
                    .iter()
                    .any(|tag| parent.tag.as_str().eq_ignore_ascii_case(tag))
                {
                    return Err(KeyedError::UnsupportedParent {
                        tag: parent.tag.to_string(),
                    }
                    .into());
                }
            }
            let guaranteed_prefix_content = direct.has_serialized_content;
            let (start, mut cursor) = expect_comment(cursor, KEYED_START, path, preflight)?;
            let pending = untrack(|| spec.collect())?;
            let mut rows = Vec::with_capacity(pending.len());
            for (index, row) in pending.into_iter().enumerate() {
                let (row_start, next) = expect_comment(cursor, ROW_START, path, preflight)?;
                let (key, view) = untrack(|| row.build())?;
                let (child, next) = preflight_view(
                    &view,
                    next,
                    inherited_namespace,
                    parent_context.clone(),
                    direct,
                    depth + 1,
                    &format!("{path}/keyed[{index}]"),
                    preflight,
                )?;
                let (row_end, next) = expect_comment(next, ROW_END, path, preflight)?;
                rows.push(AdoptedRowPlan {
                    key,
                    start: row_start,
                    end: row_end,
                    child,
                });
                cursor = next;
            }
            let (end, cursor) = expect_comment(cursor, KEYED_END, path, preflight)?;
            Ok((
                AdoptionPlan::Keyed {
                    start,
                    end,
                    spec: Rc::clone(spec),
                    rows,
                    namespace: inherited_namespace,
                    parent_context,
                    guaranteed_prefix_content,
                    path: path.to_owned(),
                },
                cursor,
            ))
        }
    }
}

fn attach_existing_range(
    scope: &MountScope,
    start: &web_sys::Comment,
    end: &web_sys::Comment,
    label: &'static str,
) -> Result<(), MountError> {
    let (_, owned_top_level) = snapshot_boundary_range(start, end, label)?;
    scope.attach_range(DomRange {
        start: start.clone(),
        end: end.clone(),
        owned_top_level,
        label,
    })
}

fn collect_subtree_nodes(node: &web_sys::Node, nodes: &mut Vec<web_sys::Node>) {
    nodes.push(node.clone());
    let mut child = node.first_child();
    while let Some(node) = child {
        child = node.next_sibling();
        collect_subtree_nodes(&node, nodes);
    }
}

fn rollback_seed(nodes: &[web_sys::Node]) -> Result<(), MountError> {
    let mut first_error = None;
    let mut passes = 0;
    for pass in 0..MAX_CLEANUP_DRAIN_PASSES {
        passes = pass + 1;
        for node in nodes.iter().rev() {
            let Some(parent) = node.parent_node() else {
                continue;
            };
            if let Err(error) = remove_child(&parent, node, "failed SSR adoption seed") {
                first_error.get_or_insert(error);
            }
        }
        if nodes.iter().all(|node| node.parent_node().is_none()) {
            return first_error.map_or(Ok(()), Err);
        }
    }
    Err(MountError::CleanupDidNotConverge {
        subject: MountDiagnostic::new("failed SSR adoption seed"),
        remaining_owned_nodes: nodes
            .iter()
            .filter(|node| node.parent_node().is_some())
            .count(),
        passes,
    })
}

#[allow(clippy::too_many_arguments)]
fn install_adopted_dynamic_text(
    scope: &MountScope,
    node: web_sys::Text,
    source: Rc<dyn Fn() -> String>,
    initial: String,
    parent_context: Option<MountParentContext>,
    guaranteed_prefix_content: bool,
    path: String,
) -> Result<(), MountError> {
    let cleanup = scope.cleanup_weak()?;
    let errors = scope.errors.clone();
    let first_run = Rc::new(Cell::new(true));
    let first_run_effect = Rc::clone(&first_run);
    let initial_error = Rc::new(RefCell::new(None));
    let initial_error_effect = Rc::clone(&initial_error);
    scope.owner_operation(MountOperation::InstallEffect, move |owner| {
        owner.effect(move || {
            let is_initial = first_run_effect.replace(false);
            let result = (|| {
                let cleanup = cleanup.upgrade().ok_or(MountError::Reactive {
                    operation: MountOperation::InstallEffect,
                    source: OwnerError::Disposed,
                })?;
                let chain = cleanup.active_chain(MountOperation::InstallEffect)?;
                let candidate = source();
                let direct = DirectChildState {
                    has_serialized_content: guaranteed_prefix_content,
                };
                validate_mount_text(&candidate, parent_context.as_ref(), &direct)?;
                for cleanup in &chain {
                    cleanup.ensure_active(MountOperation::InstallEffect)?;
                }
                if is_initial {
                    if candidate != initial || node.data() != initial {
                        return Err(mismatch(
                            &path,
                            &format!("dynamic text seed {initial:?}"),
                            format!("callback {:?}, DOM {:?}", candidate, node.data()),
                        ));
                    }
                } else {
                    node.set_data(&candidate);
                }
                Ok::<(), MountError>(())
            })();
            if let Err(error) = result {
                errors.record(error.clone());
                if is_initial {
                    *initial_error_effect.borrow_mut() = Some(error);
                }
            }
        })
    })?;
    if let Some(error) = initial_error.borrow_mut().take() {
        return Err(error);
    }
    Ok(())
}

fn install_adopted_dynamic_attribute(
    scope: &MountScope,
    element: web_sys::Element,
    plan: DynamicAttributePlan,
    path: String,
) -> Result<(), MountError> {
    let cleanup = scope.cleanup_weak()?;
    let errors = scope.errors.clone();
    let first_run = Rc::new(Cell::new(true));
    let first_run_effect = Rc::clone(&first_run);
    let initial_error = Rc::new(RefCell::new(None));
    let initial_error_effect = Rc::clone(&initial_error);
    scope.owner_operation(MountOperation::InstallEffect, move |owner| {
        owner.effect(move || {
            let is_initial = first_run_effect.replace(false);
            let result = (|| {
                let cleanup = cleanup.upgrade().ok_or(MountError::Reactive {
                    operation: MountOperation::SetAttribute,
                    source: OwnerError::Disposed,
                })?;
                let chain = cleanup.active_chain(MountOperation::SetAttribute)?;
                let candidate = (plan.source)().map_err(RenderError::InvalidAttribute)?;
                validate_attribute_value(&plan.name, &candidate)?;
                for cleanup in &chain {
                    cleanup.ensure_active(MountOperation::SetAttribute)?;
                }
                if is_initial {
                    let actual = element.get_attribute(plan.name.as_str());
                    if candidate != plan.initial || actual.as_deref() != Some(plan.initial.as_str())
                    {
                        return Err(mismatch(
                            &path,
                            &format!("dynamic attribute seed {:?}", plan.initial),
                            format!("callback {candidate:?}, DOM {actual:?}"),
                        ));
                    }
                } else {
                    set_attribute(&element, plan.namespace, plan.name.as_str(), &candidate)?;
                }
                Ok::<(), MountError>(())
            })();
            if let Err(error) = result {
                errors.record(error.clone());
                if is_initial {
                    *initial_error_effect.borrow_mut() = Some(error);
                }
            }
        })
    })?;
    if let Some(error) = initial_error.borrow_mut().take() {
        return Err(error);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_dynamic_seed(
    fresh: &View,
    child_start: &web_sys::Comment,
    child_end: &web_sys::Comment,
    namespace: ElementNamespace,
    parent_context: Option<MountParentContext>,
    guaranteed_prefix_content: bool,
    limits: RenderLimits,
    path: &str,
) -> Result<(), MountError> {
    let mut preflight = Preflight::new(limits);
    let mut direct = DirectChildState {
        has_serialized_content: guaranteed_prefix_content,
    };
    let (plan, cursor) = preflight_view(
        fresh,
        child_start.next_sibling(),
        namespace,
        parent_context,
        &mut direct,
        1,
        path,
        &mut preflight,
    )?;
    drop(plan);
    if !cursor
        .as_ref()
        .is_some_and(|node| node.is_same_node(Some(child_end.as_ref())))
    {
        return Err(mismatch(
            path,
            "the adopted dynamic end boundary",
            describe_node(cursor.as_ref()),
        ));
    }
    Ok(())
}

fn commit_plan(
    document: &web_sys::Document,
    plan: AdoptionPlan,
    scope: &MountScope,
    limits: RenderLimits,
) -> Result<(), MountError> {
    match plan {
        AdoptionPlan::Text {
            start,
            end,
            node,
            value,
            dynamic,
            parent_context,
            guaranteed_prefix_content,
            path,
        } => {
            scope.register_node(start.as_ref())?;
            let node = match node {
                Some(node) => node,
                None if dynamic.is_some() => {
                    let node = document.create_text_node("");
                    let parent = end.parent_node().ok_or_else(|| {
                        mismatch(&path, "attached dynamic text boundary", "detached boundary")
                    })?;
                    insert_before(
                        &parent,
                        node.as_ref(),
                        Some(end.as_ref()),
                        "adopted empty dynamic text",
                    )?;
                    node
                }
                None => {
                    scope.register_node(end.as_ref())?;
                    return Ok(());
                }
            };
            scope.register_node(node.as_ref())?;
            scope.register_node(end.as_ref())?;
            if let Some(source) = dynamic {
                install_adopted_dynamic_text(
                    scope,
                    node,
                    source,
                    value,
                    parent_context,
                    guaranteed_prefix_content,
                    path,
                )?;
            }
            Ok(())
        }
        AdoptionPlan::Element {
            node,
            authored,
            dynamic_attributes,
            children,
            rcdata_text,
            path,
        } => {
            scope.register_node(node.as_ref())?;
            if let Some(text) = rcdata_text {
                scope.register_node(text.as_ref())?;
            }
            for plan in dynamic_attributes {
                let attribute_path = format!("{path}/@{}", plan.name.as_str());
                install_adopted_dynamic_attribute(scope, node.clone(), plan, attribute_path)?;
            }
            for (event, handler) in &authored.listeners {
                install_listener(scope, &node, event.as_str(), Rc::clone(handler))?;
            }
            for child in children {
                commit_plan(document, child, scope, limits)?;
            }
            Ok(())
        }
        AdoptionPlan::Fragment(children) => {
            for child in children {
                commit_plan(document, child, scope, limits)?;
            }
            Ok(())
        }
        AdoptionPlan::Dynamic {
            start,
            end,
            child_start,
            child_end,
            source,
            child,
            namespace,
            parent_context,
            guaranteed_prefix_content,
            path,
        } => {
            scope.register_node(start.as_ref())?;
            scope.register_node(end.as_ref())?;
            let parent_cleanup = scope
                .cleanup_weak()?
                .upgrade()
                .ok_or(MountError::Reactive {
                    operation: MountOperation::RegisterCleanup,
                    source: OwnerError::Disposed,
                })?;
            let errors = scope.errors.fork_origin();
            let child_scope = MountScope::with_errors_and_ancestors(
                errors.clone(),
                parent_cleanup.child_ancestry(),
            );
            child_scope.register_node(child_start.as_ref())?;
            commit_plan(document, *child, &child_scope, limits)?;
            child_scope.register_node(child_end.as_ref())?;
            attach_existing_range(
                &child_scope,
                &child_start,
                &child_end,
                "adopted dynamic child range",
            )?;
            child_scope.validate_owned_range()?;
            let state = Rc::new(RefCell::new(DynamicSlotState {
                status: DynamicSlotStatus::Ready,
                stable: Some(child_scope),
            }));
            let validator_start = child_start.clone();
            let validator_end = child_end.clone();
            let validator_context = parent_context.clone();
            let validator_path = format!("{path}/dynamic");
            let validator: InitialDynamicValidator = Rc::new(move |fresh| {
                validate_dynamic_seed(
                    fresh,
                    &validator_start,
                    &validator_end,
                    namespace,
                    validator_context.clone(),
                    guaranteed_prefix_content,
                    limits,
                    &validator_path,
                )
            });
            install_dynamic_view_effect(
                document,
                source,
                start,
                end,
                state,
                namespace,
                parent_context,
                guaranteed_prefix_content,
                scope,
                Some(validator),
                errors,
            )
        }
        AdoptionPlan::Keyed {
            start,
            end,
            spec,
            rows,
            namespace,
            parent_context,
            guaranteed_prefix_content,
            path,
        } => {
            scope.register_node(start.as_ref())?;
            scope.register_node(end.as_ref())?;
            let parent_cleanup = scope
                .cleanup_weak()?
                .upgrade()
                .ok_or(MountError::Reactive {
                    operation: MountOperation::RegisterCleanup,
                    source: OwnerError::Disposed,
                })?;
            let errors = scope.errors.fork_origin();
            let mut adopted_rows = Vec::with_capacity(rows.len());
            let mut expected_keys = Vec::with_capacity(rows.len());
            for (index, row) in rows.into_iter().enumerate() {
                expected_keys.push(row.key.clone());
                let row_scope = MountScope::with_errors_and_ancestors(
                    errors.clone(),
                    parent_cleanup.child_ancestry(),
                );
                row_scope.register_node(row.start.as_ref())?;
                commit_plan(document, row.child, &row_scope, limits)?;
                row_scope.register_node(row.end.as_ref())?;
                attach_existing_range(&row_scope, &row.start, &row.end, "adopted keyed row range")?;
                row_scope
                    .validate_owned_range()
                    .map_err(|error| MountError::AdoptionMismatch {
                        path: MountDiagnostic::new(&format!("{path}/keyed[{index}]")),
                        expected: MountDiagnostic::new("one exact keyed row range"),
                        actual: MountDiagnostic::new(&error.to_string()),
                    })?;
                adopted_rows.push(KeyedRow {
                    key: row.key,
                    scope: row_scope,
                });
            }
            let state = Rc::new(RefCell::new(KeyedSlotState {
                status: KeyedSlotStatus::Ready,
                rows: adopted_rows,
            }));
            install_keyed_view_effect(
                document,
                spec,
                start,
                end,
                state,
                namespace,
                parent_context,
                guaranteed_prefix_content,
                scope,
                errors,
                Some((path, expected_keys)),
            )
        }
    }
}

/// Adopt a versioned SSR seed under `parent` without rebuilding authored DOM.
pub fn adopt(view: &View, parent: &web_sys::Node) -> Result<MountedRoot, MountError> {
    adopt_with_limits(view, parent, RenderLimits::default())
}

/// Adopt a versioned SSR seed under the element with `id`.
pub fn adopt_to(id: &str, view: &View) -> Result<MountedRoot, MountError> {
    let document = browser_document()?;
    let host = document
        .get_element_by_id(id)
        .ok_or_else(|| MountError::HostNotFound {
            id: MountDiagnostic::new(id),
        })?;
    adopt(view, host.as_ref())
}

/// Adopt a versioned SSR seed under explicit traversal limits.
pub fn adopt_with_limits(
    view: &View,
    parent: &web_sys::Node,
    limits: RenderLimits,
) -> Result<MountedRoot, MountError> {
    let document = document_for(parent)?;
    let namespace = namespace_for_children(parent)?;
    let parent_context = mount_parent_context(parent)?;
    let mut preflight = Preflight::new(limits);
    let (root_start, cursor) =
        expect_comment(parent.first_child(), ROOT_START, "$", &mut preflight)?;
    let mut direct = DirectChildState::default();
    let (plan, cursor) = preflight_view(
        view,
        cursor,
        namespace,
        parent_context,
        &mut direct,
        1,
        "$",
        &mut preflight,
    )?;
    let (root_end, cursor) = expect_comment(cursor, ROOT_END, "$", &mut preflight)?;
    if cursor.is_some() {
        return Err(mismatch(
            "$",
            "end of adoptable root",
            describe_node(cursor.as_ref()),
        ));
    }

    let (_, root_nodes) = snapshot_boundary_range(&root_start, &root_end, "SSR adoption seed")?;
    let mut rollback_nodes = Vec::new();
    for node in &root_nodes {
        collect_subtree_nodes(node, &mut rollback_nodes);
    }

    let errors = ErrorSlot::default();
    let scope = MountScope::with_errors(errors);
    scope.register_node(root_start.as_ref())?;
    scope.register_node(root_end.as_ref())?;
    if let Err(error) = commit_plan(&document, plan, &scope, limits) {
        scope.dispose();
        return match rollback_seed(&rollback_nodes) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(cleanup),
        };
    }
    let finalize = attach_existing_range(&scope, &root_start, &root_end, "adopted root range")
        .and_then(|()| scope.validate_owned_range());
    if let Err(error) = finalize {
        scope.dispose();
        return match rollback_seed(&rollback_nodes) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(cleanup),
        };
    }
    Ok(MountedRoot { scope })
}
