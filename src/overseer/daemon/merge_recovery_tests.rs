use super::*;
use crate::overseer::ledger::{LedgerPhase, MergeRecovery};

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
        manual_merge_skip: None,
    }
}

#[test]
fn every_reason_the_merge_gate_emits_resolves_to_one_class() {
    let recoverable = [
        "merge_state:dirty",
        "merge_state:blocked",
        "checks_not_green",
        "behind_update_cap_reached",
        "merge_exit:exit status: 1",
        "merge_error:timed out after 15s",
        "judge_veto:the migration has no rollback",
        "judge_escalate:the diff touches the release pipeline",
    ];
    for reason in recoverable {
        assert_eq!(
            classify(reason),
            FailureClass::Recoverable,
            "expected recoverable: {reason}"
        );
    }

    let operator = [
        "unprotected:no_pull_request_rule",
        "unprotected:no_required_status_checks",
        "unprotected:probe_unavailable",
        "unprotected:unknown_remote",
        "missing_pr_url",
        "autonomy_envelope",
        "repo_merge_settling",
        "repo_merge_settle_cap_reached",
        "repo_merge_settled",
        // Nothing has failed: the checks are still running, which is not worth a
        // worker turn.
        "checks_waiting",
        // The pull request has settled. There is no branch left for a worker to
        // push to, and reopening a closed pull request is a human act.
        "pr_already_merged",
        "pr_closed_unmerged",
        // Already the recovery: the branch was updated and re-queued.
        "behind_branch_updated",
        "behind_update_exit:exit status: 1",
        "behind_update_error:timed out",
        "check_probe_exit:exit status: 1",
        "check_parse:expected value",
    ];
    for reason in operator {
        assert_eq!(
            classify(reason),
            FailureClass::Operator,
            "expected operator-only: {reason}"
        );
    }
}

#[test]
fn an_unrecognised_reason_is_never_handed_to_a_worker() {
    // A failure mode nobody anticipated must not silently drive a worker turn.
    for reason in ["", "something_github_added_later", "merge_state:draft"] {
        assert_eq!(classify(reason), FailureClass::Operator);
    }
}

#[test]
fn recovery_stays_idle_until_it_is_switched_on() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", false, 2),
        RecoveryPlan::Idle
    );
    assert_eq!(entry.merge_recovery, MergeRecovery::default());
}

#[test]
fn an_operator_only_failure_is_never_charged() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "unprotected:unknown_remote", "sha-1", true, 2),
        RecoveryPlan::Idle
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn the_same_failure_on_the_same_head_is_handed_back_once() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
    // The next poll interval finds the same revision failing the same way; the
    // worker is already working on it.
    for reason in ["merge_state:dirty", "judge_veto:still not right"] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", true, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(entry.merge_recovery.charged, 1);
}

#[test]
fn a_new_head_resets_the_dedupe_but_never_the_budget() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    // A worker that pushes a broken fix each round would otherwise loop forever.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-3", true, 2),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-2"));
}

#[test]
fn a_zero_budget_escalates_without_ever_prompting() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "judge_veto:no", "sha-1", true, 0),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn a_failure_without_a_head_sha_is_left_alone() {
    // Without a revision there is no deduplication key, so a handback here would
    // re-prompt the worker on every pass.
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "", true, 2),
        RecoveryPlan::Idle
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn a_worker_with_no_registered_session_cannot_be_handed_anything() {
    assert_eq!(live_session("agent", &Registry::default()), None);
}

#[test]
fn a_disabled_recovery_never_reaches_a_worker() {
    // `consider` is the whole delivery path; with recovery off it must return
    // without resolving a session, sending a prompt, or writing a decision, so
    // the merge pass behaves exactly as it did before the feature existed.
    let mut entry = entry();
    let config = crate::overseer::config::OverseerConfig::default();
    assert!(!config.merge_recovery_enabled);
    consider(
        &mut entry,
        "merge_state:dirty",
        "sha-1",
        &config,
        &Registry::default(),
    )
    .unwrap();
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
    assert_eq!(entry.merge_recovery, MergeRecovery::default());
}

#[test]
fn the_prompt_reaches_the_session_as_one_submission() {
    // tmux delivers a literal newline as a submit, so a prompt sent as authored
    // would enter the worker's prompt line by line and act on the first alone.
    let prompt = crate::overseer::templates::merge_recovery_prompt(
        "#1",
        "task",
        "https://pr/1",
        "merge_state:dirty",
    );
    assert!(prompt.contains('\n'));
    let flattened = single_line(&prompt);
    assert!(!flattened.contains('\n'));
    assert!(!flattened.contains("  "));
    // Flattening must not cost the reason or the rails.
    assert!(flattened.contains("merge_state:dirty"));
    assert!(flattened.contains("Never force push"));
}

#[test]
fn every_recorded_reason_names_the_merge_recovery_step() {
    // The decision log is the only place the whole cycle is visible, so each
    // reason has to be greppable and carry what actually happened.
    assert_eq!(
        dispatched("judge_veto:no rollback"),
        "merge_recovery_dispatched:judge_veto:no rollback"
    );
    assert_eq!(
        skipped("missing_session:worker-3"),
        "merge_recovery_skipped:missing_session:worker-3"
    );
    assert_eq!(
        skipped("send_failed:tmux send-keys failed"),
        "merge_recovery_skipped:send_failed:tmux send-keys failed"
    );
    assert_eq!(CAP_REACHED, "merge_recovery_cap_reached");
}
