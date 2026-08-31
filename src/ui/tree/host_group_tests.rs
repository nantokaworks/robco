use super::{render_test_support::rendered_rows, *};
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
    assert!(rows.iter().any(|row| row.contains("local")));
    assert!(!rows.iter().any(|row| row.contains("HOST")));
}

#[test]
fn two_hosts_render_in_configured_groups_after_local() {
    let prod = HostLabel {
        name: "Production".into(),
        ssh: "prod".into(),
    };
    let dev = HostLabel {
        name: "Dev".into(),
        ssh: "dev@example".into(),
    };
    let mut app = bare_app(vec![
        repo("local", None),
        repo("prod-repo", Some(prod.clone())),
        repo("dev-repo", Some(dev.clone())),
    ]);
    app.hosts = vec![HostSlot::idle(prod), HostSlot::idle(dev)];
    let rows = rendered_rows(&app);
    let positions = [
        "local",
        "HOST Production",
        "prod-repo",
        "HOST Dev",
        "dev-repo",
    ]
    .map(|needle| rows.iter().position(|row| row.contains(needle)).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
