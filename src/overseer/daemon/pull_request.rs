//! Reading the pull request the auto-merge gate decides on.
//!
//! Every fact the gate uses — base branch, check rollup, merge state, and the change
//! metadata the merge judge sees — comes from one `gh pr view`, so the gate reasons about
//! a single consistent snapshot rather than several reads taken at different moments.

use std::process::Command;

use serde_json::Value;

use super::COMMAND_TIMEOUT;
use super::check_rollup;
pub(super) use super::check_rollup::Checks;
use crate::overseer::exec::run_timeout;

/// Base branch used when the pull request does not report one.
pub(super) const DEFAULT_BASE_BRANCH: &str = "main";

const FIELDS: &str = "state,statusCheckRollup,title,body,files,additions,deletions,changedFiles,headRefOid,baseRefName,mergeStateStatus";

/// Reads the pull request, or the hold reason its read failed under.
pub(super) fn read(repo: &str, url: &str) -> Result<Value, String> {
    let mut view = Command::new("gh");
    view.current_dir(repo)
        .args(["pr", "view", url, "--json", FIELDS]);
    let output = match run_timeout(view, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => return Err(format!("check_probe_exit:{}", output.status)),
        Err(error) => return Err(format!("check_probe:{error}")),
    };
    serde_json::from_slice(&output.stdout).map_err(|error| format!("check_parse:{error}"))
}

/// The pull request's base branch, which is the branch whose protection actually gates
/// the merge.
pub(super) fn base_branch(value: &Value) -> &str {
    value
        .get("baseRefName")
        .and_then(Value::as_str)
        .filter(|branch| !branch.is_empty())
        .unwrap_or(DEFAULT_BASE_BRANCH)
}

/// The revision the gate decided on, or an empty string when GitHub did not report one.
///
/// It is the deduplication key for merge recovery: a failure on a head already handed
/// back is the same failure, and a new head is the worker's answer to the last one.
pub(super) fn head_sha(value: &Value) -> &str {
    value
        .get("headRefOid")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The state GitHub reports for the pull request itself, parsed in one place so
/// every reader of it agrees on what an absent field means.
fn state(value: &Value) -> &str {
    value
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// How a pull request settled, for one that is no longer open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrConclusion {
    /// It landed — by Overseer, by the TUI, or by a human on GitHub.
    Merged,
    /// It was closed without merging, and only a human can reopen it.
    Closed,
}

/// The pull request's own conclusion, when it has one.
///
/// A settled pull request is a fact rather than something to wait for: no check
/// will report on it again and no protection probe can change the answer, so the
/// gate reads this before anything else it might spend a GitHub call on.
///
/// A missing or unrecognised state is deliberately `None`. A read that did not
/// answer is not a terminal fact, and treating one as a conclusion would end an
/// entry on a field GitHub renamed.
pub(super) fn conclusion(value: &Value) -> Option<PrConclusion> {
    match state(value) {
        "MERGED" => Some(PrConclusion::Merged),
        "CLOSED" => Some(PrConclusion::Closed),
        _ => None,
    }
}

/// Classifies the check rollup.
///
/// The rollup always describes the current head, so a branch updated onto its base
/// reports the checks of the new head — an empty rollup while they are still being
/// created, which waits rather than merging on the previous head's result. What the
/// entries themselves mean is [`check_rollup::classify`]'s answer.
///
/// `Failed` and `Waiting` are kept apart because merge recovery spends a worker turn
/// on the first and must not on the second: a pull request whose checks have simply
/// not finished yet has not failed at anything.
pub(super) fn checks(value: &Value) -> Checks {
    if state(value) != "OPEN" {
        return Checks::Waiting;
    }
    let Some(rollup) = value.get("statusCheckRollup").and_then(Value::as_array) else {
        return Checks::Waiting;
    };
    check_rollup::classify(rollup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn base_branch_follows_the_pull_request_and_falls_back_to_main() {
        assert_eq!(
            base_branch(&json!({"baseRefName": "release/2026"})),
            "release/2026"
        );
        assert_eq!(
            base_branch(&json!({"baseRefName": ""})),
            DEFAULT_BASE_BRANCH
        );
        assert_eq!(base_branch(&json!({})), DEFAULT_BASE_BRANCH);
    }

    #[test]
    fn only_an_open_pull_request_reporting_a_rollup_can_be_green() {
        assert_eq!(
            checks(&json!({"state":"OPEN", "statusCheckRollup":[{"conclusion":"SUCCESS"}]})),
            Checks::Green
        );
        // A read that reported no rollup at all answered nothing about the head.
        assert_eq!(checks(&json!({"state":"OPEN"})), Checks::Waiting);
        // A head whose checks have not been created yet is not green.
        assert_eq!(
            checks(&json!({"state":"OPEN", "statusCheckRollup":[]})),
            Checks::Waiting
        );
    }

    #[test]
    fn only_a_settled_pull_request_has_a_conclusion() {
        assert_eq!(
            conclusion(&json!({"state":"MERGED"})),
            Some(PrConclusion::Merged)
        );
        assert_eq!(
            conclusion(&json!({"state":"CLOSED"})),
            Some(PrConclusion::Closed)
        );
        // An open pull request has not settled, and a read that did not answer —
        // no state at all, or one this build does not know — must keep waiting
        // rather than end the entry on a field GitHub renamed.
        for unsettled in [
            json!({"state":"OPEN"}),
            json!({}),
            json!({"state":null}),
            json!({"state":"LOCKED"}),
        ] {
            assert_eq!(conclusion(&unsettled), None);
        }
    }

    #[test]
    fn a_settled_pull_request_never_reads_green() {
        // A merged or closed pull request is not a worker's to push a fix to.
        // The gate concludes on it before asking about checks at all; this is
        // the safety rail behind that, so the rollup can never green-light a
        // pull request that has already settled.
        for settled in ["MERGED", "CLOSED"] {
            assert_eq!(
                checks(&json!({"state":settled, "statusCheckRollup":[{"conclusion":"SUCCESS"}]})),
                Checks::Waiting
            );
        }
    }
}
