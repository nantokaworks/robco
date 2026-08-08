use super::*;
use crate::overseer::config::OverseerConfig;
use crate::overseer::dispatch::plan_dispatch;
use std::collections::HashMap;

fn entry(task_id: &str, retries: u32) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "auto-agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::Failed,
        dispatched_at: Utc::now(),
        settled_at: None,
        retries,
        pr_url: None,
        branch_updates: 0,
        merge_judge_primes: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
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
    }
}

#[test]
fn a_failed_spawn_still_counts_against_max_retries() {
    // The attempt is recorded before the spawn runs, so an attempt that never
    // reaches a ledger entry of its own still bounds the next pass.
    let mut ledger = Ledger::default();
    ledger.entries.push(entry("task-1", 0));

    assert_eq!(record_attempt(&mut ledger, "task-1", "#1"), 1);
    assert_eq!(ledger.entries[0].retries, 1);

    let plan = plan_dispatch(
        &OverseerConfig::default(),
        &ledger,
        &[Candidate {
            task_id: "task-1".into(),
            display_id: "#1".into(),
            title: "task".into(),
            repo: "/repo".into(),
            author: "allowed".into(),
            priority: "medium".into(),
            workspace: "workspace-1".into(),
            priority_score: None,
            status: "open".into(),
        }],
        Utc::now(),
        &HashMap::new(),
    );
    assert_eq!(plan.decisions[0].reason, "max_retries");
    assert!(!plan.decisions[0].dispatch);
}

fn candidate(display_id: &str, title: &str) -> Candidate {
    Candidate {
        task_id: "23O9SkXUps3lOIHuXvj4Z".into(),
        display_id: display_id.into(),
        title: title.into(),
        repo: "/repo".into(),
        author: "allowed".into(),
        priority: "medium".into(),
        workspace: "workspace-1".into(),
        priority_score: None,
        status: "open".into(),
    }
}

#[test]
fn a_candidate_is_named_from_its_dropr_number() {
    // Shape coverage lives with the slug itself in `naming`; what matters
    // here is that a dispatched candidate is numbered from dropr.
    assert_eq!(
        name_slug(&candidate("#295", "Add a top-level language config")).as_deref(),
        Some("295-dropr-Add-a-top-level-language-config")
    );
    assert_eq!(name_slug(&candidate("", "Add a config")), None);
}

fn other_pr(display_id: &str, url: &str) -> crate::overseer::other_prs::OtherPr {
    crate::overseer::other_prs::OtherPr {
        number: 598,
        title: "fix the bug".into(),
        author: "someone-else".into(),
        url: url.into(),
        head_ref_name: "someone-else/fix".into(),
        mergeable_state: "CLEAN".into(),
        closes_task: Some(display_id.into()),
    }
}

#[test]
fn a_pull_request_naming_the_task_names_itself_in_the_hold_reason() {
    // dropr task #354: #803 was already covered by pull request #598,
    // opened by an agent outside the overseer.
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        crate::overseer::other_prs::RepoOtherPrs {
            polled_at: Utc::now(),
            prs: vec![other_pr("#803", "https://github.com/example/repo/pull/598")],
        },
    );
    assert_eq!(
        closed_elsewhere(&other_prs, &candidate("#803", "task")),
        Some("pr_closes_task:https://github.com/example/repo/pull/598".into())
    );
}

#[test]
fn an_unrelated_pull_request_does_not_hold_the_candidate() {
    let mut other_prs = OtherPrs::default();
    other_prs.repos.insert(
        "/repo".into(),
        crate::overseer::other_prs::RepoOtherPrs {
            polled_at: Utc::now(),
            prs: vec![other_pr("#1", "https://github.com/example/repo/pull/1")],
        },
    );
    assert_eq!(
        closed_elsewhere(&other_prs, &candidate("#803", "task")),
        None
    );
    assert_eq!(
        closed_elsewhere(&OtherPrs::default(), &candidate("#803", "task")),
        None
    );
}

#[test]
fn attempts_are_counted_across_both_identifiers() {
    let mut ledger = Ledger::default();
    ledger.entries.push(entry("task-1", 0));
    // An entry recorded under the display id belongs to the same task.
    ledger.entries.push(entry("#1", 0));

    assert_eq!(record_attempt(&mut ledger, "task-1", "#1"), 2);
    assert!(ledger.entries.iter().all(|entry| entry.retries == 2));
}
