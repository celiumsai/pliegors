// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! Browser DOM mounting with explicit, deterministic ownership.
//!
//! A mount is first materialized in a `DocumentFragment`. The fragment is
//! attached only after the complete static tree has been built. Reactive
//! segments use the same rule for every replacement: stage the candidate,
//! insert it, and only then retire the previous stable range.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use pliego_reactive::{Owner, OwnerError};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use super::{
    AttrValue, DirectChildState, DomError, Element, ElementNamespace, ParentContext, ParserContext,
    RenderError, TagName, View, validate_attribute_value, validate_direct_element,
    validate_direct_text, validate_parser_adjusted_svg_attribute, validate_parser_adjusted_svg_tag,
};

/// Maximum text retained from a browser exception or caller-controlled host ID.
pub const MAX_MOUNT_DIAGNOSTIC_BYTES: usize = 256;

const XLINK_URI: &str = "http://www.w3.org/1999/xlink";
const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";

/// A bounded value embedded in a mount diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountDiagnostic {
    pub preview: String,
    pub input_bytes: usize,
    pub preview_truncated: bool,
}

impl MountDiagnostic {
    fn new(value: &str) -> Self {
        let mut end = value.len().min(MAX_MOUNT_DIAGNOSTIC_BYTES);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            preview: value[..end].to_owned(),
            input_bytes: value.len(),
            preview_truncated: end < value.len(),
        }
    }
}

impl fmt::Display for MountDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.preview)?;
        if self.preview_truncated {
            write!(f, " (truncated from {} bytes)", self.input_bytes)?;
        }
        Ok(())
    }
}

/// Browser operation that failed while materializing or disposing a mount.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountOperation {
    AppendNode,
    InsertRange,
    RemoveNode,
    CreateElement,
    SetAttribute,
    AddEventListener,
    RemoveEventListener,
    RegisterCleanup,
    InstallEffect,
}

impl fmt::Display for MountOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AppendNode => "append DOM node",
            Self::InsertRange => "insert DOM range",
            Self::RemoveNode => "remove DOM node",
            Self::CreateElement => "create DOM element",
            Self::SetAttribute => "set DOM attribute",
            Self::AddEventListener => "add event listener",
            Self::RemoveEventListener => "remove event listener",
            Self::RegisterCleanup => "register mount cleanup",
            Self::InstallEffect => "install render effect",
        })
    }
}

/// Structural condition that prevents a safe mount or cleanup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountStructureViolation {
    VoidElementHasChildren,
    BoundaryDetached,
    BoundaryParentsDiffer,
    BoundaryEndNotReachable,
    BoundaryOwnershipMismatch,
    DynamicUpdateReentered,
}

impl fmt::Display for MountStructureViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::VoidElementHasChildren => "an HTML void element has children",
            Self::BoundaryDetached => "a mount boundary is detached",
            Self::BoundaryParentsDiffer => "mount boundaries have different parents",
            Self::BoundaryEndNotReachable => "the end boundary is not reachable from the start",
            Self::BoundaryOwnershipMismatch => {
                "live range does not equal the scope's owned node sequence"
            }
            Self::DynamicUpdateReentered => {
                "a dynamic slot update re-entered an active transaction"
            }
        })
    }
}

/// A bounded, structured DOM mount failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MountError {
    WindowUnavailable,
    DocumentUnavailable,
    BodyUnavailable,
    HostNotFound {
        id: MountDiagnostic,
    },
    UnsupportedNamespace {
        namespace: MountDiagnostic,
    },
    InvalidView(DomError),
    InvalidRender(RenderError),
    DynamicUpdatePoisoned {
        cause: Box<MountError>,
    },
    DiagnosticOverflow {
        dropped_recoverable_events: usize,
        dropped_terminal_events: usize,
    },
    CleanupDidNotConverge {
        subject: MountDiagnostic,
        remaining_owned_nodes: usize,
        passes: usize,
    },
    Structure {
        violation: MountStructureViolation,
        subject: MountDiagnostic,
    },
    Dom {
        operation: MountOperation,
        subject: MountDiagnostic,
        detail: MountDiagnostic,
    },
    Reactive {
        operation: MountOperation,
        source: OwnerError,
    },
}

impl fmt::Display for MountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowUnavailable => f.write_str("browser window is unavailable"),
            Self::DocumentUnavailable => f.write_str("browser document is unavailable"),
            Self::BodyUnavailable => f.write_str("document body is unavailable"),
            Self::HostNotFound { id } => write!(f, "mount host with id {id} was not found"),
            Self::UnsupportedNamespace { namespace } => {
                write!(f, "mount host namespace {namespace} is unsupported")
            }
            Self::InvalidView(error) => write!(f, "invalid mount view: {error}"),
            Self::InvalidRender(error) => write!(f, "mount would diverge from SSR: {error}"),
            Self::DynamicUpdatePoisoned { cause } => {
                write!(f, "dynamic slot is terminally poisoned: {cause}")
            }
            Self::DiagnosticOverflow {
                dropped_recoverable_events,
                dropped_terminal_events,
            } => write!(
                f,
                "mount diagnostic queue dropped {dropped_recoverable_events} recoverable and \
                 {dropped_terminal_events} terminal events"
            ),
            Self::CleanupDidNotConverge {
                subject,
                remaining_owned_nodes,
                passes,
            } => write!(
                f,
                "cleanup for {subject} did not converge after {passes} passes; \
                 {remaining_owned_nodes} owned nodes remain connected"
            ),
            Self::Structure { violation, subject } => {
                write!(f, "unsafe DOM structure for {subject}: {violation}")
            }
            Self::Dom {
                operation,
                subject,
                detail,
            } => write!(f, "failed to {operation} for {subject}: {detail}"),
            Self::Reactive { operation, source } => {
                write!(f, "failed to {operation}: {source}")
            }
        }
    }
}

impl std::error::Error for MountError {}

impl From<DomError> for MountError {
    fn from(error: DomError) -> Self {
        Self::InvalidView(error)
    }
}

impl From<RenderError> for MountError {
    fn from(error: RenderError) -> Self {
        Self::InvalidRender(error)
    }
}

const MAX_MOUNT_ERROR_EVENTS: usize = 64;
const MAX_CLEANUP_DRAIN_PASSES: usize = 64;

#[derive(Clone)]
struct ErrorEvent {
    error: MountError,
    terminal: bool,
}

#[derive(Default)]
struct ErrorState {
    events: VecDeque<ErrorEvent>,
    dropped_recoverable_events: usize,
    dropped_terminal_events: usize,
}

#[derive(Default)]
struct ErrorOriginState {
    terminal_revision: u64,
    latest_terminal: Option<MountError>,
    parent: Option<Weak<RefCell<ErrorOriginState>>>,
}

#[derive(Clone)]
struct ErrorSlot {
    queue: Rc<RefCell<ErrorState>>,
    origin: Rc<RefCell<ErrorOriginState>>,
}

impl Default for ErrorSlot {
    fn default() -> Self {
        Self {
            queue: Rc::new(RefCell::new(ErrorState::default())),
            origin: Rc::new(RefCell::new(ErrorOriginState::default())),
        }
    }
}

impl ErrorSlot {
    fn fork_origin(&self) -> Self {
        Self {
            queue: Rc::clone(&self.queue),
            origin: Rc::new(RefCell::new(ErrorOriginState {
                parent: Some(Rc::downgrade(&self.origin)),
                ..ErrorOriginState::default()
            })),
        }
    }

    fn record(&self, error: MountError) {
        let mut state = self.queue.borrow_mut();
        Self::push(&mut state, error, false);
    }

    fn record_terminal(&self, error: MountError) {
        let mut cursor = Some(Rc::clone(&self.origin));
        while let Some(origin) = cursor {
            let parent = {
                let mut origin = origin.borrow_mut();
                origin.terminal_revision = origin.terminal_revision.wrapping_add(1);
                origin.latest_terminal = Some(error.clone());
                origin.parent.clone()
            };
            cursor = parent.and_then(|parent| parent.upgrade());
        }
        let mut state = self.queue.borrow_mut();
        Self::push(&mut state, error, true);
    }

    fn push(state: &mut ErrorState, error: MountError, terminal: bool) {
        if state.events.len() == MAX_MOUNT_ERROR_EVENTS {
            let removed =
                if let Some(recoverable) = state.events.iter().position(|event| !event.terminal) {
                    state.events.remove(recoverable)
                } else {
                    state.events.pop_front()
                };
            if removed.is_some_and(|event| event.terminal) {
                state.dropped_terminal_events = state.dropped_terminal_events.saturating_add(1);
            } else {
                state.dropped_recoverable_events =
                    state.dropped_recoverable_events.saturating_add(1);
            }
        }
        state.events.push_back(ErrorEvent { error, terminal });
    }

    fn last(&self) -> Option<MountError> {
        let state = self.queue.borrow();
        state
            .events
            .back()
            .map(|event| event.error.clone())
            .or_else(|| {
                (state.dropped_recoverable_events != 0 || state.dropped_terminal_events != 0).then(
                    || {
                        diagnostic_overflow(
                            state.dropped_recoverable_events,
                            state.dropped_terminal_events,
                        )
                    },
                )
            })
    }

    fn take(&self) -> Option<MountError> {
        let mut state = self.queue.borrow_mut();
        state
            .events
            .pop_back()
            .map(|event| event.error)
            .or_else(|| {
                let recoverable = std::mem::take(&mut state.dropped_recoverable_events);
                let terminal = std::mem::take(&mut state.dropped_terminal_events);
                (recoverable != 0 || terminal != 0)
                    .then(|| diagnostic_overflow(recoverable, terminal))
            })
    }

    fn terminal_checkpoint(&self) -> u64 {
        self.origin.borrow().terminal_revision
    }

    fn terminal_recorded_after(&self, checkpoint: u64) -> Option<MountError> {
        let origin = self.origin.borrow();
        (origin.terminal_revision != checkpoint)
            .then(|| origin.latest_terminal.clone())
            .flatten()
    }
}

fn diagnostic_overflow(recoverable: usize, terminal: usize) -> MountError {
    MountError::DiagnosticOverflow {
        dropped_recoverable_events: recoverable,
        dropped_terminal_events: terminal,
    }
}

#[derive(Clone)]
struct MountParentContext {
    tag: TagName,
    namespace: ElementNamespace,
    parser: ParserContext,
}

impl MountParentContext {
    fn render_context(&self) -> ParentContext<'_> {
        ParentContext {
            tag: &self.tag,
            namespace: self.namespace,
            parser: self.parser,
        }
    }
}

struct DomRange {
    start: web_sys::Comment,
    end: web_sys::Comment,
    owned_top_level: Vec<web_sys::Node>,
    label: &'static str,
}

struct RangeRemovalOutcome {
    reinserted: bool,
    had_specific_error: bool,
}

impl DomRange {
    fn validate_exact(&self) -> Result<(), MountError> {
        validate_node_sequence(&self.start, &self.end, &self.owned_top_level, self.label)
    }

    fn validate_owned_nodes_present(&self) -> Result<(), MountError> {
        validate_boundary_range(&self.start, &self.end, self.label)?;
        if self.owned_top_level.is_empty()
            || !self.owned_top_level[0].is_same_node(Some(self.start.as_ref()))
            || !self.owned_top_level[self.owned_top_level.len() - 1]
                .is_same_node(Some(self.end.as_ref()))
        {
            return Err(ownership_mismatch(self.label));
        }

        // Foreign siblings may appear inside a live range without becoming
        // ours. Every frozen owned node must nevertheless remain in order
        // between the boundaries; a missing/reordered node is corruption.
        let mut cursor: Option<web_sys::Node> = Some(self.start.clone().into());
        for expected in &self.owned_top_level {
            loop {
                let node = cursor.ok_or_else(|| ownership_mismatch(self.label))?;
                cursor = node.next_sibling();
                if node.is_same_node(Some(expected)) {
                    break;
                }
                if node.is_same_node(Some(self.end.as_ref())) {
                    return Err(ownership_mismatch(self.label));
                }
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> OwnedRangeSnapshot {
        OwnedRangeSnapshot {
            start: self.start.clone(),
            end: self.end.clone(),
            nodes: self.owned_top_level.clone(),
        }
    }

    fn validate_nodes(&self, nodes: &[web_sys::Node]) -> Result<(), MountError> {
        validate_node_sequence(&self.start, &self.end, nodes, self.label)
    }

    fn replace_nodes(&mut self, nodes: Vec<web_sys::Node>) {
        self.owned_top_level = nodes;
    }

    fn remove(self, errors: &ErrorSlot, report_ownership_mismatch: bool) -> RangeRemovalOutcome {
        let mut had_specific_error = false;
        if report_ownership_mismatch {
            if let Err(error) = self.validate_exact() {
                errors.record(error);
                had_specific_error = true;
            }
        }

        // This list was frozen while the stage was detached. Never discover
        // ownership from the live range during cleanup: a caller may have
        // inserted unrelated siblings between the markers after mount.
        for node in self.owned_top_level.iter().rev() {
            let Some(parent) = node.parent_node() else {
                continue;
            };
            if let Err(error) = remove_child(&parent, node, self.label) {
                errors.record(error);
                had_specific_error = true;
            }
        }
        RangeRemovalOutcome {
            reinserted: self
                .owned_top_level
                .iter()
                .any(|node| node.parent_node().is_some()),
            had_specific_error,
        }
    }
}

fn ownership_mismatch(label: &'static str) -> MountError {
    MountError::Structure {
        violation: MountStructureViolation::BoundaryOwnershipMismatch,
        subject: MountDiagnostic::new(label),
    }
}

#[derive(Clone)]
struct OwnedRangeSnapshot {
    start: web_sys::Comment,
    end: web_sys::Comment,
    nodes: Vec<web_sys::Node>,
}

fn validate_node_sequence(
    start: &web_sys::Comment,
    end: &web_sys::Comment,
    expected: &[web_sys::Node],
    label: &'static str,
) -> Result<(), MountError> {
    validate_boundary_range(start, end, label)?;
    if expected.is_empty()
        || !expected[0].is_same_node(Some(start.as_ref()))
        || !expected[expected.len() - 1].is_same_node(Some(end.as_ref()))
    {
        return Err(MountError::Structure {
            violation: MountStructureViolation::BoundaryOwnershipMismatch,
            subject: MountDiagnostic::new(label),
        });
    }
    let mut actual: Option<web_sys::Node> = Some(start.clone().into());
    for expected_node in expected {
        let node = actual.ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryOwnershipMismatch,
            subject: MountDiagnostic::new(label),
        })?;
        if !node.is_same_node(Some(expected_node)) {
            return Err(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new(label),
            });
        }
        actual = node.next_sibling();
    }
    Ok(())
}

fn validate_dynamic_layout(
    outer_start: &web_sys::Comment,
    outer_end: &web_sys::Comment,
    segments: &[&OwnedRangeSnapshot],
) -> Result<(), MountError> {
    validate_boundary_range(outer_start, outer_end, "dynamic range")?;

    let mut cursor = outer_start.next_sibling();
    for segment in segments {
        validate_node_sequence(
            &segment.start,
            &segment.end,
            &segment.nodes,
            "dynamic child range",
        )?;
        let first = segment.nodes.first().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryOwnershipMismatch,
            subject: MountDiagnostic::new("dynamic child range"),
        })?;
        if !cursor
            .as_ref()
            .is_some_and(|node| node.is_same_node(Some(first)))
        {
            return Err(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new("dynamic range"),
            });
        }
        cursor = segment.end.next_sibling();
    }

    if !cursor
        .as_ref()
        .is_some_and(|node| node.is_same_node(Some(outer_end.as_ref())))
    {
        return Err(MountError::Structure {
            violation: MountStructureViolation::BoundaryOwnershipMismatch,
            subject: MountDiagnostic::new("dynamic range"),
        });
    }
    Ok(())
}

enum DynamicSlotStatus {
    Ready,
    Updating,
    Poisoned { cause: MountError },
}

struct DynamicSlotState {
    status: DynamicSlotStatus,
    stable: Option<MountScope>,
}

impl DynamicSlotState {
    fn new() -> Self {
        Self {
            status: DynamicSlotStatus::Ready,
            stable: None,
        }
    }

    fn begin_update(&mut self) -> Result<bool, MountError> {
        match &self.status {
            DynamicSlotStatus::Ready => {
                self.status = DynamicSlotStatus::Updating;
                Ok(true)
            }
            DynamicSlotStatus::Updating => {
                let cause = dynamic_update_poisoned(dynamic_update_reentered());
                self.status = DynamicSlotStatus::Poisoned {
                    cause: cause.clone(),
                };
                Err(cause)
            }
            DynamicSlotStatus::Poisoned { .. } => Ok(false),
        }
    }

    fn ensure_updating(&self) -> Result<(), MountError> {
        match &self.status {
            DynamicSlotStatus::Updating => Ok(()),
            DynamicSlotStatus::Poisoned { cause } => Err(cause.clone()),
            DynamicSlotStatus::Ready => Err(dynamic_update_reentered()),
        }
    }

    fn stable_snapshot(&self) -> Result<Option<OwnedRangeSnapshot>, MountError> {
        self.stable
            .as_ref()
            .map(MountScope::owned_range_snapshot)
            .transpose()
    }

    fn validate_stable_ownership(&self) -> Result<(), MountError> {
        match self.stable.as_ref() {
            Some(stable) => stable.validate_owned_range(),
            None => Ok(()),
        }
    }

    fn validate_stable_owned_nodes_present(&self) -> Result<(), MountError> {
        match self.stable.as_ref() {
            Some(stable) => stable.validate_owned_nodes_present(),
            None => Ok(()),
        }
    }

    fn suppress_stable_ownership_mismatch(&self) {
        if let Some(stable) = self.stable.as_ref() {
            stable.suppress_cleanup_ownership_mismatch();
        }
    }

    fn take_stable(&mut self) -> Option<MountScope> {
        self.stable.take()
    }

    fn replace_candidate(
        &mut self,
        candidate: MountScope,
    ) -> Result<Option<MountScope>, MountError> {
        self.ensure_updating()?;
        Ok(self.stable.replace(candidate))
    }

    fn finish_update(&mut self) -> Result<(), MountError> {
        match &self.status {
            DynamicSlotStatus::Updating => {
                self.status = DynamicSlotStatus::Ready;
                Ok(())
            }
            DynamicSlotStatus::Poisoned { cause } => Err(cause.clone()),
            DynamicSlotStatus::Ready => Err(dynamic_update_reentered()),
        }
    }

    fn poison(&mut self, error: MountError) -> (MountError, bool) {
        if let DynamicSlotStatus::Poisoned { cause } = &self.status {
            return (cause.clone(), false);
        }
        let error = dynamic_update_poisoned(error);
        self.status = DynamicSlotStatus::Poisoned {
            cause: error.clone(),
        };
        (error, true)
    }

    fn fail_update(&mut self, error: MountError, irreversible: bool) -> (MountError, bool, bool) {
        match &self.status {
            DynamicSlotStatus::Poisoned { cause } => (cause.clone(), true, false),
            DynamicSlotStatus::Updating if !irreversible => {
                self.status = DynamicSlotStatus::Ready;
                (error, false, false)
            }
            DynamicSlotStatus::Updating | DynamicSlotStatus::Ready => {
                let (error, newly_terminal) = self.poison(error);
                (error, true, newly_terminal)
            }
        }
    }
}

fn dynamic_update_reentered() -> MountError {
    MountError::Structure {
        violation: MountStructureViolation::DynamicUpdateReentered,
        subject: MountDiagnostic::new("dynamic slot"),
    }
}

fn dynamic_update_poisoned(cause: MountError) -> MountError {
    match cause {
        poisoned @ MountError::DynamicUpdatePoisoned { .. } => poisoned,
        cause => MountError::DynamicUpdatePoisoned {
            cause: Box::new(cause),
        },
    }
}

fn has_structural_failure(error: &MountError) -> bool {
    match error {
        MountError::DynamicUpdatePoisoned { cause } => has_structural_failure(cause),
        MountError::Structure { .. } => true,
        _ => false,
    }
}

fn dynamic_stable_snapshot(
    state: &Rc<RefCell<DynamicSlotState>>,
) -> Result<Option<OwnedRangeSnapshot>, MountError> {
    state.borrow().stable_snapshot()
}

fn poison_and_retire_corrupted_stable(
    state: &Rc<RefCell<DynamicSlotState>>,
    parent_cleanup: &Weak<MountCleanup>,
    outer_start: &web_sys::Comment,
    outer_end: &web_sys::Comment,
    errors: &ErrorSlot,
    cause: MountError,
) -> MountError {
    state.borrow().suppress_stable_ownership_mismatch();
    let (terminal, newly_terminal) = state.borrow_mut().poison(cause);

    let cleanup_chain = parent_cleanup
        .upgrade()
        .and_then(|cleanup| cleanup.live_chain().ok());

    let retired = state.borrow_mut().take_stable();
    drop(retired);

    if let Some(cleanup_chain) = cleanup_chain.as_ref() {
        for cleanup in cleanup_chain {
            cleanup.forget_dynamic_slot_topology(outer_start, outer_end);
        }
    }
    let outer_end_node: &web_sys::Node = outer_end.as_ref();
    let outer_start_node: &web_sys::Node = outer_start.as_ref();
    let markers = [outer_end_node, outer_start_node];
    let mut passes = 0;
    for pass in 0..MAX_CLEANUP_DRAIN_PASSES {
        passes = pass + 1;
        for marker in markers {
            if let Some(parent) = marker.parent_node() {
                if let Err(error) = remove_child(&parent, marker, "poisoned dynamic boundary") {
                    errors.record(error);
                }
            }
        }
        if markers.iter().all(|marker| marker.parent_node().is_none()) {
            break;
        }
    }
    let remaining_owned_nodes = markers
        .iter()
        .filter(|marker| marker.parent_node().is_some())
        .count();
    if remaining_owned_nodes == 0 {
        if let Some(cleanup_chain) = cleanup_chain.as_ref() {
            for cleanup in cleanup_chain {
                cleanup.forget_dynamic_boundary_registry(outer_start, outer_end);
            }
        }
    } else {
        errors.record_terminal(MountError::CleanupDidNotConverge {
            subject: MountDiagnostic::new("poisoned dynamic boundaries"),
            remaining_owned_nodes,
            passes,
        });
    }
    if newly_terminal {
        errors.record_terminal(terminal.clone());
    }
    terminal
}

fn validate_dynamic_stable_layout(
    outer_start: &web_sys::Comment,
    outer_end: &web_sys::Comment,
    stable: Option<&OwnedRangeSnapshot>,
) -> Result<(), MountError> {
    match stable {
        Some(stable) => validate_dynamic_layout(outer_start, outer_end, &[stable]),
        None => validate_dynamic_layout(outer_start, outer_end, &[]),
    }
}

fn snapshot_boundary_range(
    start: &web_sys::Comment,
    end: &web_sys::Comment,
    label: &'static str,
) -> Result<(web_sys::Node, Vec<web_sys::Node>), MountError> {
    let parent = validate_boundary_range(start, end, label)?;
    let mut nodes = Vec::new();
    let mut cursor: web_sys::Node = start.clone().into();
    loop {
        let is_end = cursor.is_same_node(Some(end.as_ref()));
        nodes.push(cursor);
        if is_end {
            return Ok((parent, nodes));
        }
        cursor = nodes
            .last()
            .and_then(web_sys::Node::next_sibling)
            .ok_or_else(|| MountError::Structure {
                violation: MountStructureViolation::BoundaryEndNotReachable,
                subject: MountDiagnostic::new(label),
            })?;
    }
}

fn validate_boundary_range(
    start: &web_sys::Comment,
    end: &web_sys::Comment,
    label: &'static str,
) -> Result<web_sys::Node, MountError> {
    let subject = MountDiagnostic::new(label);
    let Some(parent) = start.parent_node() else {
        return Err(MountError::Structure {
            violation: if end.parent_node().is_some() {
                MountStructureViolation::BoundaryParentsDiffer
            } else {
                MountStructureViolation::BoundaryDetached
            },
            subject,
        });
    };
    if !end
        .parent_node()
        .is_some_and(|candidate| candidate.is_same_node(Some(&parent)))
    {
        return Err(MountError::Structure {
            violation: MountStructureViolation::BoundaryParentsDiffer,
            subject,
        });
    }

    let mut cursor: web_sys::Node = start.clone().into();
    loop {
        if cursor.is_same_node(Some(end.as_ref())) {
            return Ok(parent);
        }
        cursor = cursor.next_sibling().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryEndNotReachable,
            subject: MountDiagnostic::new(label),
        })?;
    }
}

fn node_is_within(node: &web_sys::Node, ancestor: &web_sys::Node) -> bool {
    let mut cursor = Some(node.clone());
    while let Some(current) = cursor {
        if current.is_same_node(Some(ancestor)) {
            return true;
        }
        cursor = current.parent_node();
    }
    false
}

fn nodes_are_covered_by_range(nodes: &[web_sys::Node], range: &DomRange) -> bool {
    nodes_are_covered_by_topology(nodes, &range.owned_top_level)
}

fn nodes_are_covered_by_topology(
    nodes: &[web_sys::Node],
    owned_top_level: &[web_sys::Node],
) -> bool {
    nodes.iter().all(|node| {
        owned_top_level
            .iter()
            .any(|owned| node_is_within(node, owned))
    })
}

struct MountCleanup {
    range: RefCell<Option<DomRange>>,
    owned_nodes: RefCell<Vec<web_sys::Node>>,
    ancestors: Vec<Weak<MountCleanup>>,
    errors: ErrorSlot,
    retiring: Cell<bool>,
    cleaned: Cell<bool>,
    cleanup_complete: Cell<bool>,
    report_ownership_mismatch: Cell<bool>,
    #[cfg(test)]
    node_guards: RefCell<Vec<TestResourceGuard>>,
    #[cfg(test)]
    scope_guard: RefCell<Option<TestResourceGuard>>,
}

impl MountCleanup {
    fn new(errors: ErrorSlot, ancestors: Vec<Weak<MountCleanup>>) -> Self {
        Self {
            range: RefCell::new(None),
            owned_nodes: RefCell::new(Vec::new()),
            ancestors,
            errors,
            retiring: Cell::new(false),
            cleaned: Cell::new(false),
            cleanup_complete: Cell::new(false),
            report_ownership_mismatch: Cell::new(true),
            #[cfg(test)]
            node_guards: RefCell::new(Vec::new()),
            #[cfg(test)]
            scope_guard: RefCell::new(Some(TestResourceGuard::new(TestResourceKind::Scope))),
        }
    }

    fn begin_retiring(&self) {
        self.retiring.set(true);
    }

    fn suppress_ownership_mismatch(&self) {
        self.report_ownership_mismatch.set(false);
    }

    fn child_ancestry(self: &Rc<Self>) -> Vec<Weak<Self>> {
        let mut ancestors = Vec::with_capacity(self.ancestors.len() + 1);
        ancestors.push(Rc::downgrade(self));
        ancestors.extend(self.ancestors.iter().cloned());
        ancestors
    }

    fn live_chain(self: &Rc<Self>) -> Result<Vec<Rc<Self>>, MountError> {
        let mut chain = Vec::with_capacity(self.ancestors.len() + 1);
        chain.push(Rc::clone(self));
        for ancestor in &self.ancestors {
            chain.push(ancestor.upgrade().ok_or(MountError::Reactive {
                operation: MountOperation::RegisterCleanup,
                source: OwnerError::Disposed,
            })?);
        }
        Ok(chain)
    }

    fn active_chain(
        self: &Rc<Self>,
        operation: MountOperation,
    ) -> Result<Vec<Rc<Self>>, MountError> {
        let chain = self.live_chain()?;
        for cleanup in &chain {
            cleanup.ensure_active(operation)?;
        }
        Ok(chain)
    }

    fn ensure_active(&self, operation: MountOperation) -> Result<(), MountError> {
        if self.retiring.get() || self.cleaned.get() {
            return Err(MountError::Reactive {
                operation,
                source: OwnerError::Disposed,
            });
        }
        Ok(())
    }

    fn range_snapshot(&self) -> Option<OwnedRangeSnapshot> {
        self.range.borrow().as_ref().map(DomRange::snapshot)
    }

    fn validate_exact(&self) -> Result<(), MountError> {
        let range = self.range.borrow();
        let range = range.as_ref().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        range.validate_exact()?;
        self.validate_registered_nodes(range)
    }

    fn validate_if_attached(&self) -> Result<(), MountError> {
        let range = self.range.borrow();
        match range.as_ref() {
            Some(range) => {
                range.validate_exact()?;
                self.validate_registered_nodes(range)
            }
            None => Ok(()),
        }
    }

    fn validate_registered_nodes(&self, range: &DomRange) -> Result<(), MountError> {
        if nodes_are_covered_by_range(&self.owned_nodes.borrow(), range) {
            Ok(())
        } else {
            Err(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new("registered mount nodes"),
            })
        }
    }

    fn validate_owned_nodes_present(&self) -> Result<(), MountError> {
        let range = self.range.borrow();
        let range = range.as_ref().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        range.validate_owned_nodes_present()?;
        self.validate_registered_nodes(range)
    }

    fn plan_dynamic_replacement(
        &self,
        outer_start: &web_sys::Comment,
        outer_end: &web_sys::Comment,
        candidate: &OwnedRangeSnapshot,
    ) -> Result<Option<DynamicTopologyPlan>, MountError> {
        let range = self.range.borrow();
        let Some(range) = range.as_ref() else {
            // Initial dynamic evaluation happens while its parent stage is
            // detached and before the parent range snapshot exists.
            return Ok(None);
        };
        range.validate_exact()?;
        let start_index = range
            .owned_top_level
            .iter()
            .position(|node| node.is_same_node(Some(outer_start.as_ref())));
        let end_index = range
            .owned_top_level
            .iter()
            .position(|node| node.is_same_node(Some(outer_end.as_ref())));
        let (start_index, end_index) = match (start_index, end_index) {
            (Some(start_index), Some(end_index)) => (start_index, end_index),
            (None, None) => {
                // The dynamic range is nested below one of the parent's
                // top-level owned elements, so this flat snapshot is stable.
                return Ok(None);
            }
            _ => {
                return Err(MountError::Structure {
                    violation: MountStructureViolation::BoundaryOwnershipMismatch,
                    subject: MountDiagnostic::new("parent dynamic slot"),
                });
            }
        };
        if start_index >= end_index {
            return Err(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new("parent dynamic slot"),
            });
        }

        let mut before_retire = range.owned_top_level.clone();
        before_retire.splice(end_index..end_index, candidate.nodes.iter().cloned());

        let mut after_retire = Vec::with_capacity(
            range.owned_top_level.len() - (end_index - start_index - 1) + candidate.nodes.len(),
        );
        after_retire.extend_from_slice(&range.owned_top_level[..=start_index]);
        after_retire.extend(candidate.nodes.iter().cloned());
        after_retire.extend_from_slice(&range.owned_top_level[end_index..]);
        Ok(Some(DynamicTopologyPlan {
            before_retire,
            after_retire,
        }))
    }

    fn validate_expected(&self, expected: &[web_sys::Node]) -> Result<(), MountError> {
        let range = self.range.borrow();
        let range = range.as_ref().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        range.validate_nodes(expected)?;
        if nodes_are_covered_by_topology(&self.owned_nodes.borrow(), expected) {
            Ok(())
        } else {
            Err(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new("registered mount nodes"),
            })
        }
    }

    fn commit_expected(&self, expected: Vec<web_sys::Node>) -> Result<(), MountError> {
        let mut range = self.range.borrow_mut();
        let range = range.as_mut().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        range.validate_nodes(&expected)?;
        if !nodes_are_covered_by_topology(&self.owned_nodes.borrow(), &expected) {
            return Err(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new("registered mount nodes"),
            });
        }
        range.replace_nodes(expected);
        Ok(())
    }

    fn commit_expected_unchecked(&self, expected: Vec<web_sys::Node>) -> Result<(), MountError> {
        let mut range = self.range.borrow_mut();
        let range = range.as_mut().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        range.replace_nodes(expected);
        Ok(())
    }

    fn forget_descendant_range(&self, descendant: &OwnedRangeSnapshot) {
        if descendant.nodes.is_empty() {
            return;
        }
        let mut range = self.range.borrow_mut();
        let Some(range) = range.as_mut() else {
            return;
        };
        let Some(start) = range
            .owned_top_level
            .windows(descendant.nodes.len())
            .position(|candidate| {
                candidate
                    .iter()
                    .zip(&descendant.nodes)
                    .all(|(left, right)| left.is_same_node(Some(right)))
            })
        else {
            return;
        };
        range
            .owned_top_level
            .drain(start..start + descendant.nodes.len());
    }

    fn forget_dynamic_slot_topology(
        &self,
        outer_start: &web_sys::Comment,
        outer_end: &web_sys::Comment,
    ) {
        if let Some(range) = self.range.borrow_mut().as_mut() {
            let start = range
                .owned_top_level
                .iter()
                .position(|node| node.is_same_node(Some(outer_start.as_ref())));
            let end = range
                .owned_top_level
                .iter()
                .position(|node| node.is_same_node(Some(outer_end.as_ref())));
            match (start, end) {
                (Some(start), Some(end)) if start <= end => {
                    range.owned_top_level.drain(start..=end);
                }
                _ => {}
            }
        }
    }

    fn forget_dynamic_boundary_registry(
        &self,
        outer_start: &web_sys::Comment,
        outer_end: &web_sys::Comment,
    ) {
        self.owned_nodes.borrow_mut().retain(|node| {
            !node.is_same_node(Some(outer_start.as_ref()))
                && !node.is_same_node(Some(outer_end.as_ref()))
        });
    }

    fn forget_from_retiring_ancestors(&self, descendant: &OwnedRangeSnapshot) {
        for ancestor in &self.ancestors {
            if let Some(ancestor) = ancestor.upgrade() {
                if ancestor.retiring.get() {
                    ancestor.forget_descendant_range(descendant);
                }
            }
        }
    }

    fn covers_cleanup(&self, descendant: &DomRange, owned_nodes: &[web_sys::Node]) -> bool {
        let range = self.range.borrow();
        let Some(range) = range.as_ref() else {
            return false;
        };
        if range.validate_exact().is_err() {
            return false;
        }

        let descendant_len = descendant.owned_top_level.len();
        let flattened = descendant
            .owned_top_level
            .first()
            .and_then(|first| {
                range
                    .owned_top_level
                    .iter()
                    .position(|node| node.is_same_node(Some(first)))
            })
            .and_then(|start| {
                start
                    .checked_add(descendant_len)
                    .and_then(|end| range.owned_top_level.get(start..end))
            })
            .is_some_and(|candidate| {
                candidate
                    .iter()
                    .zip(&descendant.owned_top_level)
                    .all(|(left, right)| left.is_same_node(Some(right)))
            });
        let descendant_start: web_sys::Node = descendant.start.clone().into();
        let descendant_end: web_sys::Node = descendant.end.clone().into();
        let range_covered = flattened
            || range.owned_top_level.iter().any(|owned| {
                node_is_within(&descendant_start, owned) && node_is_within(&descendant_end, owned)
            });
        range_covered && nodes_are_covered_by_range(owned_nodes, range)
    }

    fn registered_nodes_are_covered(&self) -> bool {
        let range = self.range.borrow();
        let Some(range) = range.as_ref() else {
            return true;
        };
        range.validate_exact().is_ok()
            && nodes_are_covered_by_range(&self.owned_nodes.borrow(), range)
    }

    fn delegated_dom_cleanup_target(&self) -> Option<Rc<Self>> {
        let range = self.range.borrow();
        let Some(range) = range.as_ref() else {
            // A partially-built scope has no complete ownership range to
            // delegate. Its exact node registry remains responsible for any
            // nodes user code may have moved into a connected document.
            return None;
        };
        if range.validate_exact().is_err() {
            return None;
        }

        for ancestor in &self.ancestors {
            let Some(ancestor) = ancestor.upgrade() else {
                continue;
            };
            if ancestor.retiring.get() {
                // Only the nearest retiring owner may absorb this cleanup. If
                // its topology no longer covers the child, cleaning locally is
                // required even when a farther ancestor still happens to fit.
                if ancestor.range.borrow().is_none() {
                    return None;
                }
                let owned_nodes = self.owned_nodes.borrow();
                let covered = ancestor.covers_cleanup(range, &owned_nodes);
                if !covered && self.report_ownership_mismatch.get() {
                    self.errors.record(MountError::Structure {
                        violation: MountStructureViolation::BoundaryOwnershipMismatch,
                        subject: MountDiagnostic::new("retiring ancestor coverage"),
                    });
                }
                return covered.then_some(ancestor);
            }
        }
        None
    }

    fn transfer_registered_nodes_to(&self, target: &Self) {
        let transferred = std::mem::take(&mut *self.owned_nodes.borrow_mut());
        merge_unique_nodes(&mut target.owned_nodes.borrow_mut(), transferred);
        #[cfg(test)]
        target
            .node_guards
            .borrow_mut()
            .append(&mut self.node_guards.borrow_mut());
    }

    fn transfer_survivors_to_active_ancestor(&self) -> bool {
        for ancestor in &self.ancestors {
            let Some(ancestor) = ancestor.upgrade() else {
                continue;
            };
            if ancestor.cleanup_complete.get() {
                continue;
            }
            self.transfer_registered_nodes_to(&ancestor);
            return true;
        }
        false
    }

    fn cleanup(&self) {
        if self.cleaned.replace(true) {
            return;
        }

        let registered_nodes_are_covered = self.registered_nodes_are_covered();
        if !registered_nodes_are_covered && self.report_ownership_mismatch.get() {
            self.errors.record(MountError::Structure {
                violation: MountStructureViolation::BoundaryOwnershipMismatch,
                subject: MountDiagnostic::new("registered mount nodes"),
            });
        }

        if let Some(target) = registered_nodes_are_covered
            .then(|| self.delegated_dom_cleanup_target())
            .flatten()
        {
            // The ancestor owns the same flattened topology, but it must also
            // inherit the exact node registry. Its final drain catches nodes
            // moved or reinserted by disconnectedCallback reactions.
            self.range.borrow_mut().take();
            self.transfer_registered_nodes_to(&target);
        } else {
            let range = self.range.borrow_mut().take();
            let (mut cleanup_retried, mut had_specific_error) = match range {
                Some(range) => {
                    self.forget_from_retiring_ancestors(&range.snapshot());
                    let outcome = range.remove(&self.errors, self.report_ownership_mismatch.get());
                    (outcome.reinserted, outcome.had_specific_error)
                }
                None => (false, false),
            };
            let mut passes = 0;
            let mut owned_nodes = std::mem::take(&mut *self.owned_nodes.borrow_mut());
            cleanup_retried |= owned_nodes.iter().any(web_sys::Node::is_connected);
            for pass in 0..MAX_CLEANUP_DRAIN_PASSES {
                passes = pass + 1;
                merge_unique_nodes(
                    &mut owned_nodes,
                    std::mem::take(&mut *self.owned_nodes.borrow_mut()),
                );
                for node in owned_nodes.iter().rev() {
                    let Some(parent) = node.parent_node() else {
                        continue;
                    };
                    if let Err(error) = remove_child(&parent, node, "owned mount node") {
                        self.errors.record(error);
                        had_specific_error = true;
                    }
                }
                merge_unique_nodes(
                    &mut owned_nodes,
                    std::mem::take(&mut *self.owned_nodes.borrow_mut()),
                );
                if owned_nodes.iter().all(|node| node.parent_node().is_none()) {
                    break;
                }
                cleanup_retried = true;
            }
            let survivors: Vec<_> = owned_nodes
                .into_iter()
                .filter(|node| node.parent_node().is_some())
                .collect();
            if survivors.is_empty() {
                if cleanup_retried && !had_specific_error && self.report_ownership_mismatch.get() {
                    self.errors
                        .record(ownership_mismatch("cleanup reinserted owned nodes"));
                }
            } else {
                let remaining_owned_nodes = survivors.len();
                *self.owned_nodes.borrow_mut() = survivors;
                // A child scope can fail to delegate before cleanup because an
                // owned node was already moved outside its range. Keep those
                // exact identities alive in the nearest ancestor that has not
                // completed cleanup; otherwise dropping this lease would turn
                // a diagnosed survivor into an unowned DOM leak.
                self.transfer_survivors_to_active_ancestor();
                self.errors
                    .record_terminal(MountError::CleanupDidNotConverge {
                        subject: MountDiagnostic::new("owned mount nodes"),
                        remaining_owned_nodes,
                        passes,
                    });
            }
        }
        #[cfg(test)]
        {
            if self.owned_nodes.borrow().is_empty() {
                self.node_guards.borrow_mut().clear();
            }
            self.scope_guard.borrow_mut().take();
        }
        self.cleanup_complete.set(true);
    }
}

fn merge_unique_nodes(target: &mut Vec<web_sys::Node>, nodes: Vec<web_sys::Node>) {
    for node in nodes {
        if !target.iter().any(|owned| owned.is_same_node(Some(&node))) {
            target.push(node);
        }
    }
}

struct DynamicTopologyPlan {
    before_retire: Vec<web_sys::Node>,
    after_retire: Vec<web_sys::Node>,
}

struct DynamicTopologyEntry {
    cleanup: Rc<MountCleanup>,
    plan: Option<DynamicTopologyPlan>,
}

struct DynamicTopologyTransaction {
    entries: Vec<DynamicTopologyEntry>,
}

impl DynamicTopologyTransaction {
    fn plan(
        chain: Vec<Rc<MountCleanup>>,
        outer_start: &web_sys::Comment,
        outer_end: &web_sys::Comment,
        candidate: &OwnedRangeSnapshot,
    ) -> Result<Self, MountError> {
        let mut entries = Vec::with_capacity(chain.len());
        for cleanup in chain {
            let plan = cleanup.plan_dynamic_replacement(outer_start, outer_end, candidate)?;
            entries.push(DynamicTopologyEntry { cleanup, plan });
        }
        Ok(Self { entries })
    }

    fn ensure_active(&self, operation: MountOperation) -> Result<(), MountError> {
        for entry in &self.entries {
            entry.cleanup.ensure_active(operation)?;
        }
        Ok(())
    }

    fn validate_before_retire(&self) -> Result<(), MountError> {
        for entry in &self.entries {
            match entry.plan.as_ref() {
                Some(plan) => entry.cleanup.validate_expected(&plan.before_retire)?,
                None => entry.cleanup.validate_if_attached()?,
            }
        }
        Ok(())
    }

    fn commit_after_retire(self) -> Result<(), MountError> {
        let validation = self
            .entries
            .iter()
            .try_for_each(|entry| match entry.plan.as_ref() {
                Some(plan) => entry.cleanup.validate_expected(&plan.after_retire),
                None => entry.cleanup.validate_if_attached(),
            });
        if let Err(error) = validation {
            for entry in self.entries {
                if let Some(plan) = entry.plan {
                    let _ = entry.cleanup.commit_expected_unchecked(plan.after_retire);
                }
            }
            return Err(error);
        }
        for entry in self.entries {
            if let Some(plan) = entry.plan {
                entry.cleanup.commit_expected(plan.after_retire)?;
            }
        }
        Ok(())
    }

    fn commit_after_retire_unchecked(self) -> Result<(), MountError> {
        for entry in self.entries {
            if let Some(plan) = entry.plan {
                entry.cleanup.commit_expected_unchecked(plan.after_retire)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct MountCleanupLease(Rc<MountCleanup>);

impl MountCleanupLease {
    fn new(errors: ErrorSlot, ancestors: Vec<Weak<MountCleanup>>) -> Self {
        Self(Rc::new(MountCleanup::new(errors, ancestors)))
    }

    fn downgrade(&self) -> Weak<MountCleanup> {
        Rc::downgrade(&self.0)
    }
}

impl Drop for MountCleanupLease {
    fn drop(&mut self) {
        if Rc::strong_count(&self.0) == 1 {
            self.0.cleanup();
        }
    }
}

/// Lifecycle owner for one mounted DOM range.
///
/// Effects and listener cleanups are disposed before its DOM range is removed.
/// Repeated disposal is a no-op. The owner is deliberately not cloneable: an
/// effect owned by this scope must never capture the scope itself.
pub struct MountScope {
    owner: RefCell<Option<Owner>>,
    cleanup: RefCell<Option<MountCleanupLease>>,
    errors: ErrorSlot,
    disposed: Cell<bool>,
}

impl MountScope {
    /// Create an empty lifecycle scope. Root and dynamic mounts attach a range
    /// internally before returning it to the caller.
    #[must_use]
    pub fn new() -> Self {
        Self::with_errors(ErrorSlot::default())
    }

    fn with_errors(errors: ErrorSlot) -> Self {
        Self::with_errors_and_ancestors(errors, Vec::new())
    }

    fn with_errors_and_ancestors(errors: ErrorSlot, ancestors: Vec<Weak<MountCleanup>>) -> Self {
        let owner = Owner::new();
        let cleanup = MountCleanupLease::new(errors.clone(), ancestors);
        let deferred_cleanup = cleanup.clone();
        // Created first, this owner sentinel is retired after all subsequently
        // owned render effects. Its lease therefore drops only after dynamic
        // candidate scopes and listener cleanups, including deferred disposal
        // requested from inside a running effect.
        let _sentinel = owner.effect(move || {
            let _keep_cleanup_live = &deferred_cleanup;
        });
        Self {
            owner: RefCell::new(Some(owner)),
            cleanup: RefCell::new(Some(cleanup)),
            errors,
            disposed: Cell::new(false),
        }
    }

    fn attach_range(&self, range: DomRange) -> Result<(), MountError> {
        let cleanup = self.cleanup.borrow();
        let cleanup = cleanup.as_ref().ok_or(MountError::Reactive {
            operation: MountOperation::RegisterCleanup,
            source: OwnerError::Disposed,
        })?;
        debug_assert!(cleanup.0.range.borrow().is_none());
        *cleanup.0.range.borrow_mut() = Some(range);
        Ok(())
    }

    fn owner_operation<T>(
        &self,
        operation: MountOperation,
        f: impl FnOnce(&Owner) -> Result<T, OwnerError>,
    ) -> Result<T, MountError> {
        let owner = self.owner.borrow();
        let owner = owner.as_ref().ok_or(MountError::Reactive {
            operation,
            source: OwnerError::Disposed,
        })?;
        f(owner).map_err(|source| MountError::Reactive { operation, source })
    }

    fn register_node(&self, node: &web_sys::Node) -> Result<(), MountError> {
        let cleanup = self.cleanup.borrow();
        let cleanup = cleanup.as_ref().ok_or(MountError::Reactive {
            operation: MountOperation::RegisterCleanup,
            source: OwnerError::Disposed,
        })?;
        cleanup.0.owned_nodes.borrow_mut().push(node.clone());
        #[cfg(test)]
        cleanup
            .0
            .node_guards
            .borrow_mut()
            .push(TestResourceGuard::new(TestResourceKind::Node));
        Ok(())
    }

    fn cleanup_weak(&self) -> Result<Weak<MountCleanup>, MountError> {
        let cleanup = self.cleanup.borrow();
        cleanup
            .as_ref()
            .map(MountCleanupLease::downgrade)
            .ok_or(MountError::Reactive {
                operation: MountOperation::RegisterCleanup,
                source: OwnerError::Disposed,
            })
    }

    fn owned_range_snapshot(&self) -> Result<OwnedRangeSnapshot, MountError> {
        let cleanup = self.cleanup.borrow();
        let cleanup = cleanup.as_ref().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        cleanup.0.validate_exact()?;
        cleanup
            .0
            .range_snapshot()
            .ok_or_else(|| MountError::Structure {
                violation: MountStructureViolation::BoundaryDetached,
                subject: MountDiagnostic::new("mount range"),
            })
    }

    fn validate_owned_range(&self) -> Result<(), MountError> {
        let cleanup = self.cleanup.borrow();
        let cleanup = cleanup.as_ref().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        cleanup.0.validate_exact()
    }

    fn validate_owned_nodes_present(&self) -> Result<(), MountError> {
        let cleanup = self.cleanup.borrow();
        let cleanup = cleanup.as_ref().ok_or_else(|| MountError::Structure {
            violation: MountStructureViolation::BoundaryDetached,
            subject: MountDiagnostic::new("mount range"),
        })?;
        cleanup.0.validate_owned_nodes_present()
    }

    fn suppress_cleanup_ownership_mismatch(&self) {
        if let Some(cleanup) = self.cleanup.borrow().as_ref() {
            cleanup.0.suppress_ownership_mismatch();
        }
    }

    /// Dispose all reactive resources/listeners and then remove the owned range.
    /// The order is intentional and is safe to call repeatedly.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }

        if let Some(cleanup) = self.cleanup.borrow().as_ref() {
            cleanup.0.begin_retiring();
        }

        let owner_panic = self.owner.borrow_mut().take().and_then(|owner| {
            let result = catch_unwind(AssertUnwindSafe(|| owner.dispose())).err();
            drop(owner);
            result
        });
        self.cleanup.borrow_mut().take();

        // Stable wasm32 builds use panic=abort, so a Rust panic remains outside
        // the recoverable DOM error contract. On unwind-enabled builds, still
        // finish best-effort DOM cleanup before preserving the primary panic.
        if let Some(payload) = owner_panic {
            if !std::thread::panicking() {
                resume_unwind(payload);
            }
        }
    }

    #[must_use]
    pub fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    /// Most recent asynchronous patch/cleanup failure, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<MountError> {
        self.errors.last()
    }

    /// Take and clear the most recent asynchronous patch/cleanup failure.
    pub fn take_error(&self) -> Option<MountError> {
        self.errors.take()
    }
}

impl Default for MountScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MountScope {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Handle that keeps a mounted root and all of its resources alive.
///
/// Dropping this value disposes the reactive owner, removes listeners with the
/// exact callbacks used to register them, and finally removes the DOM range.
pub struct MountedRoot {
    scope: MountScope,
}

impl MountedRoot {
    #[must_use]
    pub fn scope(&self) -> &MountScope {
        &self.scope
    }

    pub fn dispose(&self) {
        self.scope.dispose();
    }

    #[must_use]
    pub fn is_disposed(&self) -> bool {
        self.scope.is_disposed()
    }

    #[must_use]
    pub fn last_error(&self) -> Option<MountError> {
        self.scope.last_error()
    }

    pub fn take_error(&self) -> Option<MountError> {
        self.scope.take_error()
    }
}

struct StagedMount {
    fragment: web_sys::DocumentFragment,
    scope: MountScope,
}

#[derive(Clone, Copy)]
struct MountPosition<'a> {
    parent: &'a web_sys::Node,
    before: Option<&'a web_sys::Node>,
    inherited_namespace: ElementNamespace,
    parent_context: Option<&'a MountParentContext>,
}

/// Mount a view as a new sibling range under `parent`.
pub fn mount(view: &View, parent: &web_sys::Node) -> Result<MountedRoot, MountError> {
    let document = document_for(parent)?;
    let namespace = namespace_for_children(parent)?;
    let parent_context = mount_parent_context(parent)?;
    let errors = ErrorSlot::default();
    let staged = stage_view(
        &document,
        view,
        namespace,
        parent_context,
        false,
        Vec::new(),
        errors,
    )?;
    append_child(parent, staged.fragment.as_ref(), "mount root")?;
    // Connected custom-element reactions run during insertion and may move the
    // boundary markers. Never return a root whose ownership range is invalid.
    staged.scope.validate_owned_range()?;
    Ok(MountedRoot {
        scope: staged.scope,
    })
}

/// Mount into the element with `id`.
pub fn mount_to(id: &str, view: &View) -> Result<MountedRoot, MountError> {
    let document = browser_document()?;
    let host = document
        .get_element_by_id(id)
        .ok_or_else(|| MountError::HostNotFound {
            id: MountDiagnostic::new(id),
        })?;
    mount(view, host.as_ref())
}

/// Mount into the current document's `<body>`.
pub fn mount_to_body(view: &View) -> Result<MountedRoot, MountError> {
    let body = browser_document()?
        .body()
        .ok_or(MountError::BodyUnavailable)?;
    mount(view, body.as_ref())
}

fn browser_document() -> Result<web_sys::Document, MountError> {
    web_sys::window()
        .ok_or(MountError::WindowUnavailable)?
        .document()
        .ok_or(MountError::DocumentUnavailable)
}

fn document_for(parent: &web_sys::Node) -> Result<web_sys::Document, MountError> {
    parent.owner_document().map_or_else(browser_document, Ok)
}

fn namespace_for_children(parent: &web_sys::Node) -> Result<ElementNamespace, MountError> {
    let Some(element) = parent.dyn_ref::<web_sys::Element>() else {
        return Ok(ElementNamespace::Html);
    };
    existing_element_namespace(element)?;
    Ok(namespace_for_existing_element(
        element.namespace_uri().as_deref(),
        &element.local_name(),
    ))
}

fn mount_parent_context(parent: &web_sys::Node) -> Result<Option<MountParentContext>, MountError> {
    let Some(element) = parent.dyn_ref::<web_sys::Element>() else {
        return Ok(None);
    };

    let mut lineage = Vec::new();
    let mut cursor = Some(element.clone());
    while let Some(current) = cursor {
        cursor = current.parent_element();
        lineage.push(current);
    }
    lineage.reverse();

    let mut parser = ParserContext::default();
    let mut context = None;
    for current in lineage {
        let tag = TagName::new(current.local_name()).map_err(DomError::InvalidName)?;
        let namespace = existing_element_namespace(&current)?;
        let child_namespace = namespace_for_existing_element(
            current.namespace_uri().as_deref(),
            &current.local_name(),
        );
        parser = parser.descend(&tag, namespace, child_namespace);
        context = Some(MountParentContext {
            tag,
            namespace,
            parser,
        });
    }
    Ok(context)
}

fn existing_element_namespace(element: &web_sys::Element) -> Result<ElementNamespace, MountError> {
    match element.namespace_uri().as_deref() {
        Some(ElementNamespace::HTML_URI) => Ok(ElementNamespace::Html),
        Some(ElementNamespace::SVG_URI) => Ok(ElementNamespace::Svg),
        namespace => Err(MountError::UnsupportedNamespace {
            namespace: MountDiagnostic::new(namespace.unwrap_or("<null>")),
        }),
    }
}

fn namespace_for_existing_element(
    namespace_uri: Option<&str>,
    local_name: &str,
) -> ElementNamespace {
    if namespace_uri != Some(ElementNamespace::SVG_URI) {
        return ElementNamespace::Html;
    }
    if ["foreignObject", "desc", "title"]
        .iter()
        .any(|point| local_name.eq_ignore_ascii_case(point))
    {
        ElementNamespace::Html
    } else {
        ElementNamespace::Svg
    }
}

fn validate_mount_text(
    value: &str,
    parent: Option<&MountParentContext>,
    direct: &DirectChildState,
) -> Result<(), RenderError> {
    if let Some((index, character)) = value
        .char_indices()
        .find(|(_, character)| matches!(character, '\0' | '\r'))
    {
        return Err(RenderError::ParserNormalizedText { index, character });
    }
    validate_direct_text(
        parent.map(MountParentContext::render_context),
        value,
        direct,
    )
}

fn validate_mount_element(
    parent: Option<&MountParentContext>,
    tag: &TagName,
    namespace: ElementNamespace,
) -> Result<(), RenderError> {
    let parent = parent.map(MountParentContext::render_context);
    validate_parser_adjusted_svg_tag(parent, tag, namespace)?;
    validate_direct_element(parent, tag, namespace)
}

fn stage_view(
    document: &web_sys::Document,
    view: &View,
    namespace: ElementNamespace,
    parent_context: Option<MountParentContext>,
    guaranteed_prefix_content: bool,
    ancestors: Vec<Weak<MountCleanup>>,
    errors: ErrorSlot,
) -> Result<StagedMount, MountError> {
    let fragment = document.create_document_fragment();
    let start = document.create_comment("pliego:mount");
    let end = document.create_comment("/pliego:mount");
    let scope = MountScope::with_errors_and_ancestors(errors, ancestors);
    scope.register_node(start.as_ref())?;
    scope.register_node(end.as_ref())?;
    append_child(fragment.as_ref(), start.as_ref(), "mount start boundary")?;
    append_child(fragment.as_ref(), end.as_ref(), "mount end boundary")?;
    let mut direct = DirectChildState {
        has_serialized_content: guaranteed_prefix_content,
    };
    mount_before(
        document,
        view,
        MountPosition {
            parent: fragment.as_ref(),
            before: Some(end.as_ref()),
            inherited_namespace: namespace,
            parent_context: parent_context.as_ref(),
        },
        &mut direct,
        &scope,
    )?;
    let (_, owned_top_level) = snapshot_boundary_range(&start, &end, "mount range")?;
    scope.attach_range(DomRange {
        start: start.clone(),
        end: end.clone(),
        owned_top_level,
        label: "mount range",
    })?;
    Ok(StagedMount { fragment, scope })
}

fn mount_before(
    document: &web_sys::Document,
    view: &View,
    position: MountPosition<'_>,
    direct: &mut DirectChildState,
    scope: &MountScope,
) -> Result<(), MountError> {
    match view {
        View::Text(value) => {
            validate_mount_text(value, position.parent_context, direct)?;
            let text = document.create_text_node(value);
            scope.register_node(text.as_ref())?;
            insert_before(position.parent, text.as_ref(), position.before, "text node")?;
            if !value.is_empty() {
                direct.has_serialized_content = true;
            }
            Ok(())
        }
        View::DynText(value) => {
            let text = document.create_text_node("");
            scope.register_node(text.as_ref())?;
            insert_before(
                position.parent,
                text.as_ref(),
                position.before,
                "dynamic text node",
            )?;
            let value = Rc::clone(value);
            let parent_context = position.parent_context.cloned();
            let guaranteed_prefix_content = direct.has_serialized_content;
            let errors = scope.errors.clone();
            let cleanup = scope.cleanup_weak()?;
            let initial_error = Rc::new(RefCell::new(None));
            let initial_error_effect = Rc::clone(&initial_error);
            let first_run = Rc::new(Cell::new(true));
            let first_run_effect = Rc::clone(&first_run);
            #[cfg(test)]
            let effect_guard = TestResourceGuard::new(TestResourceKind::Effect);
            scope.owner_operation(MountOperation::InstallEffect, move |owner| {
                owner.effect(move || {
                    #[cfg(test)]
                    let _keep_effect_live = &effect_guard;
                    let is_initial = first_run_effect.replace(false);
                    let result = (|| {
                        let cleanup = cleanup.upgrade().ok_or(MountError::Reactive {
                            operation: MountOperation::InstallEffect,
                            source: OwnerError::Disposed,
                        })?;
                        let chain = cleanup.active_chain(MountOperation::InstallEffect)?;
                        let candidate = value();
                        for cleanup in &chain {
                            cleanup.ensure_active(MountOperation::InstallEffect)?;
                        }
                        let candidate_direct = DirectChildState {
                            has_serialized_content: guaranteed_prefix_content,
                        };
                        validate_mount_text(
                            &candidate,
                            parent_context.as_ref(),
                            &candidate_direct,
                        )?;
                        for cleanup in &chain {
                            cleanup.ensure_active(MountOperation::InstallEffect)?;
                        }
                        // Text::set_data is non-throwing. Validation and the
                        // active-owner check both precede the write.
                        text.set_data(&candidate);
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
        View::Fragment(children) => {
            for child in children {
                mount_before(document, child, position, direct, scope)?;
            }
            Ok(())
        }
        View::Element(element) => mount_element(document, element, position, direct, scope),
        View::DynView(view) => {
            mount_dynamic_view(document, Rc::clone(view), position, direct, scope)
        }
    }
}

fn mount_element(
    document: &web_sys::Document,
    element: &Element,
    position: MountPosition<'_>,
    direct: &mut DirectChildState,
    scope: &MountScope,
) -> Result<(), MountError> {
    let namespace = position.inherited_namespace.for_element(&element.tag);
    validate_mount_element(position.parent_context, &element.tag, namespace)?;
    if element.tag.is_void_in(namespace) && !element.children.is_empty() {
        return Err(MountError::Structure {
            violation: MountStructureViolation::VoidElementHasChildren,
            subject: MountDiagnostic::new(element.tag.as_str()),
        });
    }
    direct.has_serialized_content = true;

    fail_if_injected(MountOperation::CreateElement, element.tag.as_str())?;
    let created = match namespace {
        // `create_element` applies the HTML document's ASCII case and custom
        // element semantics. SVG must always use its explicit namespace.
        ElementNamespace::Html => document.create_element(element.tag.as_str()),
        ElementNamespace::Svg => {
            document.create_element_ns(Some(namespace.uri()), element.tag.as_str())
        }
    };
    let dom_element = created
        .map_err(|error| dom_error(MountOperation::CreateElement, element.tag.as_str(), error))?;
    scope.register_node(dom_element.as_ref())?;

    for (name, value) in &element.attrs {
        match value {
            AttrValue::Static(value) => {
                validate_attribute_value(name, value)?;
                validate_parser_adjusted_svg_attribute(&element.tag, name, namespace)?;
                set_attribute(&dom_element, namespace, name.as_str(), value)?;
            }
            AttrValue::Dyn(value) => {
                let dom_element = dom_element.clone();
                let element_tag = element.tag.clone();
                let name = name.clone();
                let value = Rc::clone(value);
                let errors = scope.errors.clone();
                let cleanup = scope.cleanup_weak()?;
                let initial_error = Rc::new(RefCell::new(None));
                let initial_error_effect = Rc::clone(&initial_error);
                let first_run = Rc::new(Cell::new(true));
                let first_run_effect = Rc::clone(&first_run);
                #[cfg(test)]
                let effect_guard = TestResourceGuard::new(TestResourceKind::Effect);
                scope.owner_operation(MountOperation::InstallEffect, move |owner| {
                    owner.effect(move || {
                        #[cfg(test)]
                        let _keep_effect_live = &effect_guard;
                        let is_initial = first_run_effect.replace(false);
                        let result = (|| {
                            let cleanup = cleanup.upgrade().ok_or(MountError::Reactive {
                                operation: MountOperation::InstallEffect,
                                source: OwnerError::Disposed,
                            })?;
                            let chain = cleanup.active_chain(MountOperation::SetAttribute)?;
                            let candidate = value();
                            for cleanup in &chain {
                                cleanup.ensure_active(MountOperation::SetAttribute)?;
                            }
                            let candidate = candidate.map_err(MountError::InvalidView)?;
                            validate_attribute_value(&name, &candidate)?;
                            validate_parser_adjusted_svg_attribute(&element_tag, &name, namespace)?;
                            for cleanup in &chain {
                                cleanup.ensure_active(MountOperation::SetAttribute)?;
                            }
                            let write =
                                set_attribute(&dom_element, namespace, name.as_str(), &candidate);
                            for cleanup in &chain {
                                cleanup.ensure_active(MountOperation::SetAttribute)?;
                            }
                            write
                        })();
                        if let Err(error) = result {
                            // The browser retains the previous attribute value.
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
            }
        }
    }

    for (event, handler) in &element.listeners {
        install_listener(scope, &dom_element, event.as_str(), Rc::clone(handler))?;
    }

    if !element.tag.is_void_in(namespace) {
        let child_namespace = namespace.for_children(&element.tag);
        let parser = position
            .parent_context
            .map(|context| context.parser)
            .unwrap_or_default()
            .descend(&element.tag, namespace, child_namespace);
        let child_context = MountParentContext {
            tag: element.tag.clone(),
            namespace,
            parser,
        };
        let mut child_direct = DirectChildState::default();
        for child in &element.children {
            mount_before(
                document,
                child,
                MountPosition {
                    parent: dom_element.as_ref(),
                    before: None,
                    inherited_namespace: child_namespace,
                    parent_context: Some(&child_context),
                },
                &mut child_direct,
                scope,
            )?;
        }
    }

    insert_before(
        position.parent,
        dom_element.as_ref(),
        position.before,
        element.tag.as_str(),
    )
}

fn install_listener(
    scope: &MountScope,
    element: &web_sys::Element,
    event: &str,
    handler: Rc<dyn Fn(web_sys::Event)>,
) -> Result<(), MountError> {
    let callback = Rc::new(Closure::<dyn FnMut(web_sys::Event)>::new(move |event| {
        handler(event);
    }));
    let cleanup_element = element.clone();
    let cleanup_event = event.to_owned();
    let cleanup_callback = Rc::clone(&callback);
    let errors = scope.errors.clone();
    #[cfg(test)]
    let listener_guard = TestResourceGuard::new(TestResourceKind::Listener);

    // Register the removal before adding the listener. If the add operation
    // fails, scope rollback still owns and drops the callback safely.
    scope.owner_operation(MountOperation::RegisterCleanup, move |owner| {
        owner.on_cleanup(move || {
            #[cfg(test)]
            let _keep_listener_live = &listener_guard;
            if let Err(error) =
                remove_event_listener(&cleanup_element, &cleanup_event, cleanup_callback.as_ref())
            {
                errors.record(error);
            }
        })
    })?;

    fail_if_injected(MountOperation::AddEventListener, event)?;
    element
        .add_event_listener_with_callback(event, callback.as_ref().as_ref().unchecked_ref())
        .map_err(|error| dom_error(MountOperation::AddEventListener, event, error))
}

fn mount_dynamic_view(
    document: &web_sys::Document,
    view: Rc<dyn Fn() -> View>,
    position: MountPosition<'_>,
    direct: &mut DirectChildState,
    scope: &MountScope,
) -> Result<(), MountError> {
    let start = document.create_comment("pliego:dyn");
    let end = document.create_comment("/pliego:dyn");
    scope.register_node(start.as_ref())?;
    scope.register_node(end.as_ref())?;
    insert_before(
        position.parent,
        start.as_ref(),
        position.before,
        "dynamic start boundary",
    )?;
    insert_before(
        position.parent,
        end.as_ref(),
        position.before,
        "dynamic end boundary",
    )?;

    let state = Rc::new(RefCell::new(DynamicSlotState::new()));
    let parent_cleanup = scope.cleanup_weak()?;
    let document = document.clone();
    let errors = scope.errors.fork_origin();
    let namespace = position.inherited_namespace;
    let parent_context = position.parent_context.cloned();
    let guaranteed_prefix_content = direct.has_serialized_content;
    let initial_error = Rc::new(RefCell::new(None));
    let initial_error_effect = Rc::clone(&initial_error);
    let first_run = Rc::new(Cell::new(true));
    let first_run_effect = Rc::clone(&first_run);
    #[cfg(test)]
    let effect_guard = TestResourceGuard::new(TestResourceKind::Effect);
    scope.owner_operation(MountOperation::InstallEffect, move |owner| {
        owner.effect(move || {
            #[cfg(test)]
            let _keep_effect_live = &effect_guard;
            let is_initial = first_run_effect.replace(false);
            match state.borrow_mut().begin_update() {
                Ok(true) => {}
                Ok(false) => return,
                Err(error) => {
                    errors.record_terminal(error.clone());
                    if is_initial {
                        *initial_error_effect.borrow_mut() = Some(error);
                    }
                    return;
                }
            }

            let mut irreversible = false;
            let mut stable_corrupted = false;
            let replacement = (|| {
                let parent_cleanup = parent_cleanup.upgrade().ok_or(MountError::Reactive {
                    operation: MountOperation::RegisterCleanup,
                    source: OwnerError::Disposed,
                })?;
                let cleanup_chain = parent_cleanup.active_chain(MountOperation::InsertRange)?;
                state.borrow().ensure_updating()?;
                let mut stable_snapshot = match dynamic_stable_snapshot(&state) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        stable_corrupted = state
                            .borrow()
                            .validate_stable_owned_nodes_present()
                            .is_err();
                        return Err(error);
                    }
                };
                for cleanup in &cleanup_chain {
                    cleanup.validate_if_attached()?;
                }
                validate_dynamic_stable_layout(&start, &end, stable_snapshot.as_ref())?;

                let fresh = view();
                // User code can synchronously update nested scopes or mutate
                // the host. Refresh and validate the stable topology before
                // doing any candidate work.
                for cleanup in &cleanup_chain {
                    cleanup.ensure_active(MountOperation::InsertRange)?;
                }
                state.borrow().ensure_updating()?;
                stable_snapshot = match dynamic_stable_snapshot(&state) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        stable_corrupted = state
                            .borrow()
                            .validate_stable_owned_nodes_present()
                            .is_err();
                        return Err(error);
                    }
                };
                for cleanup in &cleanup_chain {
                    cleanup.validate_if_attached()?;
                }
                validate_dynamic_stable_layout(&start, &end, stable_snapshot.as_ref())?;

                let candidate = stage_view(
                    &document,
                    &fresh,
                    namespace,
                    parent_context.clone(),
                    guaranteed_prefix_content,
                    parent_cleanup.child_ancestry(),
                    errors.clone(),
                );
                for cleanup in &cleanup_chain {
                    cleanup.ensure_active(MountOperation::InsertRange)?;
                }
                state.borrow().ensure_updating()?;
                let candidate = candidate?;
                let candidate_snapshot = candidate.scope.owned_range_snapshot()?;

                // Candidate construction may run user closures. Revalidate the
                // host markers after that code and again after connected custom
                // element reactions from insertion, before retiring stable DOM.
                let parent = validate_boundary_range(&start, &end, "dynamic range")?;
                for cleanup in &cleanup_chain {
                    cleanup.ensure_active(MountOperation::InsertRange)?;
                }
                state.borrow().ensure_updating()?;
                stable_snapshot = match dynamic_stable_snapshot(&state) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        stable_corrupted = state
                            .borrow()
                            .validate_stable_owned_nodes_present()
                            .is_err();
                        return Err(error);
                    }
                };
                for cleanup in &cleanup_chain {
                    cleanup.validate_if_attached()?;
                }
                validate_dynamic_stable_layout(&start, &end, stable_snapshot.as_ref())?;
                let topology = DynamicTopologyTransaction::plan(
                    cleanup_chain,
                    &start,
                    &end,
                    &candidate_snapshot,
                )?;

                topology.ensure_active(MountOperation::InsertRange)?;
                state.borrow().ensure_updating()?;
                let insertion = insert_before(
                    &parent,
                    candidate.fragment.as_ref(),
                    Some(end.as_ref()),
                    "dynamic candidate",
                );
                topology.ensure_active(MountOperation::InsertRange)?;
                state.borrow().ensure_updating()?;
                insertion?;
                match stable_snapshot.as_ref() {
                    Some(stable_snapshot) => validate_dynamic_layout(
                        &start,
                        &end,
                        &[stable_snapshot, &candidate_snapshot],
                    )?,
                    None => {
                        validate_dynamic_layout(&start, &end, &[&candidate_snapshot])?;
                    }
                }
                candidate.scope.validate_owned_range()?;
                topology.validate_before_retire()?;
                topology.ensure_active(MountOperation::InsertRange)?;
                state.borrow().ensure_updating()?;

                // From this point onward the old scope may execute
                // disconnected callbacks while it retires. Any later failure
                // poisons the slot instead of pretending the update rolled back.
                let retirement_terminal_checkpoint = errors.terminal_checkpoint();
                let retired = state.borrow_mut().replace_candidate(candidate.scope)?;
                irreversible = true;
                drop(retired);

                let retirement_terminal =
                    errors.terminal_recorded_after(retirement_terminal_checkpoint);
                let post_retire = state
                    .borrow()
                    .validate_stable_ownership()
                    .and_then(|()| validate_dynamic_layout(&start, &end, &[&candidate_snapshot]));
                if let Err(error) = post_retire {
                    stable_corrupted = true;
                    let _ = topology.commit_after_retire_unchecked();
                    return Err(error);
                }
                if let Some(error) = retirement_terminal {
                    let _ = topology.commit_after_retire_unchecked();
                    return Err(error);
                }
                state.borrow().ensure_updating()?;
                topology.ensure_active(MountOperation::InsertRange)?;
                if let Err(error) = topology.commit_after_retire() {
                    stable_corrupted = true;
                    return Err(error);
                }
                state.borrow_mut().finish_update()?;
                Ok::<(), MountError>(())
            })();
            if let Err(error) = replacement {
                let error = if stable_corrupted {
                    poison_and_retire_corrupted_stable(
                        &state,
                        &parent_cleanup,
                        &start,
                        &end,
                        &errors,
                        error,
                    )
                } else {
                    error
                };
                if irreversible && has_structural_failure(&error) {
                    state.borrow().suppress_stable_ownership_mismatch();
                }
                let (error, terminal, newly_terminal) =
                    state.borrow_mut().fail_update(error, irreversible);
                if terminal && newly_terminal {
                    errors.record_terminal(error.clone());
                } else if !terminal {
                    errors.record(error.clone());
                }
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

fn attribute_namespace(element_namespace: ElementNamespace, name: &str) -> Option<&'static str> {
    if element_namespace != ElementNamespace::Svg {
        return None;
    }
    if name == "xmlns" || name.starts_with("xmlns:") {
        return Some(XMLNS_URI);
    }
    let (prefix, _) = name.split_once(':')?;
    if prefix == "xlink" {
        Some(XLINK_URI)
    } else if prefix == "xml" {
        Some(XML_URI)
    } else {
        None
    }
}

fn set_attribute(
    element: &web_sys::Element,
    element_namespace: ElementNamespace,
    name: &str,
    value: &str,
) -> Result<(), MountError> {
    fail_if_injected(MountOperation::SetAttribute, name)?;
    let result = match attribute_namespace(element_namespace, name) {
        Some(namespace) => element.set_attribute_ns(Some(namespace), name, value),
        None => element.set_attribute(name, value),
    };
    result.map_err(|error| dom_error(MountOperation::SetAttribute, name, error))
}

fn append_child(
    parent: &web_sys::Node,
    child: &web_sys::Node,
    subject: &str,
) -> Result<(), MountError> {
    fail_if_injected(MountOperation::AppendNode, subject)?;
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|error| dom_error(MountOperation::AppendNode, subject, error))
}

fn insert_before(
    parent: &web_sys::Node,
    child: &web_sys::Node,
    before: Option<&web_sys::Node>,
    subject: &str,
) -> Result<(), MountError> {
    fail_if_injected(MountOperation::InsertRange, subject)?;
    parent
        .insert_before(child, before)
        .map(|_| ())
        .map_err(|error| dom_error(MountOperation::InsertRange, subject, error))
}

fn remove_child(
    parent: &web_sys::Node,
    child: &web_sys::Node,
    subject: &str,
) -> Result<(), MountError> {
    fail_if_injected(MountOperation::RemoveNode, subject)?;
    parent
        .remove_child(child)
        .map(|_| ())
        .map_err(|error| dom_error(MountOperation::RemoveNode, subject, error))
}

fn remove_event_listener(
    element: &web_sys::Element,
    event: &str,
    callback: &Closure<dyn FnMut(web_sys::Event)>,
) -> Result<(), MountError> {
    // Fault injection must not itself leave JavaScript retaining a callback
    // whose Rust Closure is about to be dropped. Perform the real removal, then
    // surface the injected diagnostic.
    let injected = fail_if_injected(MountOperation::RemoveEventListener, event).err();
    element
        .remove_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
        .map_err(|error| dom_error(MountOperation::RemoveEventListener, event, error))?;
    injected.map_or(Ok(()), Err)
}

fn dom_error(operation: MountOperation, subject: &str, error: wasm_bindgen::JsValue) -> MountError {
    let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
    MountError::Dom {
        operation,
        subject: MountDiagnostic::new(subject),
        detail: MountDiagnostic::new(&detail),
    }
}

#[cfg(not(test))]
fn fail_if_injected(_operation: MountOperation, _subject: &str) -> Result<(), MountError> {
    Ok(())
}

#[cfg(test)]
fn fail_if_injected(operation: MountOperation, subject: &str) -> Result<(), MountError> {
    let injected = TEST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.fail_next == Some(operation) {
            state.fail_next = None;
            true
        } else {
            false
        }
    });
    if injected {
        return Err(MountError::Dom {
            operation,
            subject: MountDiagnostic::new(subject),
            detail: MountDiagnostic::new("injected mount failure"),
        });
    }
    Ok(())
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestResourceKind {
    Scope,
    Node,
    Effect,
    Listener,
}

#[cfg(test)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MountTestStats {
    pub scopes: usize,
    pub nodes: usize,
    pub effects: usize,
    pub listeners: usize,
    pub cleanup_events: usize,
    pub cleanup_trace: Vec<TestResourceKind>,
}

#[cfg(test)]
const MAX_TEST_CLEANUP_TRACE: usize = 4_096;

#[cfg(test)]
#[derive(Default)]
struct TestState {
    stats: MountTestStats,
    fail_next: Option<MountOperation>,
}

#[cfg(test)]
thread_local! {
    static TEST_STATE: RefCell<TestState> = RefCell::new(TestState::default());
}

#[cfg(test)]
struct TestResourceGuard {
    kind: TestResourceKind,
}

#[cfg(test)]
impl TestResourceGuard {
    fn new(kind: TestResourceKind) -> Self {
        TEST_STATE.with(|state| {
            let stats = &mut state.borrow_mut().stats;
            match kind {
                TestResourceKind::Scope => stats.scopes += 1,
                TestResourceKind::Node => stats.nodes += 1,
                TestResourceKind::Effect => stats.effects += 1,
                TestResourceKind::Listener => stats.listeners += 1,
            }
        });
        Self { kind }
    }
}

#[cfg(test)]
impl Drop for TestResourceGuard {
    fn drop(&mut self) {
        TEST_STATE.with(|state| {
            let stats = &mut state.borrow_mut().stats;
            let value = match self.kind {
                TestResourceKind::Scope => &mut stats.scopes,
                TestResourceKind::Node => &mut stats.nodes,
                TestResourceKind::Effect => &mut stats.effects,
                TestResourceKind::Listener => &mut stats.listeners,
            };
            assert!(*value > 0, "mount test counter underflow");
            *value -= 1;
            stats.cleanup_events += 1;
            if stats.cleanup_trace.len() < MAX_TEST_CLEANUP_TRACE {
                stats.cleanup_trace.push(self.kind);
            }
        });
    }
}

#[cfg(test)]
pub(crate) fn mount_test_stats() -> MountTestStats {
    TEST_STATE.with(|state| state.borrow().stats.clone())
}

#[cfg(test)]
pub(crate) fn mount_test_reset() {
    TEST_STATE.with(|state| {
        let mut state = state.borrow_mut();
        assert_eq!(
            (
                state.stats.scopes,
                state.stats.nodes,
                state.stats.effects,
                state.stats.listeners
            ),
            (0, 0, 0, 0),
            "cannot reset mount instrumentation while resources are live"
        );
        *state = TestState::default();
    });
}

#[cfg(test)]
pub(crate) fn mount_test_fail_next(operation: MountOperation) {
    TEST_STATE.with(|state| state.borrow_mut().fail_next = Some(operation));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_preview_is_bounded_on_a_character_boundary() {
        let input = format!("{}z", "é".repeat(MAX_MOUNT_DIAGNOSTIC_BYTES));
        let diagnostic = MountDiagnostic::new(&input);
        assert!(diagnostic.preview.len() <= MAX_MOUNT_DIAGNOSTIC_BYTES);
        assert!(
            diagnostic
                .preview
                .is_char_boundary(diagnostic.preview.len())
        );
        assert_eq!(diagnostic.input_bytes, input.len());
        assert!(diagnostic.preview_truncated);
    }

    #[test]
    fn injected_failure_is_exactly_once_and_operation_scoped() {
        mount_test_reset();
        mount_test_fail_next(MountOperation::SetAttribute);
        assert!(fail_if_injected(MountOperation::AppendNode, "node").is_ok());
        assert!(matches!(
            fail_if_injected(MountOperation::SetAttribute, "href"),
            Err(MountError::Dom {
                operation: MountOperation::SetAttribute,
                ..
            })
        ));
        assert!(fail_if_injected(MountOperation::SetAttribute, "href").is_ok());
    }

    #[test]
    fn empty_scope_disposal_is_idempotent_and_balances_instrumentation() {
        mount_test_reset();
        let scope = MountScope::new();
        assert_eq!(mount_test_stats().scopes, 1);

        scope.dispose();
        scope.dispose();

        let stats = mount_test_stats();
        assert_eq!(stats.scopes, 0);
        assert_eq!(stats.nodes, 0);
        assert_eq!(stats.effects, 0);
        assert_eq!(stats.listeners, 0);
        assert_eq!(stats.cleanup_events, 1);
        assert_eq!(stats.cleanup_trace, vec![TestResourceKind::Scope]);
    }

    #[test]
    fn existing_svg_hosts_apply_all_html_integration_points() {
        for local_name in ["foreignObject", "desc", "title"] {
            assert_eq!(
                namespace_for_existing_element(Some(ElementNamespace::SVG_URI), local_name),
                ElementNamespace::Html,
                "wrong child namespace for existing {local_name} host"
            );
        }
        assert_eq!(
            namespace_for_existing_element(Some(ElementNamespace::SVG_URI), "g"),
            ElementNamespace::Svg
        );
        assert_eq!(
            namespace_for_existing_element(Some(ElementNamespace::HTML_URI), "title"),
            ElementNamespace::Html
        );
    }

    #[test]
    fn parser_sensitive_parent_rules_are_shared_with_ssr() {
        let html_parent = |tag: &str| {
            let tag = TagName::new(tag).unwrap();
            MountParentContext {
                parser: ParserContext::default().descend(
                    &tag,
                    ElementNamespace::Html,
                    ElementNamespace::Html,
                ),
                tag,
                namespace: ElementNamespace::Html,
            }
        };
        let direct = DirectChildState::default();

        let paragraph = html_parent("p");
        assert!(matches!(
            validate_mount_element(
                Some(&paragraph),
                &TagName::new("div").unwrap(),
                ElementNamespace::Html,
            ),
            Err(RenderError::ParserRepair { .. })
        ));

        let table = html_parent("table");
        assert!(matches!(
            validate_mount_text("foster", Some(&table), &direct),
            Err(RenderError::ParserRepair { .. })
        ));

        let pre = html_parent("pre");
        assert!(matches!(
            validate_mount_text("\nfirst", Some(&pre), &direct),
            Err(RenderError::ParserRepair { .. })
        ));
        let with_prefix = DirectChildState {
            has_serialized_content: true,
        };
        assert!(validate_mount_text("\nnext", Some(&pre), &with_prefix).is_ok());
        assert!(matches!(
            validate_mount_text("bad\rtext", None, &direct),
            Err(RenderError::ParserNormalizedText { .. })
        ));

        let svg = MountParentContext {
            tag: TagName::new("svg").unwrap(),
            namespace: ElementNamespace::Svg,
            parser: ParserContext::default(),
        };
        assert!(matches!(
            validate_mount_element(
                Some(&svg),
                &TagName::new("div").unwrap(),
                ElementNamespace::Svg,
            ),
            Err(RenderError::ParserRepair { .. })
        ));
        assert!(matches!(
            validate_mount_element(
                Some(&svg),
                &TagName::new("lineargradient").unwrap(),
                ElementNamespace::Svg,
            ),
            Err(RenderError::ParserRepair { .. })
        ));
    }

    #[test]
    fn attribute_namespaces_match_foreign_content_adjustment() {
        assert_eq!(
            attribute_namespace(ElementNamespace::Svg, "xlink:href"),
            Some(XLINK_URI)
        );
        assert_eq!(
            attribute_namespace(ElementNamespace::Svg, "xml:space"),
            Some(XML_URI)
        );
        assert_eq!(
            attribute_namespace(ElementNamespace::Svg, "xmlns"),
            Some(XMLNS_URI)
        );
        assert_eq!(
            attribute_namespace(ElementNamespace::Svg, "xmlns:xlink"),
            Some(XMLNS_URI)
        );
        assert_eq!(
            attribute_namespace(ElementNamespace::Html, "xlink:href"),
            None
        );
    }
}
