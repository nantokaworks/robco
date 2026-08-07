use super::*;
use crate::overseer::autonomy::ChangeFacts;
use crate::overseer::judge::{change_facts, judgment_after_gate, merge_case};
use crate::overseer::ledger::LedgerPhase;
use serde_json::json;

#[test]
fn failing_rust_gate_never_invokes_merge_judge() {
    let mut config = Config::default();
    config.overseer.autonomy_level = crate::overseer::autonomy::AutonomyLevel::FullAuto;
    let facts = ChangeFacts {
        facts_known: true,
        ..ChangeFacts::default()
    };
    let mut calls = 0;
    for (protection, checks, candidate_facts) in [
        (false, true, facts),
        (true, false, facts),
        (true, true, ChangeFacts::default()),
    ] {
        let result = judgment_after_gate(protection, checks, &candidate_facts, &config, || {
            calls += 1;
            "called"
        });
        assert_eq!(result, None);
    }
    assert_eq!(calls, 0);
}

#[test]
fn known_low_risk_metadata_reaches_judgment_in_conservative_mode() {
    let config = Config::default();
    let facts = change_facts(
        &json!({
            "additions": 3, "deletions": 1, "changedFiles": 1,
            "files": [{"path": "docs/guide.md"}]
        }),
        0,
        0,
    );
    assert!(facts.facts_known);
    assert_eq!(
        judgment_after_gate(true, true, &facts, &config, || 7),
        Some(7)
    );
}

#[test]
fn incomplete_file_metadata_is_unknown() {
    for value in [
        json!({"additions":1,"deletions":0,"changedFiles":2,"files":[{"path":"a.rs"}]}),
        json!({"additions":1,"deletions":0,"changedFiles":1,"files":[{}]}),
    ] {
        assert!(!change_facts(&value, 0, 0).facts_known);
    }
}

#[test]
fn merge_case_saturates_additions_independently() {
    let entry = crate::overseer::ledger::LedgerEntry {
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
        worker_escalated: false,
        operator_override: None,
    };
    let value = json!({
        "headRefOid":"new-sha", "additions":u64::MAX, "deletions":u32::MAX,
        "changedFiles":1, "files":[{"path":"src/lib.rs"}]
    });
    let facts = change_facts(&value, 0, 7);
    let case = merge_case(&entry, "https://pr/1", &value);
    assert_eq!(case.additions, u32::MAX);
    assert_eq!(case.deletions, u32::MAX);
    assert_eq!(case.head_sha, "new-sha");
    assert_eq!(facts.llm_calls_today, 7);
}
