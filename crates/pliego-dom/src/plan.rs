// SPDX-License-Identifier: AGPL-3.0-only

/// Identifier assigned by one mounted root in plan-observation order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomPlanId(u64);

impl DomPlanId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Stable identifier for one reactive target within a mounted root.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomTargetId(u64);

impl DomTargetId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Optional coordinator metadata attached without coupling the renderer to it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DomCommitContext {
    /// Caller-defined transaction sequence; `pliego-dom` does not own it.
    pub transaction_id: Option<u64>,
    /// Caller-defined algorithm-tagged state root, if a coordinator has one.
    pub state_root: Option<String>,
}

/// One inspectable operation about to be executed by the direct renderer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomOp {
    /// In-place update of one text target. Values are represented by byte counts,
    /// not retained content.
    SetText {
        previous_bytes: usize,
        next_bytes: usize,
    },
    /// In-place update of one validated attribute target.
    SetAttribute {
        name: String,
        had_previous: bool,
        next_bytes: usize,
    },
    /// Replacement of the owned range between one dynamic slot's boundaries.
    ReplaceSubtree { had_previous: bool },
    /// Keyed row reconciliation summarized without exposing raw node movement.
    ReconcileKeyed {
        previous_rows: usize,
        next_rows: usize,
        retained_rows: usize,
        moved_rows: usize,
        inserted_rows: usize,
        removed_rows: usize,
    },
}

/// Immutable description emitted after validation and before browser mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomPlan {
    pub id: DomPlanId,
    pub target: DomTargetId,
    pub context: DomCommitContext,
    pub operation: DomOp,
}

/// Post-mutation evidence for a previously observed plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DomCommitReceipt {
    pub plan_id: DomPlanId,
    pub target: DomTargetId,
    pub context: DomCommitContext,
    pub operation: DomOp,
}

impl DomCommitReceipt {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn from_plan(plan: DomPlan) -> Self {
        Self {
            plan_id: plan.id,
            target: plan.target,
            context: plan.context,
            operation: plan.operation,
        }
    }
}

/// Observes the renderer's real commit path without becoming a second executor.
pub trait DomCommitObserver {
    /// Metadata sampled immediately before each plan notification.
    fn context(&self) -> DomCommitContext {
        DomCommitContext::default()
    }

    fn before_commit(&self, plan: &DomPlan);
    fn after_commit(&self, receipt: &DomCommitReceipt);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_context_is_renderer_neutral() {
        let context = DomCommitContext {
            transaction_id: Some(7),
            state_root: Some("sha256:abc".to_owned()),
        };

        assert_eq!(context.transaction_id, Some(7));
        assert_eq!(context.state_root.as_deref(), Some("sha256:abc"));
    }
}
