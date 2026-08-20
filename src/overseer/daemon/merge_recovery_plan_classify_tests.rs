use super::*;

#[test]
fn every_reason_the_merge_gate_emits_resolves_to_one_class() {
    let recoverable = [
        "merge_state:dirty",
        "merge_state:blocked",
        "checks_not_green",
        "behind_update_cap_reached",
        "merge_exit:exit status: 1",
        "merge_error:timed out after 15s",
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
        "merge_request_stale",
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
