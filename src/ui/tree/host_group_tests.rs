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
fn host_states_render_in_the_header_and_as_detail_lines() {
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

    let rows = render_test_support::rendered_rows_at_width(&app, 120);
    assert!(rows[0].contains("⌁ odin"), "{}", rows[0]);
    assert!(rows[0].contains("✗ bad"), "{}", rows[0]);
    assert!(
        ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            .iter()
            .any(|glyph| rows[0].contains(&format!("{glyph} new"))),
        "{}",
        rows[0]
    );
    assert!(rows.iter().any(|row| row.contains("new: connecting...")));
    assert!(rows.iter().any(|row| row.contains("✗ bad: offline")));
    assert!(!rows.iter().any(|row| row.contains("retry later")));

    let header = rendered_cells_for_at_width(&app, "PROJECTS", 120);
    let chip_cross = header
        .iter()
        .find(|cell| cell.symbol() == "✗")
        .expect("failed chip cross");
    assert_eq!(chip_cross.fg, Color::Red);
    assert!(chip_cross.modifier.contains(Modifier::BOLD));

    let failure = rendered_cells_for(&app, "bad: offline");
    let cross = failure
        .iter()
        .find(|cell| cell.symbol() == "✗")
        .expect("failure cross");
    assert_eq!(cross.fg, Color::Red);
    assert!(cross.modifier.contains(Modifier::BOLD));
}

#[test]
fn remote_repo_uses_host_suffix_without_a_divider_or_path() {
    let odin = HostLabel {
        name: "odin".into(),
        ssh: "odin.example".into(),
    };
    let mut app = bare_app(vec![repo("remote", Some(odin.clone()))]);
    app.hosts = vec![HostSlot::connected(odin)];

    let rows = rendered_rows(&app);
    let remote = rows.iter().find(|row| row.contains("remote")).unwrap();
    assert!(remote.contains("@odin"), "{remote}");
    assert!(!remote.contains("/tmp/remote"), "{remote}");
    assert!(!rows.iter().any(|row| row.contains("HOST ")));
}

#[test]
fn connecting_detail_is_hidden_once_that_host_has_a_repo() {
    let host = HostLabel {
        name: "odin".into(),
        ssh: "odin.example".into(),
    };
    let mut app = bare_app(vec![repo("remote", Some(host.clone()))]);
    app.hosts = vec![HostSlot::idle(host)];

    let rows = rendered_rows(&app);
    assert!(!rows.iter().any(|row| row.contains("odin: connecting...")));
}

#[test]
fn narrow_header_drops_a_whole_chip_and_shows_ellipsis() {
    let host = HostLabel {
        name: "long-host".into(),
        ssh: "long.example".into(),
    };
    let mut app = bare_app(Vec::new());
    app.hosts = vec![HostSlot::connected(host)];

    let header = &render_test_support::rendered_rows_at_width(&app, 16)[0];
    assert!(header.contains("PROJECTS…"), "{header}");
    assert!(!header.contains('⌁'), "{header}");
    assert!(!header.contains("long"), "{header}");
}
