use crate::{
    Result, git,
    locale::{fmt, t},
    model::{AgentNode, RepoNode, Selection, Status},
};
use std::path::Path;

use super::super::{App, Mode};

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
    pub(in crate::ui) fn remove_pinned_repo(&mut self, path: &Path) -> Result<()> {
        let Some(repo) = self
            .registry
            .repos
            .iter()
            .position(|repo| repo.path == path)
        else {
            self.show_message(t(self.locale, "repository changed, not removed"));
            return Ok(());
        };
        if !self.registry.repos[repo].pinned || !self.registry.repos[repo].agents.is_empty() {
            self.show_message(t(self.locale, "repository changed, not removed"));
            return Ok(());
        }

        let removed = self.registry.repos[repo].name.clone();
        let path = path.to_path_buf();
        // Re-check the precondition against the stored row instead of dropping
        // the repo on the strength of this process's snapshot: another writer
        // may have registered an agent under it since the registry was read.
        self.locked_registry_update(|registry| {
            registry.repos.retain(|stored| {
                stored.path != path || !(stored.pinned && stored.agents.is_empty())
            })
        })?;
        self.clamp_selection();
        self.show_message(fmt(self.locale, "removed {}", &[&removed]));
        Ok(())
    }

    pub(in crate::ui) fn merge_selected(&mut self) {
        if matches!(self.selected_item(), Some(Selection::ChildWorktree { .. })) {
            self.show_message(t(self.locale, "merge is not available for child worktrees"));
            return;
        }
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return;
        };

        // Only this repository's in-flight merge blocks another one; merges in
        // other repositories touch none of the state `run_merge` races on.
        let repo_path = self.registry.repos[repo].path.clone();
        let repo_name = self.registry.repos[repo].name.clone();
        if let Some(running) = self.merge_job(&repo_path).map(|job| job.branch.clone()) {
            self.show_message(fmt(
                self.locale,
                "merge already in progress in {}: {}",
                &[&repo_name, &running],
            ));
            return;
        }

        self.registry.repos[repo].agents[agent_idx].merge_error = None;
        let repo_node = self.registry.repos[repo].clone();
        let selected = repo_node.agents[agent_idx].clone();
        if selected.status == Status::BranchOnly {
            self.show_message(fmt(self.locale, "branch remains: {}", &[&selected.branch]));
            return;
        }
        if repo_node.host.is_some() {
            self.mode = Mode::ConfirmMerge {
                repo,
                agent: agent_idx,
                plan: super::super::LandPlan::MergeNow,
                head: None,
            };
            return;
        }

        match git::worktree_is_clean(&selected.worktree_path) {
            Ok(true) => {}
            Ok(false) => {
                self.show_message(t(
                    self.locale,
                    "commit or clean untracked changes before merge",
                ));
                return;
            }
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }

        match git::pr_state(&repo_node.path, &selected.branch) {
            Ok(state) => {
                let checks = if state == git::PrState::Open {
                    match git::pr_checks(&repo_node.path, &selected.branch) {
                        Ok(view) => Some(view),
                        Err(err) => {
                            self.show_message(err.to_string());
                            return;
                        }
                    }
                } else {
                    None
                };
                self.offer_land(state, checks, repo, agent_idx, &selected.branch);
            }
            Err(err) => self.show_message(err.to_string()),
        }
    }

    /// The dropr task (`#N`) a pull request robco opens on `agent`'s behalf
    /// must close. The Overseer ledger is authoritative when it has an entry
    /// for this agent; a manually-created or adopted agent has none, so this
    /// falls back to the number already captured on the agent at spawn time
    /// (see `AgentNode::task_number`) — the same number a branch name leads
    /// with, just read off the node instead of re-parsed from it.
    pub(in crate::ui) fn task_display_id(&self, agent: &AgentNode) -> Option<String> {
        self.overseer_snapshot
            .ledger
            .entries
            .iter()
            .find(|entry| entry.agent_id == agent.id)
            .map(|entry| entry.display_id.clone())
            .or_else(|| {
                agent
                    .task_number
                    .as_deref()
                    .map(|number| format!("#{number}"))
            })
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
            self.show_message(t(self.locale, "path is not a git repository"));
            return;
        }

        let Ok(path) = path.canonicalize() else {
            self.show_message(t(self.locale, "could not resolve path"));
            return;
        };

        let mut changed = false;
        if let Err(err) =
            self.locked_registry_update(|registry| changed = registry.add_canonical_pinned(path))
        {
            self.show_message(err.to_string());
        } else if !changed {
            self.show_message(t(self.locale, "repository already listed"));
        } else {
            self.show_message(t(self.locale, "repository added"));
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, Utc};

    use super::*;
    use crate::{
        config::Config,
        overseer::ledger::{LedgerEntry, LedgerPhase},
        registry::Registry,
    };

    #[test]
    fn merge_error_is_recorded_and_cleared_for_the_next_attempt() {
        let mut error = None;

        set_merge_error(&mut error, "gh failed\nretry later");
        assert_eq!(error.as_deref(), Some("gh failed\nretry later"));

        error = None;
        assert_eq!(error, None);
    }

    fn agent(id: &str) -> AgentNode {
        let now = Local::now();
        AgentNode {
            id: id.to_string(),
            parent_agent_id: None,
            title: format!("{id} title"),
            task_number: None,
            worktree_path: format!("/worktrees/{id}").into(),
            branch: format!("feature/{id}"),
            base_commit: String::new(),
            program: "codex".to_string(),
            spawned_by_version: None,
            claude_session_id: None,
            profile: None,
            tmux_session: format!("robco_{id}"),
            created_at: now,
            updated_at: now,
            status: Status::Running,
            worktree_missing: false,
            merge_error: None,
            last_capture: None,
            last_spinner: None,
            last_change_at: None,
            last_auto_accept_at: None,
            shell_working: false,
            mcp_active: false,
            pane_pid: None,
            tracked_command: None,
            subagents: Vec::new(),
            children: Vec::new(),
        }
    }

    #[test]
    fn task_display_id_prefers_the_ledger_and_falls_back_to_the_agent() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());

        let ledgered = agent("ledgered");
        let mut adopted = agent("adopted");
        adopted.task_number = Some("333".into());
        let unknown = agent("unknown");

        app.overseer_snapshot.ledger.entries.push(LedgerEntry {
            task_id: "task-1".into(),
            dropr_task_id: None,
            display_id: "#477".into(),
            repo: "/repo".into(),
            agent_id: "ledgered".into(),
            branch: "feature/ledgered".into(),
            phase: LedgerPhase::Working,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: None,
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
            merge_hold_recheck_reason: None,
            merge_hold_recheck_head: None,
            prerequisite_wait: None,
            merge_hold_stuck_notified: false,
            escalation_notified_reason: None,
            escalation_notified_head: None,
            worker_escalated: false,
            operator_override: None,
            merge_approval: None,
            pr_facts: None,
            worker_finished_at: None,
            approval_dropped: None,
        });

        assert_eq!(app.task_display_id(&ledgered), Some("#477".into()));
        assert_eq!(app.task_display_id(&adopted), Some("#333".into()));
        assert_eq!(app.task_display_id(&unknown), None);
    }
}
