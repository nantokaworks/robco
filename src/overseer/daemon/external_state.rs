//! The probes that read state Overseer does not own: the dropr task board and
//! GitHub pull requests. Both are best-effort — a failed probe becomes a logged
//! skipped observation rather than invented state.

use super::super::{COMMAND_TIMEOUT, terminal};
use crate::{
    dropr::DroprOverlay,
    overseer::{
        exec::run_timeout,
        ledger::Ledger,
        monitor::{ObservationError, Observations, PrObservation, TaskObservation},
    },
};
use std::{collections::HashSet, process::Command};

pub(super) fn gather_task_states(ledger: &Ledger, observations: &mut Observations) {
    let workspaces = DroprOverlay::load_best_effort();
    let repos: HashSet<_> = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .map(|entry| entry.repo.as_str())
        .collect();
    for repo in repos {
        gather_repo_task_states(repo, ledger, &workspaces, observations);
    }
}

fn gather_repo_task_states(
    repo: &str,
    ledger: &Ledger,
    workspaces: &DroprOverlay,
    observations: &mut Observations,
) {
    let mut command = Command::new("git");
    command.args(["-C", repo, "remote", "get-url", "origin"]);
    let output = match run_timeout(command, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            observations.errors.push(
                ObservationError::new(format!("git origin probe exited {}", output.status))
                    .in_repo(repo),
            );
            return;
        }
        Err(error) => {
            observations.errors.push(
                ObservationError::new(format!("git origin probe skipped: {error}")).in_repo(repo),
            );
            return;
        }
    };
    let origin = String::from_utf8_lossy(&output.stdout);
    let Some(workspace) = workspaces.find_by_repo_url(origin.trim()) else {
        observations
            .errors
            .push(ObservationError::new("dropr workspace not found").in_repo(repo));
        return;
    };
    let Some(tasks) = crate::dropr::fetch_repo_tasks(&workspace.id) else {
        observations
            .errors
            .push(ObservationError::new("dropr task probe skipped").in_repo(repo));
        return;
    };
    for entry in ledger
        .entries
        .iter()
        .filter(|entry| entry.repo == repo && !terminal(entry.phase))
    {
        for task in tasks
            .iter()
            .filter(|task| task.display_id == entry.display_id || task.display_id == entry.task_id)
        {
            observations.tasks.push(TaskObservation {
                task_id: entry.task_id.clone(),
                state: task.status.clone(),
            });
        }
    }
}

const PR_FIELDS: &str = "state,statusCheckRollup,url";

pub(super) fn gather_pr_states(ledger: &Ledger, observations: &mut Observations) {
    for entry in ledger.entries.iter().filter(|entry| !terminal(entry.phase)) {
        let observed = match &entry.pr_url {
            Some(url) => view_pr(&entry.repo, url),
            None => list_branch_prs(&entry.repo, &entry.branch),
        };
        match observed {
            Ok(Some(mut pr)) => {
                pr.task_id = Some(entry.task_id.clone());
                observations.prs.push(pr);
            }
            // The branch has no pull request yet. A worker spends many minutes
            // implementing before it opens one, so this is the normal case and
            // there is nothing to report.
            Ok(None) => {}
            Err(message) => observations
                .errors
                .push(ObservationError::new(message).about(&entry.task_id, &entry.repo)),
        }
    }
}

/// Reads the pull request an entry already knows the URL of.
///
/// A failure here is genuine: the entry has a pull request, so `gh` failing to
/// read it is a probe that did not work rather than a state.
fn view_pr(repo: &str, url: &str) -> std::result::Result<Option<PrObservation>, String> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "view", url, "--json", PR_FIELDS]);
    match run_timeout(command, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => serde_json::from_slice(&output.stdout)
            .map(Some)
            .map_err(|error| format!("gh PR JSON unreadable: {error}")),
        Ok(output) => Err(format!("gh PR probe exited {}", output.status)),
        Err(error) => Err(format!("gh PR probe skipped: {error}")),
    }
}

/// Looks for a pull request on an entry's branch, before one is known.
///
/// `gh pr view <branch>` exits 1 while the branch has none, which made "the
/// worker has not opened its pull request yet" indistinguishable from a real
/// `gh` failure — and wrote an error into `decisions.jsonl` on every pass for
/// every entry still being implemented. `gh pr list` reports the same absence as
/// an empty array and exit 0, which is the distinction [`crate::git::PrState`]
/// already draws with its `Absent` variant.
///
/// The states are ranked the way `git::remote::pr_state_from_list` ranks them: a
/// branch can carry several pull requests at once, an open one is still
/// mergeable, and a merge that landed outweighs an attempt that was abandoned.
fn list_branch_prs(repo: &str, branch: &str) -> std::result::Result<Option<PrObservation>, String> {
    let mut command = Command::new("gh");
    command
        .current_dir(repo)
        .args(["pr", "list", "--head", branch, "--state", "all"])
        .args(["--json", PR_FIELDS]);
    let output = match run_timeout(command, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => return Err(format!("gh PR list exited {}", output.status)),
        Err(error) => return Err(format!("gh PR list skipped: {error}")),
    };
    let prs: Vec<PrObservation> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("gh PR list JSON unreadable: {error}"))?;
    Ok(best_pr(prs))
}

fn best_pr(prs: Vec<PrObservation>) -> Option<PrObservation> {
    let rank = |pr: &PrObservation| match pr.state.to_ascii_uppercase().as_str() {
        "OPEN" => 0,
        "MERGED" => 1,
        _ => 2,
    };
    prs.into_iter().min_by_key(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(state: &str) -> PrObservation {
        PrObservation {
            state: state.into(),
            url: Some(format!("https://pr/{state}")),
            ..PrObservation::default()
        }
    }

    /// The case that used to write an error into `decisions.jsonl` on every
    /// pass: a worker is still implementing and has not opened its pull request.
    #[test]
    fn a_branch_with_no_pull_request_is_a_state_not_an_error() {
        assert_eq!(best_pr(Vec::new()), None);
    }

    #[test]
    fn an_open_pull_request_outranks_earlier_attempts() {
        let best = best_pr(vec![pr("CLOSED"), pr("OPEN"), pr("MERGED")]).unwrap();
        assert_eq!(best.state, "OPEN");
        // A merge that landed still outweighs an attempt that was abandoned, so
        // a reopened-then-closed branch does not read as unmerged.
        let best = best_pr(vec![pr("CLOSED"), pr("MERGED")]).unwrap();
        assert_eq!(best.state, "MERGED");
    }
}
