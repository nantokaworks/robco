use super::*;
use serde_json::json;

/// `GET /repos/{owner}/{repo}/rules/branches/main` recorded from `nantokaworks/robco`, a
/// repository whose only protection is an organization ruleset.
fn ruleset_response() -> Value {
    json!([
        {"type": "deletion", "ruleset_source_type": "Organization",
         "ruleset_source": "nantokaworks", "ruleset_id": 15850785},
        {"type": "non_fast_forward", "ruleset_source_type": "Organization",
         "ruleset_source": "nantokaworks", "ruleset_id": 15850785},
        {"type": "pull_request", "parameters": {
            "required_approving_review_count": 0,
            "dismiss_stale_reviews_on_push": false,
            "required_reviewers": [],
            "require_code_owner_review": false,
            "require_last_push_approval": false,
            "required_review_thread_resolution": false,
            "allowed_merge_methods": ["merge", "squash", "rebase"]
         }, "ruleset_source_type": "Organization", "ruleset_source": "nantokaworks",
         "ruleset_id": 15850785},
        {"type": "required_status_checks", "parameters": {
            "strict_required_status_checks_policy": true,
            "do_not_enforce_on_create": false,
            "required_status_checks": [{"context": "validate / Validate"}]
         }, "ruleset_source_type": "Organization", "ruleset_source": "nantokaworks",
         "ruleset_id": 15850785}
    ])
}

/// The same endpoint for a repository with a ruleset that only forces pull requests.
fn pull_request_only_ruleset_response() -> Value {
    json!([
        {"type": "non_fast_forward", "ruleset_id": 2},
        {"type": "pull_request", "ruleset_id": 2, "parameters": {
            "required_approving_review_count": 1
        }}
    ])
}

/// `GET /repos/{owner}/{repo}/branches/main/protection` for a classically protected
/// repository.
fn classic_response() -> Value {
    json!({
        "required_pull_request_reviews": {"required_approving_review_count": 1},
        "required_status_checks": {"strict": true, "contexts": ["validate / Validate"]},
        "enforce_admins": {"enabled": false}
    })
}

#[test]
fn ruleset_protection_is_recognised() {
    assert_eq!(
        ruleset_facts(&ruleset_response()),
        ProtectionFacts {
            pull_request: true,
            status_checks: true
        }
    );
    assert_eq!(
        ruleset_facts(&ruleset_response()).unmet(ProtectionMode::Required),
        None
    );
}

#[test]
fn ruleset_without_status_check_contexts_reports_the_failing_condition() {
    let facts = ruleset_facts(&json!([
        {"type": "pull_request", "ruleset_id": 3, "parameters": {}},
        {"type": "required_status_checks", "ruleset_id": 3, "parameters": {
            "required_status_checks": [], "strict_required_status_checks_policy": false
        }}
    ]));
    assert_eq!(
        facts.unmet(ProtectionMode::Required),
        Some(NO_REQUIRED_STATUS_CHECKS)
    );
    assert_eq!(facts.unmet(ProtectionMode::Relaxed), None);
}

#[test]
fn classic_protection_still_satisfies_every_mode_it_used_to() {
    let facts = classic_facts(&classic_response());
    assert_eq!(facts.unmet(ProtectionMode::Required), None);
    assert_eq!(facts.unmet(ProtectionMode::Relaxed), None);
    // The classic 404 body and a missing check list stay refusals, as before.
    for value in [
        json!({"message": "Branch not protected"}),
        json!({"required_pull_request_reviews": {}, "required_status_checks": null}),
    ] {
        assert_eq!(
            classic_facts(&value).unmet(ProtectionMode::Required),
            Some(if value.get("required_pull_request_reviews").is_some() {
                NO_REQUIRED_STATUS_CHECKS
            } else {
                NO_PULL_REQUEST_RULE
            })
        );
    }
    // GitHub's newer `checks` array is accepted in place of `contexts`.
    assert!(
        classic_facts(&json!({
            "required_pull_request_reviews": {},
            "required_status_checks": {"checks": [{"context": "validate"}]}
        }))
        .status_checks
    );
}

#[test]
fn ruleset_endpoint_does_not_see_classic_protection_and_vice_versa() {
    // A classically-protected repository answers the rules endpoint with an empty array.
    assert_eq!(ruleset_facts(&json!([])), ProtectionFacts::default());
    // A ruleset-protected repository answers the classic endpoint with a 404 body. `gh`
    // exits non-zero there, so the probe never reaches this parse — but a body that did
    // arrive must not read as protection either.
    assert_eq!(
        classic_facts(&json!({
            "message": "Branch not protected",
            "documentation_url": "https://docs.github.com/rest/branches/branch-protection#get-branch-protection",
            "status": "404"
        })),
        ProtectionFacts::default()
    );
    // Unioning the two sources is what makes both shapes pass the gate.
    let union = ruleset_facts(&ruleset_response()).union(classic_facts(&json!({})));
    assert_eq!(union.unmet(ProtectionMode::Required), None);
}

#[test]
fn each_mode_gates_the_same_facts_differently() {
    let pull_request_only = ruleset_facts(&pull_request_only_ruleset_response());
    let unprotected = ProtectionFacts::default();
    assert_eq!(
        pull_request_only.unmet(ProtectionMode::Required),
        Some(NO_REQUIRED_STATUS_CHECKS)
    );
    assert_eq!(pull_request_only.unmet(ProtectionMode::Relaxed), None);
    assert_eq!(pull_request_only.unmet(ProtectionMode::Off), None);
    assert_eq!(
        unprotected.unmet(ProtectionMode::Required),
        Some(NO_PULL_REQUEST_RULE)
    );
    assert_eq!(
        unprotected.unmet(ProtectionMode::Relaxed),
        Some(NO_PULL_REQUEST_RULE)
    );
    assert_eq!(unprotected.unmet(ProtectionMode::Off), None);
}

#[test]
fn positive_cache_expires_and_is_scoped_to_branch_and_mode() {
    let cache = ProtectionCache::default();
    let now = Instant::now();
    let key = cache_key("/repo", "main", ProtectionMode::Required);
    cache.remember(key.clone(), CacheState::Verified, now);
    assert_eq!(
        cache.remembered(&key, now + PROTECTION_CACHE_TTL / 2),
        Some(CacheState::Verified)
    );
    assert_eq!(cache.remembered(&key, now + PROTECTION_CACHE_TTL), None);
    // A different base branch or a loosened mode is a different question.
    assert_eq!(
        cache.remembered(
            &cache_key("/repo", "release", ProtectionMode::Required),
            now
        ),
        None
    );
    assert_eq!(
        cache.remembered(&cache_key("/repo", "main", ProtectionMode::Relaxed), now),
        None
    );
}

#[test]
fn plan_unsupported_cache_outlives_the_positive_ttl() {
    let cache = ProtectionCache::default();
    let now = Instant::now();
    let key = cache_key("/repo", "main", ProtectionMode::Required);
    cache.remember(key.clone(), CacheState::PlanUnsupported, now);
    // Still cached well past the positive-verification TTL...
    assert_eq!(
        cache.remembered(&key, now + PROTECTION_CACHE_TTL * 2),
        Some(CacheState::PlanUnsupported)
    );
    // ...but not forever: a plan upgrade is still noticed eventually.
    assert_eq!(
        cache.remembered(&key, now + PLAN_UNSUPPORTED_CACHE_TTL),
        None
    );
}

#[test]
fn classify_prefers_real_facts_over_a_plan_refusal() {
    // One probe answered with real (if insufficient) facts; the other 403'd. The
    // real answer is the more specific, more actionable one.
    let facts = ruleset_facts(&pull_request_only_ruleset_response());
    assert_eq!(
        classify(facts, ProtectionMode::Required, true, true),
        Some(NO_REQUIRED_STATUS_CHECKS)
    );
}

#[test]
fn classify_reports_plan_unsupported_only_when_no_probe_answered() {
    let facts = ProtectionFacts::default();
    assert_eq!(
        classify(facts, ProtectionMode::Required, false, true),
        Some(PLAN_UNSUPPORTED)
    );
}

#[test]
fn classify_falls_back_to_probe_unavailable_without_a_plan_refusal_either() {
    let facts = ProtectionFacts::default();
    assert_eq!(
        classify(facts, ProtectionMode::Required, false, false),
        Some(PROBE_UNAVAILABLE)
    );
}

#[test]
fn classify_is_satisfied_when_the_facts_meet_the_mode() {
    let facts = ruleset_facts(&ruleset_response());
    assert_eq!(classify(facts, ProtectionMode::Required, true, false), None);
}

#[test]
fn http_status_is_read_from_ghs_failure_summary() {
    assert_eq!(
        http_status_from_stderr(
            b"gh: Upgrade to GitHub Pro or make this repository public to enable this feature. (HTTP 403)\n"
        ),
        Some(403)
    );
    assert_eq!(
        http_status_from_stderr(b"gh: Not Found (HTTP 404)\n"),
        Some(404)
    );
}

#[test]
fn http_status_is_none_for_output_with_no_status_marker() {
    assert_eq!(http_status_from_stderr(b"connection reset by peer\n"), None);
    assert_eq!(http_status_from_stderr(b""), None);
}

#[test]
fn off_mode_skips_the_probe_entirely() {
    let entry = crate::overseer::ledger::LedgerEntry {
        task_id: "task".into(),
        display_id: "#1".into(),
        // A path no registry knows, so any probe attempt would fail with `unknown_remote`.
        repo: "/nonexistent".into(),
        agent_id: "agent".into(),
        branch: "branch".into(),
        phase: crate::overseer::ledger::LedgerPhase::PrOpened,
        dispatched_at: chrono::Utc::now(),
        settled_at: None,
        retries: 0,
        pr_url: Some("https://pr/1".into()),
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
    };
    let registry = Registry::default();
    let cache = ProtectionCache::default();
    assert_eq!(
        unmet_condition(&entry, &registry, &cache, ProtectionMode::Off, "main"),
        None
    );
    assert_eq!(
        unmet_condition(&entry, &registry, &cache, ProtectionMode::Required, "main"),
        Some(UNKNOWN_REMOTE)
    );
}
