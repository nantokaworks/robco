use ratatui::{Terminal, backend::TestBackend};

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

/// The column the title starts at, counted in cells rather than bytes or display
/// width so neither the multi-byte marker nor a two-column project icon distorts
/// it. A wide glyph lives in a single cell and its second column is left blank,
/// so one cell is one column here.
fn title_column(rows: &[String], title: &str) -> usize {
    let row = row_containing(rows, title);
    let byte = row.find(title).unwrap();
    row[..byte].chars().count()
}

/// The repo in `app_with_managed_workers` carries no `management` field, so it
/// serde-defaults to Auto. An agent row's own marker is blanked whenever it
/// matches that repo state (`ManagementMarker::unless_matching`) — the repo
/// row already said it — so only `manual-worker`, which diverges, keeps its
/// hollow glyph; `auto-worker` inherits blank and `hand-made` (never the
/// Overseer's) was always blank.
#[test]
fn the_indented_marker_tells_auto_manual_and_unmanaged_apart() {
    let rows = rendered_rows(&app_with_managed_workers());

    for (title, expected) in [
        ("auto-worker", "        "),
        ("manual-worker", "      ○ "),
        ("hand-made", "        "),
    ] {
        assert!(
            row_containing(&rows, title).starts_with(expected),
            "{title} row does not start with {expected:?}: {:?}",
            row_containing(&rows, title)
        );
    }
}

/// Once the repo itself is switched to Manual (the `G` toggle), the repo row's
/// own state no longer says "Auto" for `auto-worker` — so that worker's own
/// marker starts diverging from its repo and reappears, while `manual-worker`
/// now matches the repo and goes blank instead.
#[test]
fn an_agent_marker_reappears_when_it_stops_matching_its_repo() {
    let mut app = app_with_managed_workers();
    app.registry.repos[0].management = crate::model::ManagementMode::Manual;
    let rows = rendered_rows(&app);

    for (title, expected) in [
        ("auto-worker", "      ● "),
        ("manual-worker", "        "),
        ("hand-made", "        "),
    ] {
        assert!(
            row_containing(&rows, title).starts_with(expected),
            "{title} row does not start with {expected:?}: {:?}",
            row_containing(&rows, title)
        );
    }
}

/// The repo row itself always shows its own marker, unconditionally — it is
/// the one row that never blanks out, since it is the state every agent row
/// is compared against.
#[test]
fn the_repo_row_always_shows_its_own_marker() {
    let auto = rendered_rows(&app_with_managed_workers());
    assert!(row_containing(&auto, "repo").contains('●'));

    let mut manual = app_with_managed_workers();
    manual.registry.repos[0].management = crate::model::ManagementMode::Manual;
    let manual_rows = rendered_rows(&manual);
    assert!(row_containing(&manual_rows, "repo").contains('○'));
}

/// An agent hangs off the repo above it, so its title has to start right of the
/// repo name. The management marker no longer expresses that containment — it
/// sits inside the row's indentation and travels with it — so the nesting step
/// left of the identity-tree indent is what has to survive the widest repo row.
#[test]
fn an_agent_title_starts_right_of_its_repo_name() {
    // The widest repo row the config allows: a two-column project icon plus a
    // reserved indicator cell both push the repo name right, leaving the
    // narrowest gap the nesting step ever has to survive.
    let mut widest = app_with_managed_workers();
    widest.config.project_icon = crate::config::ProjectIcon::Emoji;
    widest.registry.repos[0].main_status = Some(Status::Idle);

    for app in [app_with_managed_workers(), widest] {
        let rows = rendered_rows(&app);
        let repo = title_column(&rows, "repo");

        for agent in ["auto-worker", "manual-worker", "hand-made"] {
            let title = title_column(&rows, agent);
            assert!(
                title > repo,
                "{agent} title column {title} is not right of the repo name column {repo}"
            );
        }
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
