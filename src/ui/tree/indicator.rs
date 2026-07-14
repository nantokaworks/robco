use crate::model::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Indicator {
    Status(Status),
    Running,
    ShellActivity,
    SubagentActivity(usize),
    DroprRefresh,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct IndicatorState {
    pub dead: bool,
    pub running: bool,
    pub waiting: bool,
    pub worktree_missing: bool,
    pub shell_active: bool,
    pub subagents_active: usize,
    pub dropr_refresh: bool,
    pub static_status: Option<Status>,
}

impl IndicatorState {
    pub(super) fn with_status(status: Option<Status>) -> Self {
        Self {
            dead: status == Some(Status::Dead),
            running: status == Some(Status::Running),
            waiting: status == Some(Status::Waiting),
            worktree_missing: false,
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
/// dead/error status, running spinner, waiting status, shell activity, active
/// subagent count, repo dropr refresh, then the static Done/Idle/BranchOnly
/// status glyph. Worktree-missing state is supplementary and is selected
/// separately by [`select_supplementary`].
pub(super) fn select(state: IndicatorState) -> Option<Indicator> {
    if state.dead {
        Some(Indicator::Status(Status::Dead))
    } else if state.running {
        Some(Indicator::Running)
    } else if state.waiting {
        Some(Indicator::Status(Status::Waiting))
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

pub(super) fn select_supplementary(state: IndicatorState) -> bool {
    state.worktree_missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle_state() -> IndicatorState {
        IndicatorState::with_status(None)
    }

    #[test]
    fn dead_beats_running() {
        let mut state = idle_state();
        state.dead = true;
        state.running = true;
        assert_eq!(select(state), Some(Indicator::Status(Status::Dead)));
    }

    #[test]
    fn running_beats_waiting() {
        let mut state = idle_state();
        state.running = true;
        state.waiting = true;
        assert_eq!(select(state), Some(Indicator::Running));
    }

    #[test]
    fn waiting_pairs_with_missing_worktree() {
        let mut state = IndicatorState::with_status(Some(Status::Waiting));
        state.worktree_missing = true;
        assert_eq!(
            (select(state), select_supplementary(state)),
            (Some(Indicator::Status(Status::Waiting)), true)
        );
    }

    #[test]
    fn running_beats_shell_activity() {
        let mut state = IndicatorState::with_status(Some(Status::Running));
        state.shell_active = true;
        assert_eq!(select(state), Some(Indicator::Running));
    }

    #[test]
    fn shell_activity_pairs_with_missing_worktree() {
        let mut state = idle_state();
        state.worktree_missing = true;
        state.shell_active = true;
        assert_eq!(
            (select(state), select_supplementary(state)),
            (Some(Indicator::ShellActivity), true)
        );
    }

    #[test]
    fn subagent_activity_pairs_with_missing_worktree() {
        let mut state = idle_state();
        state.worktree_missing = true;
        state.subagents_active = 2;
        assert_eq!(
            (select(state), select_supplementary(state)),
            (Some(Indicator::SubagentActivity(2)), true)
        );
    }

    #[test]
    fn missing_worktree_is_the_only_indicator_without_a_primary() {
        let mut state = idle_state();
        state.worktree_missing = true;
        assert_eq!((select(state), select_supplementary(state)), (None, true));
    }

    #[test]
    fn row_without_missing_worktree_has_no_supplementary_indicator() {
        let mut state = idle_state();
        state.shell_active = true;
        assert_eq!(
            (select(state), select_supplementary(state)),
            (Some(Indicator::ShellActivity), false)
        );
    }

    #[test]
    fn shell_activity_beats_subagent_activity() {
        let mut state = idle_state();
        state.shell_active = true;
        state.subagents_active = 2;
        assert_eq!(select(state), Some(Indicator::ShellActivity));
    }

    #[test]
    fn subagent_activity_beats_dropr_refresh() {
        let mut state = idle_state();
        state.subagents_active = 3;
        state.dropr_refresh = true;
        assert_eq!(select(state), Some(Indicator::SubagentActivity(3)));
    }

    #[test]
    fn static_status_is_the_fallback() {
        let state = IndicatorState::with_status(Some(Status::Done));
        assert_eq!(select(state), Some(Indicator::Status(Status::Done)));
    }

    #[test]
    fn dropr_refresh_beats_static_fallback() {
        let mut state = IndicatorState::with_status(Some(Status::Done));
        state.dropr_refresh = true;
        assert_eq!(select(state), Some(Indicator::DroprRefresh));
    }

    #[test]
    fn repo_with_only_manual_refresh_shows_dropr_refresh() {
        let mut state = idle_state();
        state.dropr_refresh = true;
        assert_eq!(select(state), Some(Indicator::DroprRefresh));
    }
}
