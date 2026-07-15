use crate::{
    Result, git,
    model::{RepoNode, Selection, Status},
};
use std::path::Path;

use super::super::{App, Mode};

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

pub(super) fn resolve_agent(
    repos: &[RepoNode],
    repo_path: &Path,
    agent_id: &str,
) -> Option<(usize, usize)> {
    let repo = repos.iter().position(|repo| repo.path == repo_path)?;
    let agent = repos[repo]
        .agents
        .iter()
        .position(|agent| agent.id == agent_id)?;
    Some((repo, agent))
}

fn set_merge_error(error: &mut Option<String>, detail: &str) {
    *error = Some(detail.to_string());
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

    pub(in crate::ui) fn remove_pinned_repo(&mut self, path: &Path) -> Result<()> {
        let Some(repo) = self
            .registry
            .repos
            .iter()
            .position(|repo| repo.path == path)
        else {
            self.show_message("repository changed, not removed");
            return Ok(());
        };
        if !self.registry.repos[repo].pinned || !self.registry.repos[repo].agents.is_empty() {
            self.show_message("repository changed, not removed");
            return Ok(());
        }

        let removed = self.registry.repos.remove(repo);
        if repo < self.expanded.len() {
            self.expanded.remove(repo);
        }
        self.registry.save()?;
        self.clamp_selection();
        self.show_message(format!("removed {}", removed.name));
        Ok(())
    }

    pub(in crate::ui) fn merge_selected(&mut self) {
        if matches!(self.selected_item(), Some(Selection::ChildWorktree { .. })) {
            self.show_message("merge is not available for child worktrees");
            return;
        }
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return;
        };

        self.registry.repos[repo].agents[agent_idx].merge_error = None;
        let repo_node = self.registry.repos[repo].clone();
        let selected = repo_node.agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.show_message(format!("branch remains: {}", selected.branch));
            return;
        }

        match git::worktree_is_clean(&selected.worktree_path) {
            Ok(true) => {}
            Ok(false) => {
                self.show_message("commit or clean untracked changes before merge");
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }

        match git::pr_exists(&repo_node.path, &selected.branch) {
            Ok(true) => {
                self.mode = Mode::ConfirmMerge {
                    repo,
                    agent: agent_idx,
                }
            }
            Ok(false) => {
                self.show_message(format!(
                    "no open PR for {}; create a PR first",
                    selected.branch
                ));
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }

    pub(in crate::ui) fn record_merge_error(
        &mut self,
        repo: usize,
        agent_idx: usize,
        detail: String,
    ) {
        set_merge_error(
            &mut self.registry.repos[repo].agents[agent_idx].merge_error,
            &detail,
        );
        self.show_message(detail);
    }

    pub(in crate::ui) fn add_repo_path(&mut self, path: &str) {
        let path = std::path::PathBuf::from(path);
        if !crate::discover::is_git_repo(&path) {
            self.show_message("path is not a git repository");
            return;
        }

        let Ok(path) = path.canonicalize() else {
            self.show_message("could not resolve path");
            return;
        };

        if self.registry.repos.iter().any(|repo| repo.path == path) {
            self.show_message("repository already listed");
            return;
        }

        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
            .to_string();
        let remote_url = crate::git::remote_url(&path).ok();
        self.registry.repos.push(crate::model::RepoNode {
            path,
            name,
            remote_url,
            pinned: true,
            agents: Vec::new(),
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        });
        self.expanded.push(true);
        if let Err(err) = self.registry.save() {
            self.show_message(err.to_string());
        } else {
            self.show_message("repository added");
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Local;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::{config::Config, model::AgentNode, registry::Registry};

    fn agent(id: &str) -> AgentNode {
        let now = Local::now();
        AgentNode {
            id: id.to_string(),
            parent_agent_id: None,
            title: id.to_string(),
            worktree_path: format!("/worktrees/{id}").into(),
            branch: format!("feature/{id}"),
            base_commit: String::new(),
            program: "codex".to_string(),
            profile: None,
            tmux_session: format!("robco_{id}"),
            created_at: now,
            updated_at: now,
            status: Status::Running,
            worktree_missing: false,
            merge_error: None,
            last_capture: None,
            last_change_at: None,
            last_auto_accept_at: None,
            shell_working: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        }
    }

    fn repo(path: &str, agents: Vec<AgentNode>) -> RepoNode {
        RepoNode {
            path: path.into(),
            name: path.to_string(),
            remote_url: None,
            pinned: false,
            agents,
            dropr: None,
            dropr_tasks: Vec::new(),
            main_status: None,
            main_last_capture: None,
            main_last_change_at: None,
            main_shell_working: false,
            main_pane_pid: None,
            main_tracked_command: None,
            main_subagents_active: 0,
        }
    }

    #[test]
    fn pr_request_guards_invalid_targets_and_state() {
        let repos = vec![repo("/repo", vec![agent("one")])];

        assert_eq!(
            pr_target_for_selection(&repos, Some(Selection::Repo(0))),
            Err("select an agent to request a PR")
        );
        assert_eq!(
            pr_target_for_selection(
                &repos,
                Some(Selection::ChildWorktree {
                    repo: 0,
                    agent: 0,
                    child: 0,
                }),
            ),
            Err("PR request is not available for child worktrees")
        );
        assert_eq!(
            require_running_pr_session(false),
            Err("agent session is not running")
        );
        assert_eq!(require_no_open_pr(true), Err("PR is already open"));
        assert!(require_running_pr_session(true).is_ok());
        assert!(require_no_open_pr(false).is_ok());
    }

    #[test]
    fn confirm_pr_enter_forwards_edited_prompt_to_request() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.registry.repos = vec![repo("/repo", vec![agent("one")])];
        app.mode = Mode::ConfirmPr {
            repo_path: "/repo".into(),
            agent_id: "one".into(),
            branch: "feature/one".into(),
            input: "edited prompt".into(),
        };
        let mut sent = None;

        app.handle_key_with_pr_sender(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |session, prompt| {
                sent = Some((session.to_string(), prompt.to_string()));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(sent, Some(("robco_one".into(), "edited prompt".into())));
        assert!(matches!(app.mode, Mode::Normal));
    }

    #[test]
    fn confirm_pr_target_resolves_after_reordering_and_not_after_pruning() {
        let target = PrTarget {
            repo_path: "/repo-b".into(),
            agent_id: "wanted".to_string(),
            branch: "feature/wanted".to_string(),
        };
        let mut repos = vec![
            repo("/repo-a", vec![agent("other")]),
            repo("/repo-b", vec![agent("first"), agent("wanted")]),
        ];
        repos.swap(0, 1);
        repos[0].agents.swap(0, 1);

        assert_eq!(
            resolve_agent(&repos, &target.repo_path, &target.agent_id),
            Some((0, 0))
        );

        repos[0].agents.remove(0);
        assert_eq!(
            resolve_agent(&repos, &target.repo_path, &target.agent_id),
            None
        );
    }

    #[test]
    fn merge_error_is_recorded_and_cleared_for_the_next_attempt() {
        let mut error = None;

        set_merge_error(&mut error, "gh failed\nretry later");
        assert_eq!(error.as_deref(), Some("gh failed\nretry later"));

        error = None;
        assert_eq!(error, None);
    }
}
