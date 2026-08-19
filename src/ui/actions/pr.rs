use std::path::Path;

use crate::{
    Result,
    locale::{fmt, t},
    model::{RepoNode, Selection},
};

use super::{super::App, lifecycle::resolve_agent, pr_precheck::PrPrecheckRequest};

#[cfg(test)]
use super::super::Mode;

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

impl App {
    pub(in crate::ui) fn confirm_pr_selected(&mut self) {
        let target = match pr_target_for_selection(&self.registry.repos, self.selected_item()) {
            Ok(target) => target,
            Err(message) => {
                self.show_message(t(self.locale, message));
                return;
            }
        };
        let Some((repo, agent_idx)) =
            resolve_agent(&self.registry.repos, &target.repo_path, &target.agent_id)
        else {
            self.show_message(t(
                self.locale,
                "agent no longer exists; PR request cancelled",
            ));
            return;
        };
        let repo_node = &self.registry.repos[repo];
        let selected = &repo_node.agents[agent_idx];
        let display_id = self.task_display_id(selected);
        self.open_pr_dialog_with_precheck(PrPrecheckRequest {
            repo_path: repo_node.path.clone(),
            agent_id: target.agent_id,
            branch: target.branch,
            tmux_session: selected.tmux_session.clone(),
            worktree_path: selected.worktree_path.clone(),
            title: selected.title.clone(),
            display_id,
            approval_head: None,
        });
    }

    pub(crate) fn request_pr(
        &mut self,
        path: &Path,
        id: &str,
        prompt: &str,
        approval_head: Option<String>,
        send: impl FnOnce(&str, &str) -> Result<()>,
    ) -> Result<()> {
        let Some((repo, agent_idx)) = resolve_agent(&self.registry.repos, path, id) else {
            self.show_message(t(
                self.locale,
                "agent no longer exists; PR request cancelled",
            ));
            return Ok(());
        };
        let selected = &self.registry.repos[repo].agents[agent_idx];
        let session = selected.tmux_session.clone();
        let branch = selected.branch.clone();
        if let Err(err) = send(&session, prompt) {
            self.show_message(err.to_string());
            return Ok(());
        }
        if let Some(head) = approval_head {
            self.queue_merge_approval(id, head, true);
        } else {
            self.show_message(fmt(self.locale, "PR requested: {}", &[&branch]));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "pr_tests.rs"]
mod tests;
