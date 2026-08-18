//! `!merge <task>` / `!diff <task>`: the two Discord commands that act on a
//! pull request's ledger entry.
//!
//! Split out of `actions.rs` to keep both files under the size limit.

use std::{process::Command as ProcessCommand, sync::mpsc::Sender};

use crate::overseer::{
    exec::{COMMAND_TIMEOUT, run_timeout},
    ledger::{Ledger, LedgerEntry},
};

use super::ledger_requests::LedgerRequest;

/// Queues an operator's merge approval against its ledger entry, whatever
/// phase the entry is in.
///
/// Never merges here, in any phase. The TUI merge key and the `robco_merge`
/// MCP tool merge immediately (`git::merge_flow::MergeFlow`) because a person
/// is watching them in real time; `!merge` runs unattended inside the Discord
/// command handler, so an immediate merge there would skip the daemon's
/// per-repository merge serialization the same way a second concurrent
/// `!merge` would. Queuing instead — see [`queue_approval`] — lets the
/// daemon's own merge pass, which already serializes every other merge, land
/// this one too.
pub(super) fn merge(
    task: &str,
    user_id: &str,
    ledger_requests: &Sender<LedgerRequest>,
) -> crate::Result<String> {
    let ledger = Ledger::load()?;
    let entry = find_ledger_entry(&ledger, task)?;
    if entry.pr_url.is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{task}: no pull request recorded"),
        )
        .into());
    }
    queue_approval(ledger_requests, task, user_id)?;
    Ok(format!(
        "{task}: merge queued (phase: {})",
        entry.phase.label()
    ))
}

/// Shows what an escalated pull request changes, so the operator can decide
/// before confirming `!merge`. Read-only: no lock, no registry mutation.
pub(super) fn diff(task: &str) -> crate::Result<String> {
    let ledger = Ledger::load()?;
    let entry = find_ledger_entry(&ledger, task)?;
    let pr_url = entry.pr_url.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{task}: no pull request recorded"),
        )
    })?;
    let mut command = ProcessCommand::new("gh");
    command
        .current_dir(&entry.repo)
        .args(["pr", "diff", "--stat", pr_url]);
    let output = run_timeout(command, COMMAND_TIMEOUT)?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if text.is_empty() {
            format!("{task}: no changes")
        } else {
            text
        })
    } else {
        Err(std::io::Error::other(format!(
            "gh pr diff exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
        .into())
    }
}

/// Sends an operator approval to the daemon's ledger-request channel, the
/// same way `!skip` / `!retry` do, so the Discord thread never writes the
/// ledger directly. The daemon resolves the pull request's head sha when it
/// drains the request — see `ledger_requests::apply`'s `Approve` arm — rather
/// than trusting one taken here, so the approval names the revision the merge
/// pass is about to see.
fn queue_approval(
    ledger_requests: &Sender<LedgerRequest>,
    task: &str,
    user_id: &str,
) -> crate::Result<()> {
    ledger_requests
        .send(LedgerRequest::Approve {
            task: task.to_owned(),
            user_id: user_id.to_owned(),
        })
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "daemon ledger request channel is closed",
            )
        })?;
    Ok(())
}

fn find_ledger_entry<'a>(ledger: &'a Ledger, task: &str) -> crate::Result<&'a LedgerEntry> {
    ledger
        .entries
        .iter()
        .find(|entry| entry.task_id == task || entry.display_id == task)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("task not found: {task}"),
            )
            .into()
        })
}

#[cfg(test)]
#[path = "merge_actions_tests.rs"]
mod tests;
