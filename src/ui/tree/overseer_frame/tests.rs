use super::*;
use crate::{config::Config, registry::Registry};

/// A fixed three-warning fixture, decoupled from real health-warning content:
/// these tests exercise the frame's row-wrapping and width layout, not which
/// warnings `health_warnings_from` can actually produce.
fn warning_state() -> (Vec<&'static str>, App) {
    let warnings = vec!["STALE/OFFLINE", "second warning", "third warning"];
    let temp = tempfile::tempdir().unwrap();
    let app = App::new(Registry::default(), Config::default(), temp.path().into());
    (warnings, app)
}

/// One inbox item so the category renders a `none`-free header and an item
/// row, with the cursor on the item — the two rows that used to carry their
/// own leading spaces on top of the frame indent. The cursor sits on the item
/// so its marker is `>`; an unselected row spends that column on a space,
/// which is the row idiom rather than a second indent.
fn inbox_app() -> App {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;
    app.overseer_inbox = vec![crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: None,
        target_session: None,
        target_id: "task-1".into(),
        label: "task-1".into(),
        detail: "needs user".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }];
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::OverseerInbox(0)))
        .expect("no inbox item row");
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
fn the_selected_inbox_item_is_the_row_the_frame_scrolls_to() {
    // The item row, not the Inbox category above it, is what the cursor is on,
    // so it is what must stay on screen and carry the marker.
    let app = inbox_app();
    let content = build_content_with_warnings(&app, Some(23), &[]);

    let row = usize::from(content.selected_row);
    let selected = content.lines[row].to_string();
    assert!(
        selected.trim_start().starts_with("> [ESC]"),
        "selected row: {selected:?}"
    );
    // The category expands straight into item rows: the row above the first
    // item is the category itself, with no second `inbox (N)` level between
    // them. A stale row offset here would scroll the frame to the wrong row.
    let category = content.lines[row - 1].to_string();
    assert!(
        category.contains(OverseerCategory::Inbox.label()),
        "row above the first item: {category:?}"
    );
    // The count the nested row used to carry is on the category row already, so
    // removing that level lost no information.
    // The lone item has no live session, but its reason resolves to a remedy
    // other than `Answer` (an orphaned answer becomes `Review`), so it still
    // counts: actionable tracks the resolved remedy, not session presence.
    assert!(category.contains("1/1 actionable"), "{category:?}");
    assert!(
        !content
            .lines
            .iter()
            .any(|line| line.to_string().contains("inbox (")),
        "the duplicated inbox count row is back"
    );
}

#[test]
fn an_empty_inbox_still_renders_a_visible_empty_state() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);

    let content = build_content_with_warnings(&app, Some(23), &[]);
    let category = content
        .lines
        .iter()
        .position(|line| line.to_string().contains(OverseerCategory::Inbox.label()))
        .expect("no Inbox category row");

    assert_eq!(content.lines[category + 1].to_string().trim(), "none");
}

#[test]
fn the_inbox_row_indicator_lights_only_when_something_is_actionable() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;

    let unlit = build_content_with_warnings(&app, Some(23), &[])
        .lines
        .iter()
        .map(ToString::to_string)
        .find(|line| line.contains(OverseerCategory::Inbox.label()))
        .expect("no Inbox row");
    assert!(
        !unlit.contains(crate::model::Status::Waiting.glyph()),
        "an empty Inbox carries the waiting glyph: {unlit:?}"
    );

    app.overseer_inbox = vec![crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: None,
        target_session: None,
        target_id: "task-1".into(),
        label: "task-1".into(),
        detail: "needs user".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }];
    let lit = build_content_with_warnings(&app, Some(23), &[])
        .lines
        .iter()
        .map(ToString::to_string)
        .find(|line| line.contains(OverseerCategory::Inbox.label()))
        .expect("no Inbox row");
    assert!(
        lit.contains(crate::model::Status::Waiting.glyph()),
        "an actionable Inbox is missing the waiting glyph: {lit:?}"
    );
}

#[test]
fn active_health_warnings_have_dedicated_narrow_rows() {
    let (warnings, app) = warning_state();
    assert_eq!(warnings.len(), 3);

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

    for tree_width in [24_u16, 48] {
        let content = build_content_with_warnings(&app, Some(tree_width - 1), &warnings);
        assert_eq!(
            content.lines[0].to_string(),
            "OVERSEER  ⚠×3",
            "header row at tree width {tree_width}"
        );
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

/// Column each category label starts at, measured in columns rather than bytes
/// because the expand arrow ahead of it is three bytes wide.
fn label_columns(app: &App) -> Vec<(&'static str, usize)> {
    let content = build_content_with_warnings(app, Some(23), &[]);
    OverseerCategory::ALL
        .into_iter()
        .map(|category| {
            let label = category.label();
            let row = content
                .lines
                .iter()
                .map(ToString::to_string)
                .find(|line| line.contains(label))
                .unwrap_or_else(|| panic!("no {label} category row"));
            let at = row.find(label).expect("no label");
            (label, unicode_width::UnicodeWidthStr::width(&row[..at]))
        })
        .collect()
}

#[test]
fn only_the_inbox_category_carries_an_expand_arrow() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;
    let content = build_content_with_warnings(&app, Some(23), &[]);

    for category in OverseerCategory::ALL {
        let row = content
            .lines
            .iter()
            .map(ToString::to_string)
            .find(|line| line.contains(category.label()))
            .expect("no category row");
        let has_arrow = row.contains('▸') || row.contains('▾');
        assert_eq!(
            has_arrow,
            category.has_children(),
            "{} row: {row:?}",
            category.label()
        );
    }
}

#[test]
fn every_category_label_starts_at_the_same_column() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;

    // The arrow cell is reserved on every row and left blank on the leaves, so
    // losing three arrows must not outdent three labels past the fourth.
    let collapsed = label_columns(&app);
    assert!(
        collapsed
            .iter()
            .all(|(_, column)| *column == DETAIL_INDENT.len()),
        "{collapsed:?}"
    );

    // And expanding the one category that can expand does not move any of them.
    app.set_overseer_category_expanded(OverseerCategory::Inbox, true);
    assert_eq!(label_columns(&app), collapsed);
}

#[test]
fn a_leaf_category_cannot_be_expanded_by_any_key() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;

    for category in OverseerCategory::ALL
        .into_iter()
        .filter(|c| !c.has_children())
    {
        // +1: the control AI row sits ahead of every category in `visible()`.
        app.selected = category.index() + 1;
        assert_eq!(
            app.selected_item(),
            Some(Selection::OverseerCategory(category))
        );
        // Captured with the cursor already on the row, so the only difference a
        // key could make is the one under test.
        let before = build_content_with_warnings(&app, Some(23), &[])
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        for key in [
            crossterm::event::KeyCode::Right,
            crossterm::event::KeyCode::Char('l'),
            crossterm::event::KeyCode::Enter,
        ] {
            app.handle_key(crossterm::event::KeyEvent::new(
                key,
                crossterm::event::KeyModifiers::NONE,
            ))
            .unwrap();
            assert!(
                !app.overseer_category_expanded(category),
                "{} expanded on {key:?}",
                category.label()
            );
            let after = build_content_with_warnings(&app, Some(23), &[])
                .lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            assert_eq!(after, before, "{} changed on {key:?}", category.label());
        }
    }
}

#[test]
fn a_leaf_categorys_detail_is_still_reachable_in_the_preview() {
    // The sidebar stopped rendering it; `category_detail` is intentionally kept
    // because the Info preview is now its only consumer.
    let (_, mut app) = warning_state();
    app.overseer_visible = true;

    for category in OverseerCategory::ALL
        .into_iter()
        .filter(|c| !c.has_children())
    {
        let (title, detail) = crate::ui::overseer::category_preview(&app, category);
        assert_eq!(title, format!("OVERSEER / {}", category.label()));
        assert!(
            !detail.lines.is_empty(),
            "{} preview is empty",
            category.label()
        );
    }
}

#[test]
fn ledger_and_decisions_are_plain_top_level_rows() {
    // dropr:469 retired the `Details` wrapper `Ledger` and `Decisions` used to
    // nest under (dropr:378): both render as their own row, unconditionally,
    // the same as `Health`.
    let (_, mut app) = warning_state();
    app.overseer_visible = true;
    let rendered = build_content_with_warnings(&app, Some(23), &[])
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    for category in [OverseerCategory::Ledger, OverseerCategory::Decisions] {
        assert!(
            rendered.iter().any(|line| line.contains(category.label())),
            "{} row missing: {rendered:?}",
            category.label()
        );
        assert!(!category.has_children(), "{}", category.label());
    }
}

#[test]
fn a_selected_top_level_category_is_the_row_the_frame_scrolls_to() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;
    app.selected = app
        .visible()
        .iter()
        .position(|row| *row == Selection::OverseerCategory(OverseerCategory::Decisions))
        .expect("no Decisions row");

    let content = build_content_with_warnings(&app, Some(23), &[]);
    let selected = content.lines[usize::from(content.selected_row)].to_string();
    assert!(
        selected.contains(OverseerCategory::Decisions.label()),
        "selected row: {selected:?}"
    );
    assert!(
        selected.trim_start().starts_with('>'),
        "selected row carries no marker: {selected:?}"
    );
}

#[test]
fn warning_rows_are_included_in_selected_category_scroll_position() {
    let (warnings, mut app) = warning_state();
    app.overseer_visible = true;
    // +1: the control AI row sits ahead of every category in `visible()`.
    app.selected = OverseerCategory::Discord.index() + 1;

    let content = build_content_with_warnings(&app, Some(23), &warnings);

    assert_eq!(content.selected_row, 9);
    assert_eq!(content.scroll_offset(6), 4);
    assert!(content.selected_row - content.scroll_offset(6) < 6);
}

#[test]
fn the_control_ai_row_sits_above_every_category() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;
    let content = build_content_with_warnings(&app, Some(23), &[]);

    let control_row = content
        .lines
        .iter()
        .position(|line| line.to_string().contains("Control AI"))
        .expect("no Control AI row");
    let inbox_row = content
        .lines
        .iter()
        .position(|line| line.to_string().contains(OverseerCategory::Inbox.label()))
        .expect("no Inbox category row");
    assert!(control_row < inbox_row);

    app.selected = 0;
    assert_eq!(
        app.selected_item(),
        Some(crate::model::Selection::OverseerAi)
    );
    let content = build_content_with_warnings(&app, Some(23), &[]);
    assert_eq!(usize::from(content.selected_row), control_row);
}

#[test]
fn the_control_ai_row_shows_no_badge_until_the_session_exists() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;

    app.overseer_snapshot.control_status = None;
    let content = build_content_with_warnings(&app, Some(23), &[]);
    let row = content
        .lines
        .iter()
        .find(|line| line.to_string().contains("Control AI"))
        .expect("no Control AI row")
        .to_string();
    let after_label = row.split("Control AI").nth(1).unwrap_or_default();
    assert!(
        after_label.trim().is_empty(),
        "no session should draw no badge: {row:?}"
    );

    app.overseer_snapshot.control_status = Some(crate::model::Status::Running);
    let content = build_content_with_warnings(&app, Some(23), &[]);
    let row = content
        .lines
        .iter()
        .find(|line| line.to_string().contains("Control AI"))
        .expect("no Control AI row")
        .to_string();
    let after_label = row.split("Control AI").nth(1).unwrap_or_default();
    assert!(
        !after_label.trim().is_empty(),
        "a running session should draw the spinner glyph: {row:?}"
    );
}

#[test]
fn the_control_ai_row_shows_the_waiting_glyph_while_awaiting_input() {
    let (_, mut app) = warning_state();
    app.overseer_visible = true;

    app.overseer_snapshot.control_status = Some(crate::model::Status::Waiting);
    let content = build_content_with_warnings(&app, Some(23), &[]);
    let row = content
        .lines
        .iter()
        .find(|line| line.to_string().contains("Control AI"))
        .expect("no Control AI row")
        .to_string();
    let after_label = row.split("Control AI").nth(1).unwrap_or_default();
    assert!(
        !after_label.trim().is_empty(),
        "a session awaiting confirmation should draw a badge: {row:?}"
    );
}
