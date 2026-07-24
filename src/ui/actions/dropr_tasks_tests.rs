use super::*;

fn task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_owned(),
        title: display_id.to_owned(),
        priority: String::new(),
        status: "ready".to_owned(),
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
    let mut current = vec![task("#1")];

    apply_fetched_tasks(&mut current, Some(vec![task("#2")]));

    assert_eq!(current, vec![task("#2")]);
}

#[test]
fn fetched_empty_rows_clear_current_tasks() {
    let mut current = vec![task("#1")];

    apply_fetched_tasks(&mut current, Some(Vec::new()));

    assert!(current.is_empty());
}

#[test]
fn failed_fetch_retains_current_tasks() {
    let mut current = vec![task("#1")];

    apply_fetched_tasks(&mut current, None);

    assert_eq!(current, vec![task("#1")]);
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
