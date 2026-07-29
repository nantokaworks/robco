use super::*;
use crate::{config::Config, registry::Registry};

use super::render_test_support::{rendered_rows, row_containing, title_column};

/// One repo carrying an Overseer Auto worker, an Overseer Manual worker, and a
/// hand-made worktree nobody manages. Titles are kept short (single digits of
/// the 24-column sidebar minimum's budget after the box-drawing connector) so
/// these tests exercise marker/column placement rather than title truncation.
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
            agent("auto-id", "auto-agt", overseer, "auto"),
            agent("manual-id", "man-agt", overseer, "manual"),
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

/// The repo in `app_with_managed_workers` carries no `management` field, so it
/// serde-defaults to Auto. `#362`: `auto-agt` matching that Auto repo state no
/// longer blanks its marker — an Auto agent is always drawn now, since a
/// repo in Auto mode is the common case and blanking every Auto agent under
/// it used to render it identically to `hand-made`, an unmanaged worktree.
/// `man-agt` still diverges from the repo and keeps its hollow glyph;
/// `hand-made` (never the Overseer's) was always blank. `auto-agt` and
/// `man-agt` are not the repo's last agent, so both draw the `├` connector;
/// `hand-made` is last and draws `└`.
#[test]
fn the_indented_marker_tells_auto_manual_and_unmanaged_apart() {
    let rows = rendered_rows(&app_with_managed_workers());

    for (title, expected) in [
        ("auto-agt", "  ├── ● "),
        ("man-agt", "  ├── ○ "),
        ("hand-made", "  └──   "),
    ] {
        assert!(
            row_containing(&rows, title).starts_with(expected),
            "{title} row does not start with {expected:?}: {:?}",
            row_containing(&rows, title)
        );
    }
}

/// `#362`: with `project_icon = "nerdfont"` the same three-way distinction
/// holds, but through the bolt/hand pictograph pair instead of the round
/// fallback — the acceptance criterion is the marker set, not the specific
/// glyphs.
#[test]
fn the_nerdfont_marker_set_also_tells_the_three_states_apart() {
    let mut app = app_with_managed_workers();
    app.config.project_icon = crate::config::ProjectIcon::Nerdfont;
    let rows = rendered_rows(&app);

    for (title, expected) in [
        ("auto-agt", "  ├── \u{f0e7} "),
        ("man-agt", "  ├── \u{f256} "),
        ("hand-made", "  └──   "),
    ] {
        assert!(
            row_containing(&rows, title).starts_with(expected),
            "{title} row does not start with {expected:?}: {:?}",
            row_containing(&rows, title)
        );
    }
}

/// Once the repo itself is switched to Manual (the `G` toggle), `auto-agt`
/// diverges from the repo and keeps showing its marker (as it already did
/// before the switch — Auto never blanks), while `man-agt` now matches the
/// Manual repo and goes blank instead.
#[test]
fn a_manual_agent_marker_blanks_when_it_starts_matching_its_repo() {
    let mut app = app_with_managed_workers();
    app.registry.repos[0].management = crate::model::ManagementMode::Manual;
    let rows = rendered_rows(&app);

    for (title, expected) in [
        ("auto-agt", "  ├── ● "),
        ("man-agt", "  ├──   "),
        ("hand-made", "  └──   "),
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
    let mut nerdfont = app_with_managed_workers();
    nerdfont.config.project_icon = crate::config::ProjectIcon::Nerdfont;

    for app in [app_with_managed_workers(), widest, nerdfont] {
        let rows = rendered_rows(&app);
        let repo = title_column(&rows, "repo");

        for agent in ["auto-agt", "man-agt", "hand-made"] {
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
        let mut app = app_with_managed_workers();
        app.config.project_icon = icon;
        let rows = rendered_rows(&app);

        let repo_row = row_containing(&rows, "repo");
        let icon_byte = repo_row
            .find(icon.marker(true))
            .expect("repo row carries its fold icon");
        let icon_column = repo_row[..icon_byte].chars().count();

        let agent_row = row_containing(&rows, "auto-agt");
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
fn the_marker_does_not_shift_the_title_column() {
    let rows = rendered_rows(&app_with_managed_workers());

    assert_eq!(
        title_column(&rows, "auto-agt"),
        title_column(&rows, "man-agt")
    );
    assert_eq!(
        title_column(&rows, "auto-agt"),
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
            "management": "manual",
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
        "management": "manual",
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
    assert!(row_containing(&rows, "r-a").starts_with("  ├──   "));
    // nested-a is root-a's only child (its own branch is `└`), but the guide
    // over root-a's column must still show `│`, not blank.
    assert!(row_containing(&rows, "n-a").starts_with("  │ └──   "));
    // root-b is the last root-level agent, so its branch is `└` and nothing
    // precedes it.
    assert!(row_containing(&rows, "r-b").starts_with("  └──   "));
}
