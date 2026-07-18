use super::*;
use crate::overseer::autonomy::ChangeFacts;
use serde_json::json;

#[test]
fn unprotected_or_missing_status_checks_refuses() {
    assert!(!protection_allows_merge(&json!({"message": "Not Found"})));
    assert!(!protection_allows_merge(
        &json!({"required_pull_request_reviews": {}, "required_status_checks": null})
    ));
    assert!(protection_allows_merge(
        &json!({"required_pull_request_reviews": {}, "required_status_checks": {"contexts": ["test"]}})
    ));
}

#[test]
fn any_non_success_check_holds() {
    assert!(!checks_green(&json!({"state":"OPEN", "statusCheckRollup":[
        {"conclusion":"SUCCESS"}, {"conclusion":"FAILURE"}
    ]})));
    assert!(checks_green(
        &json!({"state":"OPEN", "statusCheckRollup":[{"conclusion":"SUCCESS"}]})
    ));
}

#[test]
fn positive_cache_expires_and_failures_are_not_remembered() {
    let mut cache = ProtectionCache::default();
    let now = Instant::now();
    cache.remember_probe("/repo", now, None);
    cache.remember_probe("/unprotected", now, Some(false));
    assert!(cache.0.is_empty());
    cache.remember_probe("/repo", now, Some(true));
    assert!(cache.verified("/repo", now + PROTECTION_CACHE_TTL / 2));
    assert!(!cache.verified("/repo", now + PROTECTION_CACHE_TTL));
}

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
        retries: 0,
        pr_url: Some("https://pr/1".into()),
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

#[test]
fn veto_escalates_and_cannot_be_selected_again_at_same_revision() {
    let mut entry = crate::overseer::ledger::LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        repo: "/repo".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        retries: 0,
        pr_url: Some("https://pr/1".into()),
    };
    assert!(!judgment_allows_merge(&mut entry, MergeJudgment::Veto));
    assert_eq!(entry.phase, LedgerPhase::Escalated);
    assert_ne!(entry.phase, LedgerPhase::PrOpened);
}
