use serde_json::json;

use super::*;
use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
        manual_merge_skip: None,
    }
}

#[test]
fn behind_is_recoverable_while_other_states_hold_under_their_own_name() {
    assert_eq!(
        merge_state(&json!({"mergeStateStatus": "BEHIND"})),
        MergeState::Behind
    );
    for state in ["CLEAN", "HAS_HOOKS"] {
        assert_eq!(
            merge_state(&json!({ "mergeStateStatus": state })),
            MergeState::Ready
        );
    }
    for state in ["DIRTY", "BLOCKED", "UNSTABLE", "DRAFT", "UNKNOWN"] {
        assert_eq!(
            merge_state(&json!({ "mergeStateStatus": state })),
            MergeState::Held(state)
        );
    }
    // A state GitHub adds later still parks the pull request under its own name.
    let later = json!({"mergeStateStatus": "SOMETHING_NEW"});
    assert_eq!(merge_state(&later), MergeState::Held("SOMETHING_NEW"));
    assert_eq!(hold_reason("SOMETHING_NEW"), "merge_state:something_new");
    assert_eq!(hold_reason("DIRTY"), "merge_state:dirty");
}

#[test]
fn an_unreported_merge_state_does_not_park_the_pull_request() {
    assert_eq!(merge_state(&json!({})), MergeState::Ready);
    assert_eq!(
        merge_state(&json!({"mergeStateStatus": ""})),
        MergeState::Ready
    );
    assert_eq!(
        merge_state(&json!({"mergeStateStatus": 7})),
        MergeState::Ready
    );
}

#[test]
fn branch_updates_are_bounded_and_charged_before_they_run() {
    let mut config = OverseerConfig {
        max_branch_updates: 2,
        ..OverseerConfig::default()
    };
    let mut entry = entry();
    for spent in 1..=2 {
        assert_eq!(plan_update(&mut entry, &config), BehindPlan::Update(None));
        assert_eq!(entry.branch_updates, spent);
        // Falling behind is recoverable: the entry stays queued rather than failing.
        assert_eq!(entry.phase, LedgerPhase::PrOpened);
    }
    // The budget is charged at plan time, so a branch that keeps losing the race
    // escalates instead of updating forever.
    assert_eq!(plan_update(&mut entry, &config), BehindPlan::Escalate);
    assert_eq!(entry.branch_updates, 2);

    config.merge_strategy = "rebase".into();
    config.max_branch_updates = 3;
    assert_eq!(
        plan_update(&mut entry, &config),
        BehindPlan::Update(Some("--rebase"))
    );
}

#[test]
fn a_zero_budget_never_updates_a_branch() {
    let config = OverseerConfig {
        max_branch_updates: 0,
        ..OverseerConfig::default()
    };
    let mut entry = entry();
    assert_eq!(plan_update(&mut entry, &config), BehindPlan::Escalate);
    assert_eq!(entry.branch_updates, 0);
}

#[test]
fn only_a_rebase_strategy_rebases_the_update() {
    assert_eq!(update_flag("rebase"), Some("--rebase"));
    for strategy in ["squash", "merge", ""] {
        assert_eq!(update_flag(strategy), None);
    }
}
