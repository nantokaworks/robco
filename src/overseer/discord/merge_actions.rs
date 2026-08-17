//! `!merge <task>` / `!diff <task>`: the two Discord commands that act on an
//! escalated pull request's ledger entry.
//!
//! Split out of `actions.rs` to keep both files under the size limit, and
//! because the merge path pulls in a distinct set of dependencies
//! ([`MergeFlow`], [`Registry`]) that the rest of `actions.rs` does not need.

use std::process::Command as ProcessCommand;

use crate::{
    agent,
    config::Config,
    git::merge_flow::{MergeFlow, MergeMode},
    model::{AgentNode, RepoNode},
    overseer::{
        exec::{COMMAND_TIMEOUT, run_timeout},
        ledger::{Ledger, LedgerEntry, LedgerPhase},
    },
    registry::Registry,
};

/// Merges an escalated pull request's ledger entry, reusing the exact
/// sequence the TUI merge key and the `robco_merge` MCP tool run
/// ([`MergeFlow`]) so a merge landed from Discord ends in the same state.
/// Restricted to `Escalated` entries: every other phase already has an
/// automated path to `Merged`, and widening this to any task would blur
/// the one case Discord's `!merge` exists for — a pull request the
/// automated gate could not land on its own.
pub(super) fn merge(task: &str) -> crate::Result<String> {
    let ledger = Ledger::load()?;
    let entry = find_ledger_entry(&ledger, task)?;
    if entry.phase != LedgerPhase::Escalated {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{task}: not escalated (phase: {})", entry.phase.label()),
        )
        .into());
    }
    let registry = Registry::load()?;
    let (repo, agent_node) = find_agent(&registry, &entry.agent_id)?;
    let strategy = Config::load()?.merge_strategy;
    let mut steps = Vec::new();
    MergeFlow {
        repo: &repo.path,
        branch: &agent_node.branch,
        worktree: &agent_node.worktree_path,
        tmux_session: &agent_node.tmux_session,
        shell_session: &agent::shell_session_name(agent_node),
        mode: MergeMode::MergeThenClean,
        strategy,
        source: "discord",
    }
    .run(|step| steps.push(step))?;
    forget_agent(&entry.agent_id);
    Ok(format!("{task}: merged ({})", steps.join(" -> ")))
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

fn find_agent<'a>(
    registry: &'a Registry,
    agent_id: &str,
) -> crate::Result<(&'a RepoNode, &'a AgentNode)> {
    registry
        .repos
        .iter()
        .find_map(|repo| {
            repo.agents
                .iter()
                .find(|agent| agent.id == agent_id)
                .map(|agent| (repo, agent))
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("agent_id not found: {agent_id}"),
            )
            .into()
        })
}

/// Drops the merged worker's registry row, the same cleanup
/// `mcp/tools/merge.rs::forget_agent` does after an MCP-issued merge.
/// Best effort: the pull request is already merged and the branch already
/// gone by the time this runs, so a failure here must not be reported as a
/// failed merge. The Overseer's next reconcile pass drops rows for merged
/// workers anyway.
fn forget_agent(agent_id: &str) {
    let _ = Registry::locked_update(|registry| {
        for repo in &mut registry.repos {
            repo.agents.retain(|agent| agent.id != agent_id);
        }
    });
}

#[cfg(test)]
#[path = "merge_actions_tests.rs"]
mod tests;
