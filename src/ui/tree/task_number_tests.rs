//! `#333`: an Overseer-dispatched agent's row leads with its dropr task
//! number so the operator does not have to scan past a repeated worktree
//! prefix to find it; a manually-created agent carries no `task_number` at
//! all and its row is unchanged.

use super::*;
use crate::{config::Config, registry::Registry};

use super::render_test_support::{rendered_rows, rendered_rows_at_width, row_containing};

/// `title` is a parameter so callers can probe both the 24-column sidebar
/// minimum (short title, no truncation) and a wide sidebar (long title,
/// marquee budget exercised) with the same fixture shape.
fn app_with_a_numbered_and_a_manual_worker(title: &str) -> App {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::default();
    let overseer = Some(crate::overseer::OVERSEER_AGENT_ID);
    let agent = |id: &str, title: &str, parent: Option<&str>, task_number: Option<&str>| {
        serde_json::json!({
            "id": id,
            "parent_agent_id": parent,
            "management": "auto",
            "title": title,
            "task_number": task_number,
            "worktree_path": config.worktree_root.join(id),
            "branch": id,
            "base_commit": "",
            "program": "claude",
            "tmux_session": format!("robco_repo_{id}"),
            "created_at": "2026-01-01T00:00:00+09:00",
            "updated_at": "2026-01-01T00:00:00+09:00",
        })
    };
    let repo = serde_json::from_value(serde_json::json!({
        "path": temp.path().join("repo"),
        "name": "repo",
        "remote_url": null,
        "agents": [
            agent("dispatched", title, overseer, Some("333")),
            agent("hand-made", "hand-made", None, None),
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
    app
}

/// `WIDTH` (60) already lands the tree pane at the 24-column sidebar minimum
/// (`tree_width`'s ratio of a 58-wide body clamps up to the 24-column floor).
/// A manually-created agent in the same tree carries no leading `#`.
#[test]
fn a_dispatched_row_leads_with_its_number_and_a_manual_row_does_not() {
    let rows = rendered_rows(&app_with_a_numbered_and_a_manual_worker("agt"));
    assert!(
        row_containing(&rows, "agt").contains("#333 agt"),
        "dispatched row does not lead with its task number at the sidebar minimum: {:?}",
        row_containing(&rows, "agt")
    );
    let manual_row = row_containing(&rows, "hand-made");
    assert!(
        !manual_row.contains('#'),
        "manually-created row unexpectedly carries a task number: {manual_row:?}"
    );
}

/// A wide sidebar (`tree_width` clamped to its 48-column ceiling) gives the
/// marquee enough budget to render a longer numbered title in full, without
/// the number eating into a column the title would otherwise have used.
#[test]
fn a_numbered_row_keeps_its_number_at_a_wide_sidebar() {
    // body_width = frame_width - 2 (root() margin); 0.3 * 170 = 51, clamped
    // down to the 48-column ceiling.
    let rows = rendered_rows_at_width(
        &app_with_a_numbered_and_a_manual_worker("Lead worker names"),
        172,
    );
    assert!(
        row_containing(&rows, "Lead worker names").contains("#333 Lead worker names"),
        "dispatched row does not lead with its task number at a wide sidebar: {:?}",
        row_containing(&rows, "Lead worker names")
    );
}
