use std::{path::Path, process::Command};

use serde_json::Value;

use super::{GIT_NETWORK_TIMEOUT, command_output};
use crate::{Result, exec::run_timeout, overseer::daemon::pull_request};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrChecks {
    Green,
    Waiting,
    Failed(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrCheckView {
    pub(crate) checks: PrChecks,
    pub(crate) head: String,
}

pub(crate) fn pr_checks(repo: &Path, branch: &str) -> Result<PrCheckView> {
    let mut command = Command::new("gh");
    command.current_dir(repo).args([
        "pr",
        "view",
        branch,
        "--json",
        "headRefOid,state,statusCheckRollup",
    ]);
    let output = run_timeout(command, GIT_NETWORK_TIMEOUT)?;
    let output = command_output(output, "gh pr view")?;
    pr_checks_from_view(&output)
}

fn pr_checks_from_view(json: &str) -> Result<PrCheckView> {
    let value: Value = serde_json::from_str(json)?;
    let head = value
        .get("headRefOid")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if head.is_empty() {
        return Err(crate::Error::Command {
            context: "gh pr view",
            stderr: "pull request head is empty".into(),
        });
    }
    let rollup = value
        .get("statusCheckRollup")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let checks = match pull_request::classify_checks(rollup) {
        pull_request::Checks::Green => PrChecks::Green,
        pull_request::Checks::Waiting => PrChecks::Waiting,
        pull_request::Checks::Failed => PrChecks::Failed(pull_request::failed_names(rollup)),
    };
    Ok(PrCheckView { checks, head })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_uses_the_daemon_rollup_classifier_and_names_failures() {
        let failed = r#"{"headRefOid":"abc123","state":"OPEN","statusCheckRollup":[{"name":"lint","conclusion":"FAILURE"},{"name":"test","conclusion":"SUCCESS"}]}"#;
        assert_eq!(
            pr_checks_from_view(failed).unwrap(),
            PrCheckView {
                checks: PrChecks::Failed(vec!["lint".into()]),
                head: "abc123".into(),
            }
        );
        let waiting = r#"{"headRefOid":"abc123","state":"OPEN","statusCheckRollup":[{"name":"test","conclusion":null}]}"#;
        assert_eq!(
            pr_checks_from_view(waiting).unwrap().checks,
            PrChecks::Waiting
        );
        let green = r#"{"headRefOid":"abc123","state":"OPEN","statusCheckRollup":[{"name":"test","conclusion":"SUCCESS"}]}"#;
        assert_eq!(pr_checks_from_view(green).unwrap().checks, PrChecks::Green);
    }
}
