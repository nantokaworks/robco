use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;

use super::*;
use crate::{config::Config, registry::Registry};

const WIDTH: u16 = 60;
const HEIGHT: u16 = 12;

/// One repo carrying an Overseer Auto worker, an Overseer Manual worker, and a
/// hand-made worktree nobody manages.
fn app_with_managed_workers() -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    // Only worktrees under `worktree_root` survive `prune_unmanaged_agents`.
    let agent = |id: &str, title: &str, parent: Option<&str>, management: &str| {
        serde_json::json!({
            "id": id,
            "parent_agent_id": parent,
            "management": management,
            "title": title,
            "worktree_path": config.worktree_root.join(title),
            "branch": title,
            "base_commit": "",
            "program": "claude",
            "tmux_session": format!("robco_repo_{title}"),
            "created_at": "2026-01-01T00:00:00+09:00",
            "updated_at": "2026-01-01T00:00:00+09:00",
        })
    };
    let overseer = Some(crate::overseer::OVERSEER_AGENT_ID);
    let repo = serde_json::from_value(serde_json::json!({
        "path": temp.path().join("repo"),
        "name": "repo",
        "remote_url": null,
        "agents": [
            agent("auto-id", "auto-worker", overseer, "auto"),
            agent("manual-id", "manual-worker", overseer, "manual"),
            agent("plain-id", "hand-made", None, "manual"),
        ],
    }))
    .unwrap();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let mut app = App::new(registry, config, temp.path().into());
    // Ignore host tmux sessions and the OVERSEER pane so the tree rows are
    // deterministic and start at the top of the frame.
    app.orphans = Vec::new();
    app.overseer_visible = false;
    app
}

fn rendered_rows(app: &App) -> Vec<String> {
    let visible = app.visible();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    terminal
        .draw(|frame| draw(frame, app, &visible, None))
        .unwrap();
    let buffer = terminal.backend().buffer();
    // Rows are read from the tree pane's own left edge so a row string starts
    // with the cursor cell, not with the frame margin.
    let tree = layout::panes(layout::root(buffer.area).body, app.overseer_frame_height()).tree;
    (tree.y..tree.bottom())
        .map(|y| {
            (tree.x..tree.right())
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect()
        })
        .collect()
}

fn row_containing(rows: &[String], title: &str) -> String {
    rows.iter()
        .find(|row| row.contains(title))
        .unwrap_or_else(|| panic!("no rendered row for {title}"))
        .clone()
}

/// The column the title starts at, measured in cells rather than bytes so the
/// multi-byte marker does not distort it.
fn title_column(rows: &[String], title: &str) -> usize {
    let row = row_containing(rows, title);
    let byte = row.find(title).unwrap();
    UnicodeWidthStr::width(&row[..byte])
}

#[test]
fn only_an_overseer_auto_worker_carries_the_row_head_marker() {
    let rows = rendered_rows(&app_with_managed_workers());

    assert!(
        row_containing(&rows, "auto-worker").starts_with("  ◆ "),
        "auto worker row: {:?}",
        row_containing(&rows, "auto-worker")
    );
    for unmarked in ["manual-worker", "hand-made"] {
        assert!(
            row_containing(&rows, unmarked).starts_with("    "),
            "{unmarked} row: {:?}",
            row_containing(&rows, unmarked)
        );
    }
}

#[test]
fn the_marker_does_not_shift_the_title_column() {
    let rows = rendered_rows(&app_with_managed_workers());

    assert_eq!(
        title_column(&rows, "auto-worker"),
        title_column(&rows, "manual-worker")
    );
    assert_eq!(
        title_column(&rows, "auto-worker"),
        title_column(&rows, "hand-made")
    );
}

#[test]
fn the_trailing_management_glyphs_are_gone() {
    let rows = rendered_rows(&app_with_managed_workers());

    assert!(
        !rows
            .iter()
            .any(|row| row.contains('Ⓐ') || row.contains('Ⓜ')),
        "rows still carry a trailing management glyph: {rows:?}"
    );
}
