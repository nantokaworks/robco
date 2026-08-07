//! The probes that read state Overseer does not own: the dropr task board and
//! GitHub pull requests. Both are best-effort — a failed probe becomes a
//! logged skipped observation rather than invented state. `worth_probing` is
//! also shared with the sibling `branch_activity` module, whose own probe
//! follows the same live-or-recently-settled eligibility.

use super::super::{COMMAND_TIMEOUT, terminal};
use crate::{
    dropr::DroprOverlay,
    overseer::{
        exec::run_timeout,
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        monitor::{ObservationError, Observations, PrObservation, TASK_ABSENT, TaskObservation},
    },
};
use chrono::{DateTime, Duration, Utc};
use std::{collections::HashSet, process::Command};

/// How long the reconcile probe keeps re-checking an escalated or failed
/// entry's branch after it settled, before it gives up asking.
///
/// Every entry this admits costs one `gh pr list` per poll, and nothing but
/// retention ever removes a terminal entry whose worktree an operator leaves
/// standing (see `daemon::retention`), so an unbounded window would let a
/// stuck entry cost that call forever. Thirty days comfortably covers the
/// slowest realistic turnaround for an operator to notice an escalation and
/// land the fix — dropr task #335's own motivating examples went stale for
/// weeks, not months — while still giving every entry a point where re-asking
/// stops paying for itself.
const RECONCILE_MAX_SETTLED_AGE: Duration = Duration::days(30);

pub(super) fn gather_task_states(
    ledger: &Ledger,
    observations: &mut Observations,
    now: DateTime<Utc>,
) {
    let workspaces = DroprOverlay::load_best_effort();
    let repos: HashSet<_> = ledger
        .entries
        .iter()
        .filter(|entry| worth_probing(entry, now))
        .map(|entry| entry.repo.as_str())
        .collect();
    for repo in repos {
        gather_repo_task_states(repo, ledger, &workspaces, observations, now);
    }
}

fn gather_repo_task_states(
    repo: &str,
    ledger: &Ledger,
    workspaces: &DroprOverlay,
    observations: &mut Observations,
    now: DateTime<Utc>,
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
    let fetch = crate::dropr::fetch_repo_tasks(&workspace.id);
    if !fetch.answered {
        observations.errors.push(
            ObservationError::new(format!(
                "dropr task probe skipped: {}",
                fetch.problems.join("; ")
            ))
            .in_repo(repo),
        );
        return;
    }
    // The probe answered, but not in full: an entry whose task is in a subtree
    // the fetch could not read looks unobserved, not unchanged.
    for problem in &fetch.problems {
        observations.errors.push(
            ObservationError::new(format!("dropr task probe incomplete: {problem}")).in_repo(repo),
        );
    }
    let tasks = fetch.tasks;
    for entry in ledger
        .entries
        .iter()
        .filter(|entry| entry.repo == repo && worth_probing(entry, now))
    {
        let matches: Vec<_> = tasks
            .iter()
            .filter(|task| task.display_id == entry.display_id || task.display_id == entry.task_id)
            .collect();
        // The fetch answered in full — `fetch.problems` is checked once, above
        // — and still found no task under this entry's display id. For a live
        // entry that is an eventual-consistency blip worth ignoring; for an
        // escalated one still worth a `gh`/dropr read (`worth_probing`'s
        // thirty-day window) it is the daemon's only chance to notice the
        // task itself is gone, so `daemon::retention` has something to sweep
        // it on instead of the row sitting parked forever. See dropr:401.
        if matches.is_empty() {
            if entry.phase == LedgerPhase::Escalated && fetch.problems.is_empty() {
                observations.tasks.push(TaskObservation {
                    task_id: entry.task_id.clone(),
                    state: TASK_ABSENT.to_owned(),
                    updated_at: None,
                });
            }
            continue;
        }
        for task in matches {
            observations.tasks.push(TaskObservation {
                task_id: entry.task_id.clone(),
                state: task.status.clone(),
                updated_at: task.updated_at,
            });
        }
    }
}

const PR_FIELDS: &str = "state,statusCheckRollup,url,mergedAt";

/// Whether an entry's pull request is still worth a `gh` read.
///
/// A live entry is always read. `escalated` and `failed` are read too, within
/// `RECONCILE_MAX_SETTLED_AGE` of settling: both are a question put to an
/// operator, and the answer is very often the merge itself, landed by hand or
/// by a follow-up run. Without this the ledger had no way to learn that
/// happened, and the entry stayed frozen for as long as the daemon ran — see
/// dropr task #335.
///
/// `merged` is left alone: its cleanup has already run, and re-reading it
/// would only cost a call for an answer nothing acts on. An entry with no
/// `settled_at` — one that reached a terminal phase before the field
/// existed — has no age to compare and stays eligible; the alternative would
/// silently stop reconciling exactly the backlog this bound exists to clear.
pub(in crate::overseer::daemon) fn worth_probing(entry: &LedgerEntry, now: DateTime<Utc>) -> bool {
    if !terminal(entry.phase) {
        return true;
    }
    matches!(entry.phase, LedgerPhase::Escalated | LedgerPhase::Failed)
        && entry.settled_at.is_none_or(|settled_at| {
            now.signed_duration_since(settled_at) <= RECONCILE_MAX_SETTLED_AGE
        })
}

/// Whether an entry's pull request is identified by its branch rather than
/// its recorded `pr_url`.
///
/// A live entry's own worker keeps `pr_url` in sync with reality, so trusting
/// it is cheaper and just as correct. A terminal entry earns no such trust:
/// it escalated or failed under one recorded URL, and by the time this pass
/// re-checks it, the pull request that actually closed the work can be a
/// different number on the same branch — dropr task #335's `#283` case, where
/// the recorded PR closed unmerged and the real merge landed as a separate PR
/// on the same branch. Branch identity survives that; a specific URL does
/// not.
fn probe_by_branch(entry: &LedgerEntry) -> bool {
    terminal(entry.phase)
}

pub(super) fn gather_pr_states(
    ledger: &Ledger,
    observations: &mut Observations,
    now: DateTime<Utc>,
) {
    for entry in ledger
        .entries
        .iter()
        .filter(|entry| worth_probing(entry, now))
    {
        let observed = if probe_by_branch(entry) {
            list_branch_prs(&entry.repo, &entry.branch)
        } else {
            match &entry.pr_url {
                Some(url) => view_pr(&entry.repo, url),
                None => list_branch_prs(&entry.repo, &entry.branch),
            }
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
#[path = "external_state_tests.rs"]
mod tests;
