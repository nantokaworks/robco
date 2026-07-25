use std::collections::HashMap;

use crate::{
    Result,
    model::{AgentNode, RepoNode},
    registry::Registry,
};

use super::{App, actions::discovery::path_key};

impl App {
    /// Apply `mutate` to the registry on disk under the cross-process lock and
    /// adopt the result.
    ///
    /// The TUI, the `robco` CLI, the MCP server, and the Overseer daemon all
    /// write the same state file, so writing this process's snapshot back
    /// wholesale would erase whatever another one committed in between — a
    /// worker the daemon just spawned, or a management mode another client just
    /// changed. `Registry::locked_update` avoids that by re-reading under the
    /// lock, at the cost of handing back a registry parsed from disk: every
    /// `#[serde(skip)]` runtime field on it is back at its default. Those are
    /// carried over from the in-memory registry here, so a write no longer
    /// blanks the status column until the next refresh tick.
    ///
    /// `mutate` must locate its target by identity (repo path, agent id) rather
    /// than by an index taken from `self.registry`, since the reloaded registry
    /// may hold rows another process added or removed.
    pub(in crate::ui) fn locked_registry_update<F>(&mut self, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut Registry),
    {
        let mut reloaded = Registry::locked_update(mutate)?;
        carry_runtime(&self.registry.repos, &mut reloaded.repos);
        self.expanded = realign_expanded(&self.registry.repos, &self.expanded, &reloaded.repos);
        self.registry = reloaded;
        Ok(())
    }
}

/// Move every runtime-only field from `current` onto the matching rows of a
/// registry just reloaded from disk. Repos and agents are matched by identity,
/// not position, because the reload may have picked up another process's edits.
fn carry_runtime(current: &[RepoNode], reloaded: &mut [RepoNode]) {
    for repo in reloaded {
        let Some(source) = current.iter().find(|source| source.path == repo.path) else {
            continue;
        };
        carry_repo(source, repo);
        for agent in &mut repo.agents {
            if let Some(source) = source.agents.iter().find(|source| source.id == agent.id) {
                carry_agent(source, agent);
            }
        }
    }
}

fn carry_repo(source: &RepoNode, target: &mut RepoNode) {
    target.dropr.clone_from(&source.dropr);
    target.dropr_tasks.clone_from(&source.dropr_tasks);
    target.main_status = source.main_status;
    target
        .main_last_capture
        .clone_from(&source.main_last_capture);
    target
        .main_last_spinner
        .clone_from(&source.main_last_spinner);
    target.main_last_change_at = source.main_last_change_at;
    target.main_shell_working = source.main_shell_working;
    target.main_mcp_active = source.main_mcp_active;
    target.main_pane_pid = source.main_pane_pid;
    target
        .main_tracked_command
        .clone_from(&source.main_tracked_command);
    target.main_subagents_active = source.main_subagents_active;
}

fn carry_agent(source: &AgentNode, target: &mut AgentNode) {
    target.status = source.status;
    target.worktree_missing = source.worktree_missing;
    target.merge_error.clone_from(&source.merge_error);
    target.last_capture.clone_from(&source.last_capture);
    target.last_spinner.clone_from(&source.last_spinner);
    target.last_change_at = source.last_change_at;
    target.last_auto_accept_at = source.last_auto_accept_at;
    target.shell_working = source.shell_working;
    target.mcp_active = source.mcp_active;
    target.pane_pid = source.pane_pid;
    target.tracked_command.clone_from(&source.tracked_command);
    target.subagents.clone_from(&source.subagents);
    target.children.clone_from(&source.children);
}

/// `App::expanded` is positional, so a reload that adds or drops a repo would
/// otherwise shift every collapse state onto the wrong row. Re-key the flags by
/// repo path; a repo this process has not seen before starts expanded, matching
/// how the discovery refresh treats one.
fn realign_expanded(current: &[RepoNode], expanded: &[bool], reloaded: &[RepoNode]) -> Vec<bool> {
    let by_path: HashMap<String, bool> = current
        .iter()
        .zip(expanded)
        .map(|(repo, value)| (path_key(&repo.path), *value))
        .collect();
    reloaded
        .iter()
        .map(|repo| by_path.get(&path_key(&repo.path)).copied().unwrap_or(true))
        .collect()
}

#[cfg(test)]
#[path = "registry_write_tests.rs"]
mod tests;
