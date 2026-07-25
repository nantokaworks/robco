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
    let mut cache = ProtectionCache::default();
    let now = Instant::now();
    let key = cache_key("/repo", "main", ProtectionMode::Required);
    cache.remember_verified(key.clone(), now);
    assert!(cache.verified(&key, now + PROTECTION_CACHE_TTL / 2));
    assert!(!cache.verified(&key, now + PROTECTION_CACHE_TTL));
    // A different base branch or a loosened mode is a different question.
    assert!(!cache.verified(
        &cache_key("/repo", "release", ProtectionMode::Required),
        now
    ));
    assert!(!cache.verified(&cache_key("/repo", "main", ProtectionMode::Relaxed), now));
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
        retries: 0,
        pr_url: Some("https://pr/1".into()),
        branch_updates: 0,
        merge_recovery: Default::default(),
    };
    let registry = Registry::default();
    let mut cache = ProtectionCache::default();
    assert_eq!(
        unmet_condition(&entry, &registry, &mut cache, ProtectionMode::Off, "main"),
        None
    );
    assert_eq!(
        unmet_condition(
            &entry,
            &registry,
            &mut cache,
            ProtectionMode::Required,
            "main"
        ),
        Some(UNKNOWN_REMOTE)
    );
}
