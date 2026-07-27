//! Reasons the auto-merge gate itself records.

use crate::overseer::remedy::{Move, Remedy};

pub(super) const EXACT: &[(&str, Remedy)] = &[
    (
        // `daemon::merge::judge_allows` — the merge judge would have escalated
        // or vetoed, but the autonomy envelope stopped it before it was asked.
        // There is no worker fix for a policy line, so this is the operator's
        // call to make, either once by hand or by moving the line itself.
        "autonomy_envelope",
        Remedy {
            step: Move::Merge,
            means: "the merge judge's autonomy envelope held this back, not a real veto",
            next: "merge it by hand (`m` on the agent row), or raise it with \
                   `robco overseer autonomy <level>`",
        },
    ),
    (
        // `daemon::merge::CHECKS_WAITING` — nothing has failed, the pull
        // request's checks simply have not finished yet.
        "checks_waiting",
        Remedy {
            step: Move::Watch,
            means: "the pull request's checks are still running; nothing has failed",
            next: "nothing to do until a check finishes",
        },
    ),
    (
        // `daemon::merge::CHECKS_FAILED` — a check finished and did not pass,
        // on the worker's own head.
        "checks_not_green",
        Remedy {
            step: Move::Answer,
            means: "a required check finished and failed on the worker's own head",
            next: "press Enter and tell the worker to fix the failing check and push",
        },
    ),
    (
        // `merge_state::hold_reason` for `DIRTY` — the head conflicts with its
        // base and needs a rebase or merge from the worker.
        "merge_state:dirty",
        Remedy {
            step: Move::Answer,
            means: "the branch conflicts with its base",
            next: "press Enter and tell the worker to rebase onto the base branch and push",
        },
    ),
    (
        // `merge_state::hold_reason` for `BLOCKED` — missing a required review
        // or a required check, both cleared by a fresh push.
        "merge_state:blocked",
        Remedy {
            step: Move::Answer,
            means: "the branch is missing a required review or a required check",
            next: "press Enter and tell the worker to satisfy the missing review \
                   or check and push",
        },
    ),
    (
        // `merge_state::UPDATE_CAP_REACHED` — `classify` names this
        // `Recoverable`, so it must resolve to `Answer` here too (the
        // anti-drift pin `table_agrees_with_classify_on_every_recoverable_entry`
        // in `remedy_tests.rs` holds both directions to this).
        crate::overseer::daemon::merge_state::UPDATE_CAP_REACHED,
        Remedy {
            step: Move::Answer,
            means: "the automated budget for keeping this branch caught up with \
                    its base is spent",
            next: "press Enter and tell the worker to rebase onto the base branch itself",
        },
    ),
    (
        // `daemon::merge::MISSING_PR_URL` — the ledger never learned a pull
        // request URL for this entry, so the gate has nothing to read.
        "missing_pr_url",
        Remedy {
            step: Move::Review,
            means: "the ledger has no pull-request URL recorded for this entry",
            next: "check the ledger entry and dispatch a fresh pull request if needed",
        },
    ),
    (
        // `daemon::merge_decision::CLOSED_UNMERGED` — the pull request closed
        // without merging; only a human can reopen it or start over.
        "pr_closed_unmerged",
        Remedy {
            step: Move::Retry,
            means: "the pull request was closed without merging",
            next: "reopen it on the branch, or re-dispatch the task fresh",
        },
    ),
    (
        // `daemon::merge_recovery::CAP_REACHED` — the per-entry handback
        // budget is spent; the automated path gave up on this one.
        "merge_recovery_cap_reached",
        Remedy {
            step: Move::Review,
            means: "the automated handback budget for this pull request is spent",
            next: "look at the pull request by hand; the automated handback gave up \
                   after repeated attempts",
        },
    ),
    (
        // `daemon::merge_judge_fail_safe::CAP_REACHED` — the merge judge kept
        // failing to produce a real verdict and the re-ask budget is spent.
        "merge_judge_fail_safe_cap_reached",
        Remedy {
            step: Move::Review,
            means: "the merge judge's own session kept failing and the re-ask budget is spent",
            next: "check the judge backend is reachable, then let the next pass re-ask it",
        },
    ),
];

pub(super) const PREFIX: &[(&str, Remedy)] = &[
    (
        // `daemon::merge_apply::merge_now` — `gh pr merge` exited non-zero;
        // the suffix is the raw exit status.
        "merge_exit:",
        Remedy {
            step: Move::Answer,
            means: "`gh pr merge` exited without merging",
            next: "press Enter and tell the worker to look at the failing merge and push a fix",
        },
    ),
    (
        // `daemon::merge_apply::merge_now` — the `gh` invocation itself
        // failed to run; the suffix is the process error.
        "merge_error:",
        Remedy {
            step: Move::Answer,
            means: "the merge command itself failed to run",
            next: "press Enter and tell the worker to look at the branch and push a fix",
        },
    ),
    (
        // `daemon::merge_apply` — GitHub refused the merge for a reason the
        // worker cannot clear without rewriting a published branch.
        "merge_refused:",
        Remedy {
            step: Move::Review,
            means: "GitHub declined the merge for a reason the worker cannot clear",
            next: "choose a different `merge_strategy`, or merge by hand",
        },
    ),
    (
        // `daemon::protection::unmet_condition` — the base branch or repo
        // fails a protection requirement the configured mode demands.
        "unprotected:",
        Remedy {
            step: Move::Review,
            means: "the base branch or repository fails a protection requirement",
            next: "fix the branch protection settings, or switch `protection_mode`",
        },
    ),
    (
        // `judge::completion::parse_failed` — the judge session itself failed
        // (timed out, launched wrong, exited without writing a result) and
        // said nothing about the change under review. Matched on its own
        // sentence rather than a snake_case code because that is the only
        // shape the judge ever writes it in.
        "judgment fail-safe:",
        Remedy {
            step: Move::Review,
            means: "the merge or triage judge's own session failed; this is not a real verdict",
            next: "check the judge backend is reachable, then let the next pass re-ask it",
        },
    ),
];
