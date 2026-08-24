use super::*;

fn idle_state() -> IndicatorState {
    IndicatorState::with_status(None)
}

fn assert_missing_pair(state: IndicatorState, primary: Option<Indicator>) {
    assert_eq!(select(state), primary);
    assert_eq!(
        select_supplementary(state),
        SupplementaryIndicators {
            merge_queued: false,
            worktree_missing: true,
            merge_failed: false,
            needs_decision: false,
            worker_finished: false,
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
                merge_queued: false,
                worktree_missing: true,
                merge_failed: false,
                needs_decision: false,
                worker_finished: false,
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
                merge_queued: false,
                worktree_missing: false,
                merge_failed: false,
                needs_decision: false,
                worker_finished: false,
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
            merge_queued: false,
            worktree_missing: false,
            merge_failed: true,
            needs_decision: false,
            worker_finished: false,
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
            merge_queued: false,
            worktree_missing: false,
            merge_failed: false,
            needs_decision: true,
            worker_finished: false,
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

#[test]
fn a_done_row_with_an_open_pull_request_shows_its_lifecycle_glyph_instead_of_done() {
    let mut state = IndicatorState::with_status(Some(Status::Done));
    state.merge_lifecycle = Some(MergeLifecycle::ChecksRunning);
    assert_eq!(
        select(state),
        Some(Indicator::MergeLifecycle(MergeLifecycle::ChecksRunning))
    );
}

#[test]
fn a_done_row_with_no_ledger_entry_still_shows_plain_done() {
    // The no-entry case (a hand-created worktree, or work the Overseer
    // never dispatched) must fall back to the ordinary session glyph rather
    // than rendering as an error or as merged.
    let state = IndicatorState::with_status(Some(Status::Done));
    assert_eq!(state.merge_lifecycle, None);
    assert_eq!(select(state), Some(Indicator::Status(Status::Done)));
}

#[test]
fn a_merged_pull_request_keeps_the_plain_done_glyph() {
    // `merge_lifecycle` never reports a value for `LedgerPhase::Merged` (see
    // `ui::overseer::decisions::merge_lifecycle`), so a merged worker is
    // indistinguishable here from the no-entry case above — both are
    // genuinely finished.
    let state = IndicatorState::with_status(Some(Status::Done));
    assert_eq!(select(state), Some(Indicator::Status(Status::Done)));
}

#[test]
fn checks_failing_is_distinguishable_from_checks_running() {
    let mut running = IndicatorState::with_status(Some(Status::Done));
    running.merge_lifecycle = Some(MergeLifecycle::ChecksRunning);
    let mut failing = IndicatorState::with_status(Some(Status::Done));
    failing.merge_lifecycle = Some(MergeLifecycle::ChecksFailing);
    assert_ne!(select(running), select(failing));
}

#[test]
fn running_beats_merge_lifecycle() {
    // A worker that resumed real work outranks whatever the ledger still
    // says about its last pull request — same priority reasoning as
    // `needs_decision`'s gating in `ui::tree`.
    let mut state = IndicatorState::with_status(Some(Status::Running));
    state.running = true;
    state.merge_lifecycle = Some(MergeLifecycle::ChecksFailing);
    assert_eq!(select(state), Some(Indicator::Running));
}

#[test]
fn merge_lifecycle_is_inert_off_the_done_status() {
    // `merge_lifecycle` is only ever consulted in place of `Status::Done`;
    // setting it alongside `Idle` must not conjure a glyph out of nothing.
    let mut state = IndicatorState::with_status(Some(Status::Idle));
    state.merge_lifecycle = Some(MergeLifecycle::OnHold);
    assert_eq!(select(state), Some(Indicator::Status(Status::Idle)));
}
