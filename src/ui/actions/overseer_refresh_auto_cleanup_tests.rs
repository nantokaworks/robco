//! `App::run_auto_cleanup` — dropr:563. Split out of `overseer_refresh.rs`'s
//! own `mod tests` to keep that file at its size limit.

use std::path::{Path, PathBuf};

use super::*;
use crate::ui::test_support;

fn app_with_agent(repo_path: &Path, agent_id: &str) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.registry.repos = vec![test_support::repo(
        repo_path.to_path_buf(),
        vec![test_support::agent(agent_id, repo_path.join("worktree"))],
    )];
    app
}

#[test]
fn a_candidate_starts_the_existing_clean_only_sequence() {
    let repo_path = PathBuf::from("/repo");
    let mut app = app_with_agent(&repo_path, "worker-a");

    app.run_auto_cleanup(vec![(repo_path.clone(), "worker-a".to_string())]);

    assert_eq!(
        app.merge_job(&repo_path).map(|job| job.agent_id.as_str()),
        Some("worker-a")
    );
}

#[test]
fn a_repository_already_running_a_merge_job_is_left_alone() {
    let repo_path = PathBuf::from("/repo");
    let mut app = app_with_agent(&repo_path, "worker-a");
    app.run_auto_cleanup(vec![(repo_path.clone(), "worker-a".to_string())]);
    assert_eq!(app.merge_jobs.len(), 1);

    // A second sweep naming the same repository must not touch the job
    // already in flight — `start_cleanup` would otherwise show a "merge
    // already in progress" toast nobody asked for, since this path is never
    // operator-initiated.
    app.run_auto_cleanup(vec![(repo_path.clone(), "worker-a".to_string())]);

    assert_eq!(app.merge_jobs.len(), 1);
    assert!(app.message.is_none());
}

#[test]
fn a_candidate_that_no_longer_resolves_is_silently_skipped() {
    // Between the background capture and this apply, the agent could have
    // been removed (killed, or already cleaned up another way). Nothing to
    // do, and nothing to crash over.
    let mut app = App::new(
        Registry::default(),
        Config::default(),
        tempfile::tempdir().unwrap().path().into(),
    );

    app.run_auto_cleanup(vec![(PathBuf::from("/gone"), "ghost".to_string())]);

    assert!(app.merge_jobs.is_empty());
}
