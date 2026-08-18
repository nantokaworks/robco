use super::*;

fn entry(task_id: &str, phase: LedgerPhase) -> LedgerEntry {
    LedgerEntry {
        task_id: task_id.into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
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
        merge_approval: None,
    }
}

#[test]
fn an_entry_that_reached_merged_this_pass_gets_a_release_check() {
    // `merge_apply::merge_now` and `merge_decision::concluded` both set
    // `entry.phase = Merged` directly, entirely outside
    // `monitor::reconcile_entry` — the only other place this action is
    // pushed. Without this diff, a pull request the auto-merge pass itself
    // merges never reaches `overseer::release_pipeline`: a `[release]`-scoped
    // merge like this one would leave no decision-log entry at all.
    let before = vec![entry("task", LedgerPhase::PrOpened)];
    let after = vec![entry("task", LedgerPhase::Merged)];
    assert_eq!(
        newly_merged(&before, &after),
        vec![Action::CheckReleasePipeline {
            task_id: "task".into(),
            repo: "/repo".into(),
            pr_url: Some("https://pr/1".into()),
        }]
    );
}

#[test]
fn an_entry_already_merged_before_this_pass_gets_no_repeat_check() {
    // The retried-cleanup path: an entry already `Merged` going into the
    // pass must not ask `overseer::release_pipeline` to reconsider the same
    // pull request on every pass its worktree removal happens to still be
    // pending — the same reasoning `monitor::reconcile_entry`'s own check
    // follows for the reconcile pass.
    let before = vec![entry("task", LedgerPhase::Merged)];
    let after = vec![entry("task", LedgerPhase::Merged)];
    assert!(newly_merged(&before, &after).is_empty());
}

#[test]
fn an_entry_that_stays_short_of_merged_gets_no_check() {
    let before = vec![entry("task", LedgerPhase::PrOpened)];
    let after = vec![entry("task", LedgerPhase::Escalated)];
    assert!(newly_merged(&before, &after).is_empty());
}

#[test]
fn only_the_entry_that_merged_gets_a_check() {
    let before = vec![
        entry("merged-task", LedgerPhase::PrOpened),
        entry("held-task", LedgerPhase::PrOpened),
    ];
    let after = vec![
        entry("merged-task", LedgerPhase::Merged),
        entry("held-task", LedgerPhase::PrOpened),
    ];
    assert_eq!(
        newly_merged(&before, &after),
        vec![Action::CheckReleasePipeline {
            task_id: "merged-task".into(),
            repo: "/repo".into(),
            pr_url: Some("https://pr/1".into()),
        }]
    );
}
