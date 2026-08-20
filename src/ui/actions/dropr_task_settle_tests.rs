use std::path::PathBuf;

use super::*;
use crate::{config::Config, dropr::DroprWorkspace, registry::Registry, ui::test_support};

fn workspace(id: &str) -> DroprWorkspace {
    DroprWorkspace {
        kind: "materialised".into(),
        id: id.into(),
        name: "workspace".into(),
        repo_url: String::new(),
    }
}

/// A repository linked to `workspace_id`, or unlinked when it is `None`.
fn repo(path: &str, workspace_id: Option<&str>) -> RepoNode {
    let mut node = test_support::repo(PathBuf::from(path), Vec::new());
    node.dropr = workspace_id.map(workspace);
    node
}

fn test_app(repos: Vec<RepoNode>) -> App {
    let temp = tempfile::tempdir().unwrap();
    let registry = Registry {
        repos,
        ..Default::default()
    };
    App::new(registry, Config::default(), temp.path().into())
}

#[test]
fn a_merge_that_landed_names_its_own_repository_s_workspace() {
    let repos = vec![repo("/repo", Some("workspace-1"))];

    let target = settle_target(&repos, &PathBuf::from("/repo"), &Ok(()));

    assert_eq!(target.as_deref(), Some("workspace-1"));
}

/// The failure path. Nothing is written and nothing is marked, so a merge that
/// did not land has nothing to correct — it must simply not ask.
#[test]
fn a_merge_that_failed_asks_for_nothing() {
    let repos = vec![repo("/repo", Some("workspace-1"))];

    let target = settle_target(
        &repos,
        &PathBuf::from("/repo"),
        &Err("merge refused".to_string()),
    );

    assert_eq!(target, None);
}

#[test]
fn an_unlinked_repository_asks_for_nothing() {
    let repos = vec![repo("/repo", None)];

    let target = settle_target(&repos, &PathBuf::from("/repo"), &Ok(()));

    assert_eq!(target, None);
}

/// A repository robco does not track cannot be resolved to a workspace, so a
/// stray path is a silent no-op rather than a panic.
#[test]
fn a_repository_robco_does_not_know_asks_for_nothing() {
    let repos = vec![repo("/repo", Some("workspace-1"))];

    let target = settle_target(&repos, &PathBuf::from("/elsewhere"), &Ok(()));

    assert_eq!(target, None);
}

#[test]
fn only_the_merged_repository_s_workspace_is_asked() {
    let repos = vec![
        repo("/repo-a", Some("workspace-a")),
        repo("/repo-b", Some("workspace-b")),
    ];

    let target = settle_target(&repos, &PathBuf::from("/repo-b"), &Ok(()));

    assert_eq!(target.as_deref(), Some("workspace-b"));
}

#[test]
fn a_landed_merge_is_recorded_for_the_next_tick() {
    let mut app = test_app(vec![repo("/repo", Some("workspace-1"))]);

    app.note_merge_settled(&PathBuf::from("/repo"), &Ok(()));

    assert_eq!(app.dropr_task_settle, vec!["workspace-1".to_string()]);
}

#[test]
fn a_failed_merge_records_nothing_for_the_next_tick() {
    let mut app = test_app(vec![repo("/repo", Some("workspace-1"))]);

    app.note_merge_settled(&PathBuf::from("/repo"), &Err("boom".to_string()));

    assert!(app.dropr_task_settle.is_empty());
}

/// Two repositories can share one workspace, and their merges can finish inside
/// the same tick. One entry means one fetch — this is what keeps a burst of
/// merges from becoming a burst of dropr calls.
#[test]
fn two_merges_onto_one_workspace_ask_once() {
    let mut app = test_app(vec![
        repo("/repo-a", Some("workspace-1")),
        repo("/repo-b", Some("workspace-1")),
    ]);

    app.note_merge_settled(&PathBuf::from("/repo-a"), &Ok(()));
    app.note_merge_settled(&PathBuf::from("/repo-b"), &Ok(()));

    assert_eq!(app.dropr_task_settle, vec!["workspace-1".to_string()]);
}
