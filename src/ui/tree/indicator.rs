use crate::model::{MergeLifecycle, Status};

mod render;
pub(in crate::ui::tree) use render::{primary_span, supplementary_spans};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Indicator {
    Status(Status),
    Merging,
    Running,
    McpActivity,
    ShellActivity,
    SubagentActivity(usize),
    DroprRefresh,
    /// Overrides the plain `Status::Done` glyph when the worker's pull
    /// request is open but not merged, so a session gone quiet never
    /// pretends to be finished. See [`select`].
    MergeLifecycle(MergeLifecycle),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct IndicatorState {
    pub dead: bool,
    pub merging: bool,
    pub running: bool,
    pub waiting: bool,
    pub worktree_missing: bool,
    pub merge_failed: bool,
    /// The worker reported itself blocked (`robco report --kind blocked`) and
    /// the Overseer's own triage has not since closed the loop on its own.
    /// See [`crate::ui::overseer::OverseerSnapshot::blocked_reason`].
    pub needs_decision: bool,
    pub mcp_active: bool,
    pub shell_active: bool,
    pub subagents_active: usize,
    pub dropr_refresh: bool,
    pub static_status: Option<Status>,
    /// Where the worker's pull request stands, when it has one open. Only
    /// ever shown in place of the plain `Status::Done` glyph — see
    /// [`select`] — so a value here is inert unless `static_status` is
    /// exactly `Some(Status::Done)`.
    pub merge_lifecycle: Option<MergeLifecycle>,
}

impl IndicatorState {
    pub(super) fn with_status(status: Option<Status>) -> Self {
        Self {
            dead: status == Some(Status::Dead),
            merging: false,
            running: status == Some(Status::Running),
            waiting: status == Some(Status::Waiting),
            worktree_missing: false,
            merge_failed: false,
            needs_decision: false,
            mcp_active: false,
            shell_active: false,
            subagents_active: 0,
            dropr_refresh: false,
            static_status: status.filter(|status| {
                matches!(status, Status::Done | Status::Idle | Status::BranchOnly)
            }),
            merge_lifecycle: None,
        }
    }
}

/// Selects the primary row indicator in this order, highest priority first:
/// dead/error status, merge activity, running spinner, waiting status, MCP
/// activity, shell activity, active subagent count, repo dropr refresh, then
/// the static Done/Idle/BranchOnly status glyph — except that a `Done` row
/// with an open, unmerged pull request shows its merge-lifecycle glyph
/// instead, so a session gone quiet is never indistinguishable from one
/// that actually merged. Worktree-missing state is supplementary and is
/// selected separately by [`select_supplementary`].
pub(super) fn select(state: IndicatorState) -> Option<Indicator> {
    if state.dead {
        Some(Indicator::Status(Status::Dead))
    } else if state.merging {
        Some(Indicator::Merging)
    } else if state.running {
        Some(Indicator::Running)
    } else if state.waiting {
        Some(Indicator::Status(Status::Waiting))
    } else if state.mcp_active {
        Some(Indicator::McpActivity)
    } else if state.shell_active {
        Some(Indicator::ShellActivity)
    } else if state.subagents_active > 0 {
        Some(Indicator::SubagentActivity(state.subagents_active))
    } else if state.dropr_refresh {
        Some(Indicator::DroprRefresh)
    } else if state.static_status == Some(Status::Done) && state.merge_lifecycle.is_some() {
        state.merge_lifecycle.map(Indicator::MergeLifecycle)
    } else {
        state.static_status.map(Indicator::Status)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SupplementaryIndicators {
    pub worktree_missing: bool,
    pub merge_failed: bool,
    pub needs_decision: bool,
}

pub(super) fn select_supplementary(state: IndicatorState) -> SupplementaryIndicators {
    SupplementaryIndicators {
        worktree_missing: state.worktree_missing,
        merge_failed: state.merge_failed,
        needs_decision: state.needs_decision,
    }
}

#[cfg(test)]
#[path = "indicator_tests.rs"]
mod tests;
