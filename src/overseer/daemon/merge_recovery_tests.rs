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
        "unprotected:plan_unsupported",
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

/// The pass that runs `gh pr update-branch` must leave the entry in `pr_opened`.
///
/// `auto_merge_pass` gives a repository's head-of-queue slot back when an entry
/// reaches a terminal phase during the pass, so the pull request behind it can
/// act immediately. If a branch update could turn its own entry terminal, that
/// release would fire on the pass that just spent a check run — and the next
/// pull request would update its branch too, which is exactly the wasted CI the
/// merge queue exists to prevent. Recovery is the only step between the update
/// and the end of the pass that can move a phase, and it must stay inert here.
#[test]
fn a_branch_update_never_turns_its_own_entry_terminal() {
    let mut entry = entry();
    for reason in [
        crate::overseer::daemon::merge_state::BRANCH_UPDATED,
        "behind_update_exit:exit status: 1",
        "behind_update_error:timed out",
    ] {
        assert_eq!(classify(reason), FailureClass::Operator, "{reason}");
        assert_eq!(
            plan(&mut entry, reason, "head", "base", true, 2),
            RecoveryPlan::Idle,
            "{reason}"
        );
        assert_eq!(entry.phase, LedgerPhase::PrOpened, "{reason}");
    }
}

#[test]
fn an_unrecognised_reason_is_never_handed_to_a_worker() {
    // A failure mode nobody anticipated must not silently drive a worker turn.
    for reason in ["", "something_github_added_later", "merge_state:draft"] {
        assert_eq!(classify(reason), FailureClass::Operator);
    }
}

/// Switched off, the classification was inert *and* invisible: nothing was handed
/// back and nothing said so, which is why an operator reading `merge-recovery:
/// off` had no way to tell what the setting was costing them.
#[test]
fn a_disabled_recovery_records_the_failure_it_did_not_hand_back() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", false, 2),
        RecoveryPlan::Dropped
    );
    assert_eq!(entry.merge_recovery.dropped, 1);
    assert_eq!(entry.merge_recovery.dropped_head.as_deref(), Some("sha-1"));
    assert_eq!(entry.merge_recovery.dropped_base.as_deref(), Some("base-1"));
    // Nothing is charged and no worker is chosen: the switch still decides
    // whether anything happens, only the silence is gone.
    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);

    // One decision per revision, not one per poll. A new head is a new failure.
    for reason in ["merge_state:dirty", "judge_veto:still not right"] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", "base-1", false, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", "base-1", false, 2),
        RecoveryPlan::Dropped
    );
    assert_eq!(entry.merge_recovery.dropped, 2);

    // A base that moved under the same head is a new failure too, the same way
    // it is when recovery is switched on.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", "base-2", false, 2),
        RecoveryPlan::Dropped
    );
    assert_eq!(entry.merge_recovery.dropped, 3);
}

/// The drop is a consequence of the setting, so a failure no worker could fix
/// stays silent whether recovery is on or off.
#[test]
fn a_disabled_recovery_records_nothing_for_an_operator_only_failure() {
    let mut entry = entry();
    for reason in [
        "unprotected:unknown_remote",
        "checks_waiting",
        "missing_pr_url",
    ] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", "base-1", false, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(entry.merge_recovery, MergeRecovery::default());
}

/// The drops the disabled setting accumulated are what `robco overseer status`
/// reports next to the switch, terminal entries included: an entry that escalated
/// because nobody was handed its failure is the case worth reading.
#[test]
fn the_ledger_totals_the_failures_the_switch_dropped() {
    let with_drops = |dropped: u32, phase| LedgerEntry {
        phase,
        merge_recovery: MergeRecovery {
            dropped,
            ..MergeRecovery::default()
        },
        ..entry()
    };
    let ledger = crate::overseer::ledger::Ledger {
        entries: vec![
            with_drops(2, LedgerPhase::PrOpened),
            with_drops(0, LedgerPhase::PrOpened),
            with_drops(3, LedgerPhase::Escalated),
        ],
        ..Default::default()
    };

    assert_eq!(ledger.merge_recovery_drops(), 5);
}

#[test]
fn an_operator_only_failure_is_never_charged() {
    let mut entry = entry();
    assert_eq!(
        plan(
            &mut entry,
            "unprotected:unknown_remote",
            "sha-1",
            "base-1",
            true,
            2
        ),
        RecoveryPlan::Idle
    );
    assert_eq!(entry.merge_recovery.charged, 0);
}

#[test]
fn the_same_failure_on_the_same_head_is_handed_back_once() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
    // The next poll interval finds the same revision failing the same way against
    // the same base; the worker is already working on it.
    for reason in ["merge_state:dirty", "judge_veto:still not right"] {
        assert_eq!(
            plan(&mut entry, reason, "sha-1", "base-1", true, 2),
            RecoveryPlan::Idle
        );
    }
    assert_eq!(entry.merge_recovery.charged, 1);
}

#[test]
fn a_new_head_resets_the_dedupe_but_never_the_budget() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-2", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    // A worker that pushes a broken fix each round would otherwise loop forever.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-3", "base-1", true, 2),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-2"));
    assert_eq!(entry.merge_recovery.base.as_deref(), Some("base-1"));
}

/// #368: the live incident this test exists to catch. A pull request was
/// mergeable when its handback was delivered, and the same head then conflicted
/// again only because a later merge advanced the base branch — the worker never
/// touched anything. The dedup key must not mistake a stationary head for a
/// steady state when the base underneath it moved.
#[test]
fn a_moved_base_resets_the_dedupe_but_never_the_budget() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    // The head never moved, but a later merge to the base branch left it
    // conflicting again — a genuinely new failure the worker was never told
    // about.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-2", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    // A busy base that keeps moving would otherwise re-arm the handback forever.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-3", true, 2),
        RecoveryPlan::CapReached
    );
    assert_eq!(entry.merge_recovery.charged, 2);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-1"));
    assert_eq!(entry.merge_recovery.base.as_deref(), Some("base-2"));
}

#[test]
fn a_zero_budget_escalates_without_ever_prompting() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "judge_veto:no", "sha-1", "base-1", true, 0),
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
        plan(&mut entry, "merge_state:dirty", "", "base-1", true, 2),
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
    // `plan` is what selects the delivery path, and with recovery off no reason
    // and no revision may select it: the switch still decides whether a worker is
    // ever driven, and only `Dispatch` resolves a session or sends a prompt.
    let config = crate::overseer::config::OverseerConfig::default();
    assert!(!config.merge_recovery_enabled);
    let mut entry = entry();
    for reason in [
        "merge_state:dirty",
        "judge_veto:no rollback",
        "unprotected:unknown_remote",
    ] {
        for head in ["sha-1", "sha-2", ""] {
            for base in ["base-1", "base-2", ""] {
                assert!(matches!(
                    plan(
                        &mut entry,
                        reason,
                        head,
                        base,
                        false,
                        config.max_merge_recoveries
                    ),
                    RecoveryPlan::Idle | RecoveryPlan::Dropped
                ));
            }
        }
    }
    assert_eq!(entry.phase, LedgerPhase::PrOpened);
    assert_eq!(entry.merge_recovery.charged, 0);
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
        None,
    );
    assert!(prompt.contains('\n'));
    let flattened = crate::tmux::single_line(&prompt);
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
    assert_eq!(
        disabled("merge_state:dirty"),
        "merge_recovery_disabled:merge_state:dirty"
    );
    assert_eq!(
        undelivered("judge_veto:no rollback"),
        "merge_recovery_undelivered:judge_veto:no rollback"
    );
    assert_eq!(CAP_REACHED, "merge_recovery_cap_reached");
}

/// The exact bug this module exists to catch: `send` reports success (tmux's
/// `send-keys` exits 0 either way — it types keys into a pane, it does not
/// know whether the receiving program acted on them), but the session never
/// shows it started a turn. The charge `plan` took on the way in must not
/// survive that outcome, and the dedup key must clear so the same head is a
/// candidate again on the next pass instead of being permanently marked as
/// already handled.
#[test]
fn an_unconfirmed_delivery_refunds_the_charge_and_clears_the_dedupe_key() {
    let mut entry = entry();
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
    assert_eq!(entry.merge_recovery.head.as_deref(), Some("sha-1"));
    assert_eq!(entry.merge_recovery.base.as_deref(), Some("base-1"));

    refund(&mut entry);

    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);
    assert_eq!(entry.merge_recovery.base, None);
    // Un-charged and un-deduped: the same (head, base) pair is a fresh candidate
    // again.
    assert_eq!(
        plan(&mut entry, "merge_state:dirty", "sha-1", "base-1", true, 2),
        RecoveryPlan::Dispatch
    );
    assert_eq!(entry.merge_recovery.charged, 1);
}

#[test]
fn refund_never_underflows_a_charge_that_was_never_taken() {
    let mut entry = entry();
    refund(&mut entry);
    assert_eq!(entry.merge_recovery.charged, 0);
    assert_eq!(entry.merge_recovery.head, None);
    assert_eq!(entry.merge_recovery.base, None);
}
