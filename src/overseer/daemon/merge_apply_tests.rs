use serde_json::json;

use super::*;
use crate::overseer::ledger::LedgerPhase;

fn entry(repo: &str) -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: repo.into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        manual_merge_skip: None,
        merge_judge_fail_safes: 0,
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        merge_hold_stuck_notified: false,
    }
}

#[test]
fn a_ready_pull_request_claims_the_head_slot_and_the_single_pull_request_path_is_unchanged() {
    let mut heads = Heads::new();
    let mut e = entry("/repo");
    let value = json!({"mergeStateStatus": "CLEAN"});
    let halt = merge_state_cleared(
        &mut e,
        "https://pr/1",
        &value,
        &Config::default(),
        &mut heads,
    );
    assert!(halt.is_none());
    // The slot is now taken: a lone pull request still behaves as it always
    // has (nothing to gate against), but the claim itself is now recorded.
    assert!(!heads.claim("/repo"));
}

#[test]
fn a_behind_pull_request_that_is_not_next_is_held_without_touching_its_branch() {
    let mut heads = Heads::new();
    assert!(heads.claim("/repo")); // an earlier pull request already claimed the head this pass
    let mut e = entry("/repo");
    let value = json!({"mergeStateStatus": "BEHIND"});
    let halt = merge_state_cleared(
        &mut e,
        "https://pr/1",
        &value,
        &Config::default(),
        &mut heads,
    )
    .expect("a behind pull request that is not next must still be held");
    assert_eq!(halt.reason, merge_queue::WAITING_TURN);
    // Not charged: waiting a turn is not the same failure `max_branch_updates` bounds.
    assert_eq!(e.branch_updates, 0);
}

#[test]
fn a_held_pull_request_never_claims_the_slot_so_it_does_not_starve_the_one_behind_it() {
    let mut heads = Heads::new();
    let mut e = entry("/repo");
    let value = json!({"mergeStateStatus": "DIRTY"});
    let halt = merge_state_cleared(
        &mut e,
        "https://pr/1",
        &value,
        &Config::default(),
        &mut heads,
    )
    .expect("a non-mergeable state must still hold under its own name");
    assert_eq!(halt.reason, "merge_state:dirty");
    // The order skips the stuck pull request: the slot is still free for
    // whichever pull request is next in queue order.
    assert!(heads.claim("/repo"));
}

#[test]
fn different_repositories_never_contend_for_the_same_slot() {
    // Uses `Ready` rather than `Behind` for both so the assertion stays a pure
    // decision check: it never has to shell out to `gh pr update-branch`, the
    // same way `merge_state::run_update` itself is exercised only through
    // `plan_update`'s decision, not by actually invoking `gh`, elsewhere in
    // this crate's tests.
    let mut heads = Heads::new();
    let mut a = entry("/repo-a");
    let mut b = entry("/repo-b");
    let value = json!({"mergeStateStatus": "CLEAN"});
    let halt_a = merge_state_cleared(
        &mut a,
        "https://pr/a",
        &value,
        &Config::default(),
        &mut heads,
    );
    let halt_b = merge_state_cleared(
        &mut b,
        "https://pr/b",
        &value,
        &Config::default(),
        &mut heads,
    );
    assert!(halt_a.is_none());
    assert!(halt_b.is_none());
    assert!(!heads.claim("/repo-a"));
    assert!(!heads.claim("/repo-b"));
}
