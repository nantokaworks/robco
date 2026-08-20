use super::*;
use crate::{config::Config, registry::Registry};

use super::render_test_support::{rendered_rows, row_containing, title_column};

/// Three flat, Overseer-owned agents in one repo. Titles are kept short
/// (single digits of the 24-column sidebar minimum's budget after the
/// box-drawing connector) so these tests exercise row layout — column
/// alignment, connector position — rather than title truncation.
fn app_with_flat_agents() -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    // Only worktrees under `worktree_root` survive `prune_unmanaged_agents`.
    let agent = |id: &str, title: &str| {
        serde_json::json!({
            "id": id,
            "parent_agent_id": crate::overseer::OVERSEER_AGENT_ID,
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
    let repo = serde_json::from_value(serde_json::json!({
        "path": temp.path().join("repo"),
        "name": "repo",
        "remote_url": null,
        "agents": [
            agent("a-id", "agt-a"),
            agent("b-id", "agt-b"),
            agent("c-id", "agt-c"),
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

/// An agent hangs off the repo above it, so its title has to start right of the
/// repo name — the nesting step left of the identity-tree indent has to survive
/// the widest repo row.
#[test]
fn an_agent_title_starts_right_of_its_repo_name() {
    // The widest repo row the config allows: a two-column project icon plus a
    // reserved indicator cell both push the repo name right, leaving the
    // narrowest gap the nesting step ever has to survive.
    let mut widest = app_with_flat_agents();
    widest.config.project_icon = crate::config::ProjectIcon::Emoji;
    widest.registry.repos[0].main_status = Some(Status::Idle);
    let mut nerdfont = app_with_flat_agents();
    nerdfont.config.project_icon = crate::config::ProjectIcon::Nerdfont;

    for app in [app_with_flat_agents(), widest, nerdfont] {
        let rows = rendered_rows(&app);
        let repo = title_column(&rows, "repo");

        for agent in ["agt-a", "agt-b", "agt-c"] {
            let title = title_column(&rows, agent);
            assert!(
                title > repo,
                "{agent} title column {title} is not right of the repo name column {repo}"
            );
        }
    }
}

/// `#327`: an agent row's connector must hang directly under the repo row's
/// fold icon, not float clear of it. Checked under both `project_icon = none`
/// (a single narrow triangle) and `project_icon = emoji` (a two-cell folder
/// glyph) — an offset that only lines up by coincidence under the default
/// config would still be wrong here.
#[test]
fn an_agent_connector_starts_under_the_repo_fold_icon() {
    for icon in [
        crate::config::ProjectIcon::None,
        crate::config::ProjectIcon::Emoji,
    ] {
        let mut app = app_with_flat_agents();
        app.config.project_icon = icon;
        let rows = rendered_rows(&app);

        let repo_row = row_containing(&rows, "repo");
        let icon_byte = repo_row
            .find(icon.marker(true))
            .expect("repo row carries its fold icon");
        let icon_column = repo_row[..icon_byte].chars().count();

        let agent_row = row_containing(&rows, "agt-a");
        let connector_byte = agent_row
            .find('├')
            .expect("agent row carries its connector");
        let connector_column = agent_row[..connector_byte].chars().count();

        assert_eq!(
            connector_column, icon_column,
            "{icon:?}: connector column {connector_column} does not match fold icon column {icon_column}"
        );
    }
}

#[test]
fn agent_titles_at_the_same_depth_share_one_column() {
    let rows = rendered_rows(&app_with_flat_agents());

    assert_eq!(title_column(&rows, "agt-a"), title_column(&rows, "agt-b"));
    assert_eq!(title_column(&rows, "agt-a"), title_column(&rows, "agt-c"));
}

/// No row draws a management marker glyph any more — the dial that produced
/// one is gone.
#[test]
fn no_management_marker_glyph_is_drawn() {
    let rows = rendered_rows(&app_with_flat_agents());

    assert!(
        !rows
            .iter()
            .any(|row| row.contains('●') || row.contains('○')),
        "rows still carry a management marker glyph: {rows:?}"
    );
}

/// A subagent one level deep, with a root-level later sibling of its own
/// parent: the guide over the parent's column must keep drawing `│` while the
/// subagent's own connector still reads `└` (it is its parent's only child).
/// This is the last-sibling-at-every-depth case #308 exists to cover — not
/// just the subagent's own branch, but every ancestor's guide above it.
#[test]
fn nested_agent_rows_draw_ancestor_guides() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let agent = |id: &str, title: &str, parent: Option<&str>| {
        serde_json::json!({
            "id": id,
            "parent_agent_id": parent,
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
    let repo = serde_json::from_value(serde_json::json!({
        "path": temp.path().join("repo"),
        "name": "repo",
        "remote_url": null,
        "agents": [
            agent("root-a", "r-a", None),
            agent("root-b", "r-b", None),
            agent("nested-a", "n-a", Some("root-a")),
        ],
    }))
    .unwrap();
    let registry = Registry {
        version: 1,
        repos: vec![repo],
    };
    let mut app = App::new(registry, config, temp.path().into());
    app.orphans = Vec::new();
    app.overseer_visible = false;

    let rows = rendered_rows(&app);
    // root-a has a later root sibling (root-b), so its own branch is `├` and
    // the guide it leaves behind for its descendants continues.
    assert!(row_containing(&rows, "r-a").starts_with("  ├── "));
    // nested-a is root-a's only child (its own branch is `└`), but the guide
    // over root-a's column must still show `│`, not blank.
    assert!(row_containing(&rows, "n-a").starts_with("  │ └── "));
    // root-b is the last root-level agent, so its branch is `└` and nothing
    // precedes it.
    assert!(row_containing(&rows, "r-b").starts_with("  └── "));
}
