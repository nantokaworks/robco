use super::*;
use crate::dropr::DroprTaskCandidate;

fn task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_owned(),
        title: display_id.to_owned(),
        priority: String::new(),
        status: "open".to_owned(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
    }
}

fn answered(tasks: Vec<DroprTaskCandidate>) -> DroprTaskFetch {
    DroprTaskFetch {
        tasks,
        problems: Vec::new(),
        answered: true,
    }
}

#[test]
fn unloaded_overlay_is_not_reported_as_missing_linkage() {
    assert_eq!(
        no_workspace_reason(true, OverlayStatus::Pending),
        DroprTaskReload::OverlayPending
    );
    assert_eq!(
        no_workspace_reason(true, OverlayStatus::Unavailable),
        DroprTaskReload::OverlayUnavailable
    );
    assert_eq!(
        no_workspace_reason(false, OverlayStatus::Pending),
        DroprTaskReload::OverlayDisabled
    );
}

#[test]
fn loaded_overlay_with_no_match_reports_missing_linkage() {
    assert_eq!(
        no_workspace_reason(true, OverlayStatus::Loaded),
        DroprTaskReload::NoLinkedWorkspaces
    );
}

#[test]
fn fetched_rows_overwrite_current_tasks() {
    let mut current = answered(vec![task("#1")]);

    apply_fetched_tasks(&mut current, answered(vec![task("#2")]));

    assert_eq!(current.tasks, vec![task("#2")]);
}

#[test]
fn fetched_empty_rows_clear_current_tasks() {
    let mut current = answered(vec![task("#1")]);

    apply_fetched_tasks(&mut current, answered(Vec::new()));

    assert!(current.tasks.is_empty());
    assert!(current.answered);
}

/// The defect: a failed fetch used to leave the previous rows in place, so the
/// pane went on presenting a list nothing had re-checked as if it were current.
#[test]
fn failed_fetch_replaces_current_tasks_with_the_failure() {
    let mut current = answered(vec![task("#1")]);

    apply_fetched_tasks(&mut current, DroprTaskFetch::failed("dropr did not answer"));

    assert!(current.tasks.is_empty());
    assert!(!current.answered);
    assert_eq!(current.problems, ["dropr did not answer"]);
}

/// A reload the operator asked for that never came back has to say so; the pane
/// otherwise sits unchanged and reads as "nothing to update".
#[test]
fn an_abandoned_manual_reload_is_reported_on_the_pane() {
    let mut current = answered(vec![task("#1")]);

    note_abandoned_reload(&mut current, Locale::En);
    note_abandoned_reload(&mut current, Locale::En);

    // The rows stay — they are what the pane has — but they no longer read as
    // current, and repeated sweeps do not stack the same line.
    assert_eq!(current.tasks, vec![task("#1")]);
    assert_eq!(current.problems, [ABANDONED_RELOAD]);
}

#[test]
fn a_stale_manual_refresh_is_named_when_it_is_swept() {
    let now = Instant::now();
    let mut refresh = DroprTaskRefresh::new();
    refresh
        .in_flight
        .insert("manual-workspace".to_owned(), now - REFRESH_STALE_AFTER);
    refresh
        .in_flight
        .insert("background-workspace".to_owned(), now - REFRESH_STALE_AFTER);
    refresh.manual.insert("manual-workspace".to_owned());

    let abandoned = expire_stale_refreshes(&mut refresh, now);

    // Only the manual one is reported: nobody is waiting on the background sweep.
    assert_eq!(abandoned, ["manual-workspace"]);
}

/// The budget the fetch runs under has to be shorter than the window the UI
/// will still accept an answer in, or a slow dropr silently loses every reload.
#[test]
fn the_fetch_budget_fits_inside_the_stale_window() {
    assert!(crate::dropr::FETCH_BUDGET < REFRESH_STALE_AFTER);
}

#[test]
fn background_refresh_is_hidden_from_ui() {
    let mut refresh = DroprTaskRefresh::new();
    assert!(!track_refresh(&mut refresh, "workspace", false));
    refresh
        .in_flight
        .insert("workspace".to_owned(), Instant::now());

    assert!(!refresh_visible(&refresh, "workspace"));
}

#[test]
fn manual_refresh_is_visible_to_ui() {
    let mut refresh = DroprTaskRefresh::new();
    assert!(!track_refresh(&mut refresh, "workspace", true));
    refresh
        .in_flight
        .insert("workspace".to_owned(), Instant::now());

    assert!(refresh_visible(&refresh, "workspace"));
}

#[test]
fn stale_refresh_expires_manual_flag() {
    let mut refresh = DroprTaskRefresh::new();
    refresh
        .in_flight
        .insert("workspace".to_owned(), Instant::now() - REFRESH_STALE_AFTER);
    refresh.manual.insert("workspace".to_owned());

    assert!(!track_refresh(&mut refresh, "workspace", false));
    assert!(!refresh.in_flight.contains_key("workspace"));
    assert!(!refresh.manual.contains("workspace"));
}

#[test]
fn sweep_expires_removed_workspace_refresh() {
    let now = Instant::now();
    let mut refresh = DroprTaskRefresh::new();
    refresh
        .in_flight
        .insert("removed-workspace".to_owned(), now - REFRESH_STALE_AFTER);
    refresh.in_flight.insert("linked-workspace".to_owned(), now);
    refresh.manual.insert("removed-workspace".to_owned());
    refresh.manual.insert("linked-workspace".to_owned());

    expire_stale_refreshes(&mut refresh, now);

    assert!(!refresh.in_flight.contains_key("removed-workspace"));
    assert!(!refresh.manual.contains("removed-workspace"));
    assert!(refresh.in_flight.contains_key("linked-workspace"));
    assert!(refresh.manual.contains("linked-workspace"));
}

#[test]
fn manual_request_marks_in_flight_background_refresh() {
    let mut refresh = DroprTaskRefresh::new();
    refresh
        .in_flight
        .insert("workspace".to_owned(), Instant::now());

    assert!(track_refresh(&mut refresh, "workspace", true));
    assert!(refresh_visible(&refresh, "workspace"));
}
