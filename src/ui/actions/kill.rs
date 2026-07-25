use crate::{
    Error, Result, agent, git,
    model::{Selection, Status},
    registry::Registry,
};

use super::{
    super::{App, ForceKillTarget, Mode},
    lifecycle::resolve_agent,
};

const FILE_LIMIT: usize = 8;

pub(super) fn force_recoverable(error: &Error) -> bool {
    matches!(
        error,
        Error::DirtyWorktree(_)
            | Error::Command {
                context: "git worktree remove",
                ..
            }
    )
}

/// Drop `agent_id` from `repo_path`'s worktree list in the stored registry.
/// Matching by identity rather than by an index read from this process's
/// snapshot keeps the removal aimed at the right row when the locked reload
/// picks up worktrees another writer added or removed.
fn remove_agent(registry: &mut Registry, repo_path: &std::path::Path, agent_id: &str) {
    if let Some(repo) = registry
        .repos
        .iter_mut()
        .find(|repo| repo.path == repo_path)
    {
        repo.agents.retain(|agent| agent.id != agent_id);
    }
}

fn error_lines(error: &Error, worktree: Option<&std::path::Path>, force: bool) -> Vec<String> {
    let mut lines = vec![error.to_string()];
    if force {
        lines.push("Warning: force delete discards these files:".into());
        if let Some(worktree) = worktree
            && let Ok(status) = git::status_short(worktree)
        {
            let files: Vec<_> = status.lines().map(str::to_string).collect();
            let count = files.len();
            lines.extend(files.into_iter().take(FILE_LIMIT));
            if count > FILE_LIMIT {
                lines.push(format!("... and {} more", count - FILE_LIMIT));
            }
        }
    }
    lines
}

impl App {
    pub(in crate::ui) fn confirm_kill_selected(&mut self) {
        match self.selected_item() {
            Some(Selection::ChildWorktree { .. }) => {
                self.show_message("kill is not available for child worktrees");
            }
            Some(Selection::Agent { repo, agent }) => {
                let repo_node = &self.registry.repos[repo];
                let agent_node = &repo_node.agents[agent];
                if self.is_merging_agent(&repo_node.path, &agent_node.id) {
                    self.show_message("cannot kill an agent while it is merging");
                    return;
                }
                if self.registry.repos[repo].agents[agent].status == Status::BranchOnly {
                    self.mode = Mode::ConfirmDeleteBranch { repo, agent };
                } else {
                    self.mode = Mode::ConfirmKill { repo, agent };
                }
            }
            Some(Selection::Repo(repo)) => {
                let repo_node = &self.registry.repos[repo];
                if repo_node.pinned {
                    if repo_node.agents.is_empty() {
                        self.mode = Mode::ConfirmRemoveRepo {
                            path: repo_node.path.clone(),
                        };
                    } else {
                        self.show_message("remove agents first");
                    }
                }
            }
            Some(Selection::Orphan(orphan)) => {
                if let Some(orphan) = self.orphans.get(orphan) {
                    self.mode = Mode::ConfirmKillOrphan {
                        session: orphan.name.clone(),
                    };
                }
            }
            _ => {}
        }
    }

    pub(in crate::ui) fn kill_agent(&mut self, repo: usize, agent: usize) -> Result<()> {
        let Some(repo_node) = self.registry.repos.get(repo) else {
            return Ok(());
        };
        let Some(agent_node) = repo_node.agents.get(agent) else {
            return Ok(());
        };
        let target = ForceKillTarget {
            repo_path: repo_node.path.clone(),
            agent_id: agent_node.id.clone(),
        };
        self.kill_target(target, false)
    }

    pub(in crate::ui) fn force_kill(&mut self, target: ForceKillTarget) -> Result<()> {
        self.kill_target(target, true)
    }

    fn kill_target(&mut self, target: ForceKillTarget, force: bool) -> Result<()> {
        if self.is_merging_agent(&target.repo_path, &target.agent_id) {
            self.mode = Mode::Normal;
            self.show_message("cannot kill an agent while it is merging");
            return Ok(());
        }
        let Some((repo, agent_idx)) =
            resolve_agent(&self.registry.repos, &target.repo_path, &target.agent_id)
        else {
            self.mode = Mode::Normal;
            return Ok(());
        };
        let selected_repo = self.registry.repos[repo].clone();
        let selected_agent = selected_repo.agents[agent_idx].clone();
        match agent::kill_agent(&selected_repo, &selected_agent, force) {
            Ok(()) => self.finish_kill(repo, agent_idx, &selected_repo, &selected_agent),
            Err(error) => {
                let recoverable = force_recoverable(&error);
                let lines = error_lines(
                    &error,
                    recoverable.then_some(selected_agent.worktree_path.as_path()),
                    recoverable,
                );
                self.mode = Mode::ErrorDialog {
                    title: "kill failed".into(),
                    lines,
                    force_kill: recoverable.then_some(target),
                };
                Ok(())
            }
        }
    }

    fn finish_kill(
        &mut self,
        repo: usize,
        agent_idx: usize,
        selected_repo: &crate::model::RepoNode,
        selected_agent: &crate::model::AgentNode,
    ) -> Result<()> {
        if git::branch_exists(&selected_repo.path, &selected_agent.branch).unwrap_or(false) {
            // `status` is runtime-only, so there is nothing here to persist —
            // and writing the registry anyway would push this snapshot over any
            // concurrent edit for no gain. The branch row is removed for real by
            // `delete_agent_branch`.
            self.registry.repos[repo].agents[agent_idx].status = Status::BranchOnly;
            self.mode = Mode::ConfirmDeleteBranch {
                repo,
                agent: agent_idx,
            };
        } else {
            self.locked_registry_update(|registry| {
                remove_agent(registry, &selected_repo.path, &selected_agent.id);
            })?;
            self.mode = Mode::Normal;
            self.show_message(format!("killed {}", selected_agent.title));
        }
        Ok(())
    }

    pub(in crate::ui) fn delete_agent_branch(
        &mut self,
        repo: usize,
        agent_idx: usize,
    ) -> Result<()> {
        let Some(repo_node) = self.registry.repos.get(repo) else {
            return Ok(());
        };
        let Some(agent_node) = repo_node.agents.get(agent_idx) else {
            return Ok(());
        };
        if self.is_merging_agent(&repo_node.path, &agent_node.id) {
            self.mode = Mode::Normal;
            self.show_message("cannot delete a branch while its agent is merging");
            return Ok(());
        }
        let selected_repo = repo_node.clone();
        let selected_agent = agent_node.clone();
        match git::delete_branch(&selected_repo.path, &selected_agent.branch) {
            Ok(()) => {
                self.locked_registry_update(|registry| {
                    remove_agent(registry, &selected_repo.path, &selected_agent.id);
                })?;
                self.show_message(format!("deleted branch {}", selected_agent.branch));
            }
            Err(error) => {
                self.mode = Mode::ErrorDialog {
                    title: "branch delete failed".into(),
                    lines: error_lines(&error, None, false),
                    force_kill: None,
                };
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "kill_tests.rs"]
mod tests;
