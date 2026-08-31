use chrono::Utc;

use super::*;
use crate::{
    git::test_repo::TestRepo,
    overseer::ledger::{LedgerEntry, LedgerPhase},
    ui::test_support,
};

fn merged_entry(agent_id: &str) -> LedgerEntry {
    LedgerEntry {
        task_id: "task-1".into(),
        dropr_task_id: None,
        display_id: "#1".into(),
        repo: "nantokaworks/robco".into(),
        agent_id: agent_id.into(),
        branch: "branch".into(),
        phase: LedgerPhase::Merged,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
        worker_finished_at: None,
        approval_dropped: None,
        branch_update_head: None,
    }
}

fn dead_agent(id: &str, worktree_path: std::path::PathBuf) -> crate::model::AgentNode {
    let mut agent = test_support::agent(id, worktree_path);
    agent.status = Status::Dead;
    agent
}

#[test]
fn a_clean_merged_dead_agent_is_a_cleanup_candidate() {
    let repo = TestRepo::new();
    repo.feature_branch("task-a", "a.txt");
    repo.push("task-a");
    let worktree = repo.worktree("task-a");

    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(
            repo.path().to_path_buf(),
            vec![dead_agent("worker-a", worktree)],
        )],
    };
    let mut ledger = Ledger::default();
    ledger.entries.push(merged_entry("worker-a"));

    assert_eq!(
        merged_cleanup_candidates(&registry, &ledger),
        vec![(repo.path().to_path_buf(), "worker-a".to_string())]
    );
}

#[test]
fn a_dirty_worktree_is_never_an_automatic_cleanup_candidate() {
    // dropr:563 — CleanOnly's own worktree removal force-deletes once a
    // branch is already merged, so this has to refuse before ever handing
    // it a dirty tree unattended.
    let repo = TestRepo::new();
    repo.feature_branch("task-b", "b.txt");
    repo.push("task-b");
    let worktree = repo.worktree("task-b");
    std::fs::write(worktree.join("uncommitted.txt"), "wip").unwrap();

    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(
            repo.path().to_path_buf(),
            vec![dead_agent("worker-b", worktree)],
        )],
    };
    let mut ledger = Ledger::default();
    ledger.entries.push(merged_entry("worker-b"));

    assert!(merged_cleanup_candidates(&registry, &ledger).is_empty());
}

#[test]
fn an_agent_whose_pull_request_is_not_observed_merged_is_never_a_candidate() {
    let repo = TestRepo::new();
    repo.feature_branch("task-c", "c.txt");
    repo.push("task-c");
    let worktree = repo.worktree("task-c");

    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(
            repo.path().to_path_buf(),
            vec![dead_agent("worker-c", worktree)],
        )],
    };

    assert!(merged_cleanup_candidates(&registry, &Ledger::default()).is_empty());
}

#[test]
fn a_live_agent_is_never_a_candidate_even_if_its_pull_request_merged() {
    let repo = TestRepo::new();
    repo.feature_branch("task-d", "d.txt");
    repo.push("task-d");
    let worktree = repo.worktree("task-d");

    let mut agent = test_support::agent("worker-d", worktree);
    agent.status = Status::Running;
    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(repo.path().to_path_buf(), vec![agent])],
    };
    let mut ledger = Ledger::default();
    ledger.entries.push(merged_entry("worker-d"));

    assert!(merged_cleanup_candidates(&registry, &ledger).is_empty());
}

#[test]
fn a_missing_worktree_still_counts_as_clean() {
    let temp = tempfile::tempdir().unwrap();
    let mut agent = dead_agent("worker-e", temp.path().join("gone"));
    agent.worktree_missing = true;
    let repo_path = temp.path().join("repo");
    let registry = Registry {
        version: 1,
        repos: vec![test_support::repo(repo_path.clone(), vec![agent])],
    };
    let mut ledger = Ledger::default();
    ledger.entries.push(merged_entry("worker-e"));

    assert_eq!(
        merged_cleanup_candidates(&registry, &ledger),
        vec![(repo_path, "worker-e".to_string())]
    );
}
