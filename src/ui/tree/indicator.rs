use crate::model::Status;

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
        }
    }
}

/// Selects the primary row indicator in this order, highest priority first:
/// dead/error status, merge activity, running spinner, waiting status, MCP
/// activity, shell activity, active subagent count, repo dropr refresh, then
/// the static Done/Idle/BranchOnly status glyph. Worktree-missing state is
/// supplementary and is selected separately by [`select_supplementary`].
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
