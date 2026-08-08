use chrono::TimeZone;

use super::*;

fn now() -> chrono::DateTime<Local> {
    Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
}

#[test]
fn live_running_session_reports_missing_worktree() {
    let mut state = WatchStatusState::default();
    let report = classify_agent_observation(
        true,
        Some("Generating response (esc to interrupt)"),
        false,
        false,
        &mut state,
        now(),
    )
    .unwrap();

    assert_eq!(report.status, Status::Running);
    assert!(!report.awaiting_confirmation);
    assert!(report.worktree_missing);
}

#[test]
fn live_confirmation_reports_waiting_and_missing_worktree() {
    let mut state = WatchStatusState::default();
    let report = classify_agent_observation(
        true,
        Some("Allow edit src/main.rs? (y/n)"),
        false,
        false,
        &mut state,
        now(),
    )
    .unwrap();

    assert_eq!(report.status, Status::Waiting);
    assert!(report.awaiting_confirmation);
    assert!(report.worktree_missing);
}

#[test]
fn live_session_clears_missing_flag_when_worktree_reappears() {
    let mut state = WatchStatusState::default();
    let missing = classify_agent_observation(
        true,
        Some("Generating response (esc to interrupt)"),
        false,
        false,
        &mut state,
        now(),
    )
    .unwrap();
    let restored = classify_agent_observation(
        true,
        Some("Generating response (esc to interrupt)"),
        true,
        false,
        &mut state,
        now(),
    )
    .unwrap();

    assert!(missing.worktree_missing);
    assert!(!restored.worktree_missing);
}

#[test]
fn dead_session_paths_keep_missing_flag_clear() {
    let mut state = WatchStatusState::default();
    let branch_only =
        classify_agent_observation(false, None, false, true, &mut state, now()).unwrap();
    let dead = classify_agent_observation(false, None, false, false, &mut state, now()).unwrap();

    assert_eq!(branch_only.status, Status::BranchOnly);
    assert!(!branch_only.worktree_missing);
    assert_eq!(dead.status, Status::Dead);
    assert!(!dead.worktree_missing);
}

fn report(status: Status) -> StatusReport {
    StatusReport {
        status,
        awaiting_confirmation: true,
        worktree_missing: true,
        mcp_active: false,
    }
}

#[test]
fn shell_pane_downgrade_only_changes_running_shells() {
    assert_eq!(
        shell_pane_downgrade(report(Status::Running), Some("zsh")).status,
        Status::Idle
    );
    for command in [Some("claude"), Some("2_1_208"), None] {
        assert_eq!(
            shell_pane_downgrade(report(Status::Running), command).status,
            Status::Running
        );
    }
    for status in [
        Status::Idle,
        Status::Waiting,
        Status::Done,
        Status::Dead,
        Status::BranchOnly,
    ] {
        let unchanged = shell_pane_downgrade(report(status), Some("bash"));
        assert_eq!(unchanged.status, status);
        assert!(unchanged.awaiting_confirmation);
        assert!(unchanged.worktree_missing);
    }
}

#[test]
fn stale_working_marker_in_shell_pane_is_not_running() {
    let classified = classify_capture(
        "Generating response (esc to interrupt)",
        &mut WatchStatusState::default(),
        now(),
    );

    assert_eq!(classified.status, Status::Running);
    assert_eq!(
        shell_pane_downgrade(classified, Some("zsh")).status,
        Status::Idle
    );
}

#[test]
fn control_session_idle_capture_does_not_report_running() {
    let mut state = WatchStatusState::default();
    let status = classify_session_observation(Some("nothing interesting here"), &mut state, now());

    assert_eq!(status, Some(Status::Idle));
}

#[test]
fn control_session_working_capture_reports_running() {
    let mut state = WatchStatusState::default();
    let status = classify_session_observation(
        Some("Generating response (esc to interrupt)"),
        &mut state,
        now(),
    );

    assert_eq!(status, Some(Status::Running));
}

#[test]
fn control_session_absent_capture_reports_none() {
    let mut state = WatchStatusState::default();
    let status = classify_session_observation(None, &mut state, now());

    assert_eq!(status, None);
}
