use super::*;
use crate::{
    config::Config,
    overseer::{config::OverseerConfig, ledger::Ledger},
    registry::Registry,
};

fn warning_state() -> (Vec<&'static str>, App) {
    let config = OverseerConfig {
        dispatch_enabled: true,
        failure_circuit_threshold: 2,
        ..OverseerConfig::default()
    };
    let mut ledger = Ledger::default();
    ledger.counters.consecutive_failures = 2;
    let warnings = crate::ui::overseer::health_warnings_from(&config, &ledger, false, false);
    let temp = tempfile::tempdir().unwrap();
    let app = App::new(Registry::default(), Config::default(), temp.path().into());
    (warnings, app)
}

/// One inbox item so the category renders a marker row, a `none`-free
/// header, and the key hint — the three rows that used to carry their own
/// leading spaces on top of the frame indent.
fn inbox_app() -> App {
    let (_, mut app) = warning_state();
    app.overseer_inbox = vec![crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        target_session: None,
        target_id: "task-1".into(),
        label: "task-1".into(),
        at: chrono::Utc::now(),
    }];
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app
}

#[test]
fn expanded_detail_rows_share_one_indent_under_the_category_label() {
    let app = inbox_app();
    let content = build_content_with_warnings(&app, Some(23), &[]);

    let label = OverseerCategory::Inbox.label();
    let category = content
        .lines
        .iter()
        .position(|line| line.to_string().contains(label))
        .expect("no Inbox category row");
    // The label the detail rows nest under starts at column 4. Measured in
    // columns, not bytes: the expand arrow ahead of it is three bytes wide.
    let row = content.lines[category].to_string();
    let label_at = row.find(label).expect("no Inbox label");
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(&row[..label_at]),
        DETAIL_INDENT.len()
    );

    // Every detail row starts at exactly that column: one indent origin, no
    // row adding a second one of its own.
    let detail_count = crate::ui::overseer::category_detail(&app, OverseerCategory::Inbox).len();
    let details = content.lines[category + 1..=category + detail_count]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert!(
        details.iter().any(|line| line.contains("[ESC]")),
        "inbox item row missing: {details:?}"
    );
    for detail in &details {
        if detail.trim().is_empty() {
            continue;
        }
        assert_eq!(
            detail.len() - detail.trim_start().len(),
            DETAIL_INDENT.len(),
            "detail row is not at the frame's single indent origin: {detail:?}"
        );
    }
}

#[test]
fn active_health_warnings_have_dedicated_narrow_rows() {
    let (warnings, app) = warning_state();
    assert_eq!(
        warnings,
        ["STALE/OFFLINE", "circuit OPEN", "dispatch/no daemon"]
    );

    for tree_width in [24, 48] {
        let content = build_content_with_warnings(&app, Some(tree_width - 1), &warnings);
        for warning in &warnings {
            let expected = format!("⚠ {warning}");
            let rows = content
                .lines
                .iter()
                .filter(|line| line.to_string() == expected)
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 1);
            assert!(rows[0].width() <= 23);
        }
    }
}

#[test]
fn the_header_is_a_plain_label_with_no_arrow_or_marker() {
    let (warnings, app) = warning_state();
    let content = build_content_with_warnings(&app, Some(23), &warnings);
    let header = content.lines[0].to_string();

    assert!(header.starts_with("OVERSEER"), "header row: {header:?}");
    assert!(!header.contains('▾') && !header.contains('▸'));
    assert!(header.contains("⚠×3"));
}

#[test]
fn the_header_indicator_stays_beside_the_label_at_every_frame_width() {
    let (warnings, app) = warning_state();
    // A fresh app reports a dead daemon, so the glyph is the static Dead
    // status rather than a time-dependent spinner frame.
    let glyph = crate::model::Status::Dead.glyph();

    let mut headers = Vec::new();
    for tree_width in [24_u16, 48] {
        let content = build_content_with_warnings(&app, Some(tree_width - 1), &warnings);
        let header = content.lines[0].to_string();
        assert_eq!(
            header,
            format!("OVERSEER  {glyph}  ⚠×3"),
            "header row at tree width {tree_width}"
        );
        headers.push(header);
    }
    // The row does not grow with the frame: widening the sidebar must not
    // push the glyph away from the label it describes.
    assert_eq!(headers[0], headers[1]);
}

#[test]
fn a_live_daemon_leaves_the_header_label_bare() {
    let (warnings, mut app) = warning_state();
    app.overseer_snapshot.daemon_alive = true;

    // Dispatch on (the state that used to animate forever) and dispatch off
    // are both healthy, so neither draws a glyph beside the label.
    for dispatch_enabled in [true, false] {
        app.overseer_snapshot.overseer.dispatch_enabled = dispatch_enabled;
        for tree_width in [24_u16, 48] {
            let content = build_content_with_warnings(&app, Some(tree_width - 1), &warnings);
            assert_eq!(
                content.lines[0].to_string(),
                "OVERSEER  ⚠×3",
                "header row with dispatch_enabled={dispatch_enabled} at tree width {tree_width}"
            );
        }
    }
}

#[test]
fn a_daemon_on_another_build_is_warned_about_under_the_header() {
    // The state the header cannot otherwise show: the daemon is alive, so it
    // draws no glyph, while running an image that predates what was merged.
    let (_, mut app) = warning_state();
    app.overseer_snapshot.daemon_alive = true;
    app.overseer_snapshot.daemon_version = Some("0.0.1".into());

    let content = content_lines(&app);
    let rendered = content
        .lines
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();

    assert!(rendered[0].contains("⚠×"), "header row: {:?}", rendered[0]);
    assert!(
        rendered
            .iter()
            .any(|line| line.contains(crate::overseer::heartbeat::DRIFT_LABEL)),
        "{rendered:?}"
    );
}

#[test]
fn a_bare_header_hands_its_reserved_columns_back_to_the_label() {
    let (_, mut app) = warning_state();
    app.overseer_snapshot.daemon_alive = true;

    // Eight columns is exactly the label: with no glyph to reserve room for,
    // nothing is trimmed off it.
    let content = build_content_with_warnings(&app, Some(8), &[]);
    assert_eq!(content.lines[0].to_string(), "OVERSEER");
}

#[test]
fn warning_rows_are_included_in_selected_category_scroll_position() {
    let (warnings, mut app) = warning_state();
    app.overseer_visible = true;
    app.selected = OverseerCategory::Decisions.index();

    let content = build_content_with_warnings(&app, Some(23), &warnings);

    assert_eq!(content.selected_row, 7);
    assert_eq!(content.scroll_offset(6), 2);
    assert!(content.selected_row - content.scroll_offset(6) < 6);
}
