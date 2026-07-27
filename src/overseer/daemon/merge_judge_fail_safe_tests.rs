use super::*;
use crate::overseer::judge::MergeJudgment;

fn entry() -> LedgerEntry {
    LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
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
    }
}

fn config(max: u32) -> OverseerConfig {
    OverseerConfig {
        max_merge_judge_fail_safes: max,
        ..OverseerConfig::default()
    }
}

fn fail_safe(reason: &str) -> MergeAdvice {
    MergeAdvice {
        outcome: MergeJudgment::Escalate,
        reason: reason.into(),
        fail_safe: true,
        ignored_fields: Vec::new(),
    }
}

fn real(outcome: MergeJudgment, reason: &str) -> MergeAdvice {
    MergeAdvice {
        outcome,
        reason: reason.into(),
        fail_safe: false,
        ignored_fields: Vec::new(),
    }
}

#[test]
fn a_fail_safe_verdict_re_asks_within_budget_without_escalating() {
    let mut entry = entry();
    let config = config(3);
    for expected in [1, 2] {
        let advice = fail_safe("judgment fail-safe: session exited without result.json");
        assert!(handle(&mut entry, &advice, &config).unwrap());
        assert_eq!(entry.merge_judge_fail_safes, expected);
        assert_eq!(entry.phase, LedgerPhase::PrOpened);
    }
}

#[test]
fn a_judge_that_never_recovers_escalates_for_real() {
    let mut entry = entry();
    let config = config(3);
    let advice = fail_safe("judgment fail-safe: session timed out");
    for _ in 0..3 {
        assert!(handle(&mut entry, &advice, &config).unwrap());
    }
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
    // The fourth fail-safe verdict spends the budget rather than being re-asked
    // once more: a permanently broken judge must still converge.
    assert!(handle(&mut entry, &advice, &config).unwrap());
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_eq!(entry.merge_judge_fail_safes, 3);
}

#[test]
fn a_zero_budget_escalates_on_the_first_fail_safe_verdict() {
    let mut entry = entry();
    let advice = fail_safe("judgment fail-safe: session timed out");
    assert!(handle(&mut entry, &advice, &config(0)).unwrap());
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_eq!(entry.merge_judge_fail_safes, 0);
}

#[test]
fn a_real_verdict_resets_the_count_a_broken_judge_left_behind() {
    let mut entry = entry();
    let config = config(3);
    let advice = fail_safe("judgment fail-safe: session timed out");
    handle(&mut entry, &advice, &config).unwrap();
    handle(&mut entry, &advice, &config).unwrap();
    assert_eq!(entry.merge_judge_fail_safes, 2);

    assert!(!handle(&mut entry, &real(MergeJudgment::Allow, "reviewed"), &config).unwrap());
    assert_eq!(entry.merge_judge_fail_safes, 0);
}
