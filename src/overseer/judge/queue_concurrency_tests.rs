//! Per-repository concurrency: separate slots run at once, a shared slot
//! still serializes, the global ceiling bounds the total, and a restart
//! resolves every request back to its own slot.

use super::{MergeCase, Request, queue::test_queue, tests::merge_request};
use crate::config::{Config, Profile};
use std::path::Path;

/// Two pull requests against different repositories occupy separate slots
/// and both start in the same tick — the entire point of keying the queue
/// by repository instead of running one global judge.
#[test]
fn judgments_in_different_repositories_run_concurrently() {
    let temp = tempfile::tempdir().unwrap();
    let config = sleeping_config(temp.path());
    let mut queue = test_queue(temp.path());
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-a", "task-a", "https://pr/a"))
            .unwrap()
            .is_none()
    );
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-b", "task-b", "https://pr/b"))
            .unwrap()
            .is_none()
    );
    queue.tick(&config).unwrap();
    assert_eq!(
        queue.active_len(),
        2,
        "both repositories should be judged at once"
    );
    assert_eq!(queue.pending_len(), 0);
}

/// Two pull requests against the same repository still serialize: the
/// second waits for the repository's one slot even though the global
/// concurrency ceiling would otherwise allow it to start alongside the
/// first.
#[test]
fn judgments_in_the_same_repository_still_serialize() {
    let temp = tempfile::tempdir().unwrap();
    let config = sleeping_config(temp.path());
    let mut queue = test_queue(temp.path());
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-a", "task-a", "https://pr/a"))
            .unwrap()
            .is_none()
    );
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-a", "task-c", "https://pr/c"))
            .unwrap()
            .is_none()
    );
    queue.tick(&config).unwrap();
    assert_eq!(queue.active_len(), 1, "one repository, one judge");
    assert_eq!(queue.pending_len(), 1, "the second waits for the slot");
}

/// The global ceiling caps total concurrency even when every pending
/// judgment targets a different, otherwise-open repository slot.
#[test]
fn the_concurrency_ceiling_bounds_total_active_judgments() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = sleeping_config(temp.path());
    config.overseer.max_concurrent_judges = 1;
    let mut queue = test_queue(temp.path());
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-a", "task-a", "https://pr/a"))
            .unwrap()
            .is_none()
    );
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-b", "task-b", "https://pr/b"))
            .unwrap()
            .is_none()
    );
    queue.tick(&config).unwrap();
    assert_eq!(
        queue.active_len(),
        1,
        "the ceiling bounds total concurrency"
    );
    assert_eq!(queue.pending_len(), 1);
}

/// A restart drops every session, active or waiting, back into `pending` —
/// and each still resolves to its own repository's slot on the next tick,
/// exactly as if the daemon had never stopped.
#[test]
fn a_restart_returns_every_active_judgment_to_its_own_repository_slot() {
    let temp = tempfile::tempdir().unwrap();
    let config = sleeping_config(temp.path());
    let mut queue = test_queue(temp.path());
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-a", "task-a", "https://pr/a"))
            .unwrap()
            .is_none()
    );
    assert!(
        queue
            .merge_advice(merge_case_for("/repo-b", "task-b", "https://pr/b"))
            .unwrap()
            .is_none()
    );
    queue.tick(&config).unwrap();
    assert_eq!(queue.active_len(), 2);
    drop(queue);

    let mut restarted = test_queue(temp.path());
    assert_eq!(restarted.pending_len(), 2, "both come back pending");
    restarted.tick(&config).unwrap();
    assert_eq!(
        restarted.active_len(),
        2,
        "each request still resolves to its own repository slot"
    );
}

fn merge_case_for(repo: &str, task_id: &str, pr_url: &str) -> MergeCase {
    let Request::Merge { case, .. } = merge_request() else {
        unreachable!()
    };
    MergeCase {
        task_id: task_id.into(),
        repo: repo.into(),
        pr_url: pr_url.into(),
        ..case
    }
}

fn sleeping_config(dir: &Path) -> Config {
    let script = crate::overseer::session::executable_script(dir, "sleep 30");
    Config {
        profiles: vec![Profile {
            name: "claude".into(),
            program: script.to_string_lossy().into(),
            autonomous_args: vec![],
            model: None,
            backend: None,
        }],
        ..Default::default()
    }
}
