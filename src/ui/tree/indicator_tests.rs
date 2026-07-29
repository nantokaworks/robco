use super::*;

fn idle_state() -> IndicatorState {
    IndicatorState::with_status(None)
}

fn assert_missing_pair(state: IndicatorState, primary: Option<Indicator>) {
    assert_eq!(select(state), primary);
    assert_eq!(
        select_supplementary(state),
        SupplementaryIndicators {
            worktree_missing: true,
            merge_failed: false,
            needs_decision: false,
        }
    );
}

#[test]
fn dead_beats_running() {
    let mut state = idle_state();
    state.dead = true;
    state.running = true;
    assert_eq!(select(state), Some(Indicator::Status(Status::Dead)));
}

#[test]
fn merging_is_high_priority_but_does_not_hide_dead_state() {
    let mut state = idle_state();
    state.merging = true;
    state.running = true;
    state.shell_active = true;
    assert_eq!(select(state), Some(Indicator::Merging));

    state.dead = true;
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
    assert_missing_pair(state, Some(Indicator::Status(Status::Waiting)));
}

#[test]
fn running_beats_mcp_activity() {
    let mut state = IndicatorState::with_status(Some(Status::Running));
    state.mcp_active = true;
    assert_eq!(select(state), Some(Indicator::Running));
}

#[test]
fn waiting_beats_mcp_activity() {
    let mut state = IndicatorState::with_status(Some(Status::Waiting));
    state.mcp_active = true;
    assert_eq!(select(state), Some(Indicator::Status(Status::Waiting)));
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
    assert_missing_pair(state, Some(Indicator::ShellActivity));
}

#[test]
fn mcp_activity_beats_shell_activity() {
    let mut state = idle_state();
    state.mcp_active = true;
    state.shell_active = true;
    assert_eq!(select(state), Some(Indicator::McpActivity));
}

#[test]
fn mcp_activity_pairs_with_missing_worktree() {
    let mut state = idle_state();
    state.worktree_missing = true;
    state.mcp_active = true;
    assert_missing_pair(state, Some(Indicator::McpActivity));
}

#[test]
fn subagent_activity_pairs_with_missing_worktree() {
    let mut state = idle_state();
    state.worktree_missing = true;
    state.subagents_active = 2;
    assert_missing_pair(state, Some(Indicator::SubagentActivity(2)));
}

#[test]
fn missing_worktree_is_the_only_indicator_without_a_primary() {
    let mut state = idle_state();
    state.worktree_missing = true;
    assert_eq!(
        (select(state), select_supplementary(state)),
        (
            None,
            SupplementaryIndicators {
                worktree_missing: true,
                merge_failed: false,
                needs_decision: false,
            }
        )
    );
}

#[test]
fn row_without_missing_worktree_has_no_supplementary_indicator() {
    let mut state = idle_state();
    state.shell_active = true;
    assert_eq!(
        (select(state), select_supplementary(state)),
        (
            Some(Indicator::ShellActivity),
            SupplementaryIndicators {
                worktree_missing: false,
                merge_failed: false,
                needs_decision: false,
            }
        )
    );
}

#[test]
fn merge_failure_is_supplementary() {
    let mut state = IndicatorState::with_status(Some(Status::Done));
    state.merge_failed = true;
    assert_eq!(
        select_supplementary(state),
        SupplementaryIndicators {
            worktree_missing: false,
            merge_failed: true,
            needs_decision: false,
        }
    );
}

#[test]
fn needs_decision_is_supplementary() {
    let mut state = IndicatorState::with_status(Some(Status::Done));
    state.needs_decision = true;
    assert_eq!(
        select_supplementary(state),
        SupplementaryIndicators {
            worktree_missing: false,
            merge_failed: false,
            needs_decision: true,
        }
    );
}

#[test]
fn needs_decision_does_not_change_the_primary_indicator() {
    // Overlaying a "needs a decision" badge must never repurpose the
    // primary glyph — a `Running` row stays `Running`.
    let mut state = IndicatorState::with_status(Some(Status::Running));
    state.running = true;
    state.needs_decision = true;
    assert_eq!(select(state), Some(Indicator::Running));
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
