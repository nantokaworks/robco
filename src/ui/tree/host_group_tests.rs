use ratatui::style::{Color, Modifier};

use super::{
    render_test_support::{rendered_cells_for, rendered_cells_for_at_width, rendered_rows},
    *,
};
use crate::{
    config::Config, model::HostLabel, registry::Registry, ui::actions::remote_hosts::HostSlot,
};

fn repo(name: &str, host: Option<HostLabel>) -> crate::model::RepoNode {
    let mut repo: crate::model::RepoNode = serde_json::from_value(serde_json::json!({
        "path": format!("/tmp/{name}"), "name": name, "remote_url": null,
        "pinned": true
    }))
    .unwrap();
    repo.host = host;
    repo
}

fn bare_app(repos: Vec<crate::model::RepoNode>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(
        Registry { version: 1, repos },
        Config::default(),
        temp.path().into(),
    );
    app.overseer_visible = false;
    app.orphans.clear();
    app
}

#[test]
fn zero_hosts_adds_no_tree_indirection() {
    let rows = rendered_rows(&bare_app(vec![repo("local", None)]));
    assert_eq!(rows[0].trim_end(), "PROJECTS");
    assert!(rows.iter().any(|row| row.contains("local")));
    assert!(!rows.iter().any(|row| row.contains("HOST")));
}

#[test]
fn host_states_render_as_top_level_rows() {
    let odin = HostLabel {
        name: "odin".into(),
        ssh: "odin.example".into(),
    };
    let connecting = HostLabel {
        name: "new".into(),
        ssh: "new.example".into(),
    };
    let failed = HostLabel {
        name: "bad".into(),
        ssh: "bad.example".into(),
    };
    let mut app = bare_app(vec![
        repo("local", None),
        repo("remote", Some(odin.clone())),
    ]);
    app.hosts = vec![
        HostSlot::connected(odin),
        HostSlot::idle(connecting),
        HostSlot::failed(failed, "offline\nretry later"),
    ];
    app.sync_remote_host_views();

    let rows = render_test_support::rendered_rows_at_width(&app, 120);
    assert_eq!(rows[0].trim_end(), "PROJECTS");
    assert!(
        rows.iter()
            .any(|row| row.contains("⌁ odin") && row.contains("1 repos"))
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("✗ bad") && row.contains("offline"))
    );
    assert!(
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            .iter()
            .any(|glyph| rows.iter().any(|row| row.contains(&format!("{glyph} new"))))
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("new") && row.contains("connecting..."))
    );
    assert!(rows.iter().any(|row| row.contains("✗ bad: offline")));
    assert_eq!(
        rows.iter()
            .filter(|row| row.contains("✗ bad: offline"))
            .count(),
        1
    );
    assert!(!rows.iter().any(|row| row.contains("retry later")));

    assert!(!rows[0].contains("odin"));
    assert!(!rows[0].contains("bad"));

    let failure = rendered_cells_for(&app, "bad: offline");
    let cross = failure
        .iter()
        .find(|cell| cell.symbol() == "✗")
        .expect("failure cross");
    assert_eq!(cross.fg, Color::Red);
    assert!(cross.modifier.contains(Modifier::BOLD));
}

#[test]
fn remote_repo_nests_without_a_host_suffix_or_path() {
    let odin = HostLabel {
        name: "odin".into(),
        ssh: "odin.example".into(),
    };
    let mut app = bare_app(vec![repo("remote", Some(odin.clone()))]);
    app.hosts = vec![HostSlot::connected(odin)];
    app.sync_remote_host_views();

    let rows = rendered_rows(&app);
    let remote = rows.iter().find(|row| row.contains("remote")).unwrap();
    assert!(!remote.contains("@odin"), "{remote}");
    assert!(!remote.contains("/tmp/remote"), "{remote}");
    assert!(rows.iter().any(|row| row.contains("⌁ odin")));
    assert!(remote.starts_with("  "), "{remote}");
}

#[test]
fn connecting_host_hides_stale_repo_children() {
    let host = HostLabel {
        name: "odin".into(),
        ssh: "odin.example".into(),
    };
    let mut app = bare_app(vec![repo("remote", Some(host.clone()))]);
    app.hosts = vec![HostSlot::idle(host)];
    app.sync_remote_host_views();

    let rows = rendered_rows(&app);
    assert!(
        rows.iter()
            .any(|row| row.contains("odin") && row.contains("connecting.")),
        "{rows:?}"
    );
    assert!(!rows.iter().any(|row| row.contains("remote")));
}

#[test]
fn narrow_host_row_keeps_the_label_before_clipping_the_summary() {
    let host = HostLabel {
        name: "long-host".into(),
        ssh: "long.example".into(),
    };
    let mut app = bare_app(Vec::new());
    app.hosts = vec![HostSlot::connected(host)];
    app.sync_remote_host_views();

    let rows = render_test_support::rendered_rows_at_width(&app, 16);
    let host = rows
        .iter()
        .find(|row| row.contains("long-h"))
        .unwrap_or_else(|| panic!("{rows:?}"));
    assert!(host.contains("⌁ long-h"), "{host}");
    assert!(!host.contains("0 repos"), "{host}");
}

#[test]
fn connected_host_with_dead_daemon_shows_red_warning() {
    let host = HostLabel {
        name: "odin".into(),
        ssh: "odin.example".into(),
    };
    let mut app = bare_app(Vec::new());
    app.hosts = vec![HostSlot::connected_with_chats(
        host,
        None,
        Default::default(),
        false,
    )];
    app.sync_remote_host_views();

    let cells = rendered_cells_for_at_width(&app, "odin", 120);
    let warning = cells.iter().find(|cell| cell.symbol() == "⚠").unwrap();
    assert_eq!(warning.fg, Color::Red);
}
