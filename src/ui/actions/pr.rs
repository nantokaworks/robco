use std::path::Path;

use crate::{
    Result, git,
    model::{RepoNode, Selection},
};

use super::{
    super::{App, Mode},
    lifecycle::resolve_agent,
};

#[derive(Debug, PartialEq, Eq)]
struct PrTarget {
    repo_path: std::path::PathBuf,
    agent_id: String,
    branch: String,
}

fn pr_target_for_selection(
    repos: &[RepoNode],
    selection: Option<Selection>,
) -> std::result::Result<PrTarget, &'static str> {
    match selection {
        Some(Selection::ChildWorktree { .. }) => {
            Err("PR request is not available for child worktrees")
        }
        Some(Selection::Agent { repo, agent }) => {
            let repo = repos.get(repo).ok_or("select an agent to request a PR")?;
            let agent = repo
                .agents
                .get(agent)
                .ok_or("select an agent to request a PR")?;
            Ok(PrTarget {
                repo_path: repo.path.clone(),
                agent_id: agent.id.clone(),
                branch: agent.branch.clone(),
            })
        }
        _ => Err("select an agent to request a PR"),
    }
}

fn require_running_pr_session(running: bool) -> std::result::Result<(), &'static str> {
    running.then_some(()).ok_or("agent session is not running")
}

fn require_no_open_pr(exists: bool) -> std::result::Result<(), &'static str> {
    (!exists).then_some(()).ok_or("PR is already open")
}

impl App {
    pub(in crate::ui) fn confirm_pr_selected(&mut self) {
        let target = match pr_target_for_selection(&self.registry.repos, self.selected_item()) {
            Ok(target) => target,
            Err(message) => {
                self.show_message(message);
                return;
            }
        };
        let Some((repo, agent_idx)) =
            resolve_agent(&self.registry.repos, &target.repo_path, &target.agent_id)
        else {
            self.show_message("agent no longer exists; PR request cancelled");
            return;
        };
        let repo_node = &self.registry.repos[repo];
        let selected = &repo_node.agents[agent_idx];
        match crate::tmux::has_session(&selected.tmux_session) {
            Ok(running) => {
                if let Err(message) = require_running_pr_session(running) {
                    self.show_message(message);
                    return;
                }
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }
        match git::pr_exists(&repo_node.path, &selected.branch) {
            Ok(exists) => {
                if require_no_open_pr(exists).is_err() {
                    self.show_message(format!("PR already open for {}", selected.branch));
                    return;
                }
                self.mode = Mode::ConfirmPr {
                    repo_path: target.repo_path,
                    agent_id: target.agent_id,
                    branch: target.branch,
                    input: self.config.pr_prompt.clone(),
                };
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(crate) fn request_pr(
        &mut self,
        path: &Path,
        id: &str,
        prompt: &str,
        send: impl FnOnce(&str, &str) -> Result<()>,
    ) -> Result<()> {
        let Some((repo, agent_idx)) = resolve_agent(&self.registry.repos, path, id) else {
            self.show_message("agent no longer exists; PR request cancelled");
            return Ok(());
        };
        let selected = &self.registry.repos[repo].agents[agent_idx];
        let session = selected.tmux_session.clone();
        let branch = selected.branch.clone();
        if let Err(err) = send(&session, prompt) {
            self.show_message(err.to_string());
            return Ok(());
        }
        self.show_message(format!("PR requested: {branch}"));
        Ok(())
    }
}

#[cfg(test)]
#[path = "pr_tests.rs"]
mod tests;
