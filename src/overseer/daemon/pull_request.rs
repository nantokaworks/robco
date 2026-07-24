//! Reading the pull request the auto-merge gate decides on.
//!
//! Every fact the gate uses — base branch, check rollup, merge state, and the change
//! metadata the merge judge sees — comes from one `gh pr view`, so the gate reasons about
//! a single consistent snapshot rather than several reads taken at different moments.

use std::process::Command;

use serde_json::Value;

use super::COMMAND_TIMEOUT;
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

/// Whether the pull request is open and every reported check succeeded.
///
/// The rollup always describes the current head, so a branch updated onto its base
/// reports the checks of the new head — an empty rollup while they are still being
/// created, which holds rather than merges on the previous head's result.
pub(super) fn checks_green(value: &Value) -> bool {
    if value.get("state").and_then(Value::as_str) != Some("OPEN") {
        return false;
    }
    let Some(checks) = value.get("statusCheckRollup").and_then(Value::as_array) else {
        return false;
    };
    !checks.is_empty()
        && checks.iter().all(|check| {
            check
                .get("conclusion")
                .or_else(|| check.get("state"))
                .and_then(Value::as_str)
                == Some("SUCCESS")
        })
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
    fn any_non_success_check_holds() {
        assert!(!checks_green(&json!({"state":"OPEN", "statusCheckRollup":[
            {"conclusion":"SUCCESS"}, {"conclusion":"FAILURE"}
        ]})));
        assert!(checks_green(
            &json!({"state":"OPEN", "statusCheckRollup":[{"conclusion":"SUCCESS"}]})
        ));
        // A head whose checks have not been created yet is not green.
        assert!(!checks_green(
            &json!({"state":"OPEN", "statusCheckRollup":[]})
        ));
    }
}
