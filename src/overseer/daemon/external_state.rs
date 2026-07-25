//! The probes that read state Overseer does not own: the dropr task board and
//! GitHub pull requests. Both are best-effort — a failed probe becomes a logged
//! skipped observation rather than invented state.

use super::super::{COMMAND_TIMEOUT, terminal};
use crate::{
    dropr::DroprOverlay,
    overseer::{
        exec::run_timeout,
        ledger::Ledger,
        monitor::{Observations, PrObservation, TaskObservation},
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
            observations.errors.push(format!(
                "git origin probe exited {} for {repo}",
                output.status
            ));
            return;
        }
        Err(error) => {
            observations
                .errors
                .push(format!("git origin probe skipped for {repo}: {error}"));
            return;
        }
    };
    let origin = String::from_utf8_lossy(&output.stdout);
    let Some(workspace) = workspaces.find_by_repo_url(origin.trim()) else {
        observations
            .errors
            .push(format!("dropr workspace not found for {repo}"));
        return;
    };
    let fetch = crate::dropr::fetch_repo_tasks(&workspace.id);
    if !fetch.answered {
        observations.errors.push(format!(
            "dropr task probe skipped for {repo}: {}",
            fetch.problems.join("; ")
        ));
        return;
    }
    // The probe answered, but not in full: an entry whose task is in a subtree
    // the fetch could not read looks unobserved, not unchanged.
    for problem in &fetch.problems {
        observations
            .errors
            .push(format!("dropr task probe incomplete for {repo}: {problem}"));
    }
    let tasks = fetch.tasks;
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

pub(super) fn gather_pr_states(ledger: &Ledger, observations: &mut Observations) {
    for entry in ledger.entries.iter().filter(|entry| !terminal(entry.phase)) {
        let mut command = Command::new("gh");
        let selector = entry.pr_url.as_deref().unwrap_or(&entry.branch);
        command.current_dir(&entry.repo).args([
            "pr",
            "view",
            selector,
            "--json",
            "state,statusCheckRollup,url",
        ]);
        match run_timeout(command, COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                match serde_json::from_slice::<PrObservation>(&output.stdout) {
                    Ok(mut pr) => {
                        pr.task_id = Some(entry.task_id.clone());
                        observations.prs.push(pr);
                    }
                    Err(error) => observations
                        .errors
                        .push(format!("gh PR JSON skipped: {error}")),
                }
            }
            Ok(output) => observations
                .errors
                .push(format!("gh PR probe exited {}", output.status)),
            Err(error) => observations
                .errors
                .push(format!("gh PR probe skipped: {error}")),
        }
    }
}
