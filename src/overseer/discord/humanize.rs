//! Turns the daemon's code-shaped decision reasons into one short readable
//! sentence for the notification embed.
//!
//! Only reasons in the known vocabulary are mapped; everything else returns
//! `None` and the caller keeps the raw text verbatim, so a reason this table
//! has never heard of renders exactly as it did before this module existed.
//! Free-form prose wrapped in a known code prefix (see
//! [`VERBATIM_PREFIXES`]) has the wrapper stripped and the prose itself
//! becomes the sentence.

/// Reasons matched whole, each mapped to one short sentence.
const EXACT: &[(&str, &str)] = &[
    // Daemon lifecycle events (`daemon::discord_events`).
    ("task_started", "A worker started working on this task."),
    ("pr_opened", "The worker opened a pull request."),
    ("merged", "The pull request was merged."),
    ("task_failed", "The worker failed to finish this task."),
    ("task_escalated", "This task needs an operator's decision."),
    (
        "worker_blocked",
        "The worker is blocked and asked for help.",
    ),
    ("queue_drained", "The task queue is empty."),
    // Repository health watch (`daemon::repo_watch`).
    (
        "advisory_task_filed",
        "A security advisory is failing this repository's checks. A task was filed.",
    ),
    (
        "dependabot_task_filed",
        "Dependabot pull requests need a coordinated rebase. A task was filed.",
    ),
    // Merge-gate hold reasons (`daemon::merge_gate`, `merge_state`,
    // `merge_queue`).
    (
        "checks_waiting",
        "The pull request's checks are still running.",
    ),
    (
        "checks_not_green",
        "A required check failed on the pull request.",
    ),
    (
        "behind_not_next",
        "The pull request is waiting for its turn in the merge queue.",
    ),
    (
        "behind_branch_updated",
        "The branch was updated from its base, so the checks run again.",
    ),
    (
        "behind_update_cap_reached",
        "The budget for updating this branch from its base is used up.",
    ),
    (
        "merge_state:dirty",
        "The branch has conflicts with its base.",
    ),
    (
        "merge_state:blocked",
        "The branch is missing a required review or check.",
    ),
    (
        "missing_pr_url",
        "No pull request URL is recorded for this task.",
    ),
    (
        "pr_closed_unmerged",
        "The pull request was closed without merging.",
    ),
    (
        "autonomy_envelope",
        "The autonomy policy held this merge for an operator. Approve it from the TUI inbox or with `robco_approve` on Discord.",
    ),
    // Spent budgets (`merge_recovery`).
    (
        "merge_recovery_cap_reached",
        "The automated handback gave up after repeated attempts.",
    ),
    // Dispatch circuit (`dispatch::runtime`).
    (
        "candidate_circuit_open",
        "Repeated spawn failures tripped the circuit for this task.",
    ),
    // Release pipeline skip reasons (`overseer::release_pipeline::ready`).
    (
        "release_pipeline_skipped:working_tree_dirty",
        "The release checkout has uncommitted changes, so the release did not run.",
    ),
    (
        "release_pipeline_skipped:working_tree_check_failed",
        "The release checkout could not be checked for uncommitted changes.",
    ),
    (
        "release_pipeline_skipped:checkout_not_on_merged_commit",
        "The release checkout is not on the merged commit, so the release did not run.",
    ),
    (
        "release_pipeline_skipped:checkout_detached",
        "The release checkout has a detached HEAD, so the release did not run.",
    ),
    (
        "release_pipeline_skipped:checkout_branch_check_failed",
        "The release checkout's branch could not be checked.",
    ),
];

/// Reasons matched by prefix. The text after the prefix carries detail that
/// stays available in the raw code the caller shows next to the sentence.
const PREFIX: &[(&str, &str)] = &[
    // The branch name rides along after this prefix (`overseer::release_
    // pipeline::readiness::ready`), so the sentence itself names the fix
    // rather than the branch — the raw code shown next to it names the
    // branch.
    (
        "release_pipeline_skipped:checkout_not_on_main:",
        "The release checkout is on another branch, not main, so the release did not run. \
         Move the checkout back to main to unblock the release.",
    ),
    (
        "merge_hold_cap_reached:",
        "The pull request was held on the same condition too many times.",
    ),
    (
        "merge_hold_stuck:",
        "The pull request has been stuck on the same hold for hours.",
    ),
    (
        "merge_hold_recheck_exhausted:",
        "The recheck budget for this hold is used up.",
    ),
    (
        "prerequisite_unmerged:",
        "This task waits for another task's pull request to merge first.",
    ),
    (
        "claimed_elsewhere:",
        "Another agent already holds this task.",
    ),
    (
        "spawn_failed:",
        "The daemon could not start a worker for this task.",
    ),
    (
        "circuit_open:",
        "The failure circuit is open, so dispatch is stopped.",
    ),
    (
        "circuit_at_risk:",
        "Repeated failures are close to opening the circuit.",
    ),
    ("merge_exit:", "The merge command exited without merging."),
    ("merge_error:", "The merge command failed to run."),
    ("merge_refused:", "GitHub refused the merge."),
    (
        "unprotected:",
        "The base branch does not meet a protection requirement.",
    ),
    (
        "behind_update_exit:",
        "Updating the branch from its base failed.",
    ),
    (
        "behind_update_error:",
        "Updating the branch from its base failed.",
    ),
    (
        "merge_recovery_skipped:",
        "The automated handback was skipped this pass.",
    ),
    (
        "merge_recovery_disabled:",
        "This looks worker-fixable, but automated handback is turned off.",
    ),
    (
        "merge_recovery_undelivered:",
        "The handback message did not reach the worker.",
    ),
    ("session_auth_failed:", "The session could not sign in."),
];

/// Wrappers around text that is already a sentence in its own right — the
/// release pipeline's own outcome (`overseer::release_pipeline`). The inner
/// text becomes the description verbatim and the wrapped raw code stays
/// secondary.
const VERBATIM_PREFIXES: &[&str] = &["release_published:", "release_failed:"];

/// The vocabulary-table half of [`sentence`]: `EXACT` / `PREFIX` only, no
/// verbatim-prefix stripping. Unlike `sentence`'s owned `String` — which
/// mixes static vocabulary with runtime prose — this returns the table's own
/// `&'static str`, so a caller can feed it straight into `locale::t` as a
/// translation-table key. The Inbox reason display (`ui::overseer::inbox_rows`)
/// uses this rather than `sentence` for exactly that reason: wrapped prose is
/// not in the vocabulary and is never translated (see the module doc comment).
pub(crate) fn static_sentence(reason: &str) -> Option<&'static str> {
    let trimmed = reason.trim();
    if let Some((_, sentence)) = EXACT.iter().find(|(code, _)| *code == trimmed) {
        return Some(sentence);
    }
    PREFIX
        .iter()
        .find(|(prefix, _)| trimmed.starts_with(prefix))
        .map(|(_, sentence)| *sentence)
}

/// One readable sentence for a known code-shaped reason, or `None` when the
/// reason is unknown and the caller should keep it verbatim.
pub(super) fn sentence(reason: &str) -> Option<String> {
    let trimmed = reason.trim();
    if let Some(known) = static_sentence(trimmed) {
        return Some(known.into());
    }
    for prefix in VERBATIM_PREFIXES {
        if let Some(inner) = trimmed.strip_prefix(prefix) {
            let inner = inner.trim();
            if !inner.is_empty() {
                return Some(inner.into());
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "humanize_tests.rs"]
mod tests;
