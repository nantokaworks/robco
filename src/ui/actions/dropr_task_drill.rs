//! The dropr task drill-down (dropr:475): `Enter` on a repository row with
//! INFO showing moves focus into its task list; `Enter` on a task opens its
//! body; one key from the body starts the work.
//!
//! The launch itself replaces PR #373's queued `RuntimeRequest::RunTask` (see
//! `overseer::runtime_request`, still used by Discord's `!run` and kept for
//! that): it creates the agent in this process, the way `n` does
//! (`agent::create_agent`), so it works with the daemon stopped and nothing
//! waits on its dispatch schedule. Two checks the daemon's own dispatch gate
//! makes survive here because losing them causes duplicate work — a live
//! worker or an existing branch for the task refuses the launch, and the
//! dropr claim is taken before the session starts.

use crate::{
    agent, dropr, git,
    locale::{fmt, t},
    model::Selection,
    overseer::{exec::COMMAND_TIMEOUT, templates::worker_prompt},
};

use super::super::{App, DroprTaskFocus, summary::dropr_tasks};

/// Identifies a task claim taken by this drill-down, distinct from the
/// Overseer daemon's own `OVERSEER_AGENT_ID` — this launch is operator-issued
/// and in-process, not a dispatch decision, so it should not read as one.
const DIRECT_LAUNCH_AGENT_ID: &str = "robco-ui";

impl App {
    /// `Enter` on a repository row with the INFO pane showing: move focus
    /// into its task list, starting on the first row.
    pub(in crate::ui) fn enter_dropr_task_list(&mut self) {
        self.dropr_task_focus = Some(DroprTaskFocus::List { task: 0 });
        self.preview_scroll = 0;
    }

    /// `Esc` / `h` / `Left` while the task list is focused: return to the
    /// repository row.
    pub(in crate::ui) fn leave_dropr_task_list(&mut self) {
        self.dropr_task_focus = None;
    }

    /// `j`/`k` or the arrows while the task list is focused: walk it,
    /// clamped to what is actually listed.
    pub(in crate::ui) fn move_dropr_task_cursor(&mut self, delta: isize) {
        let Some(DroprTaskFocus::List { task }) = self.dropr_task_focus else {
            return;
        };
        let Some(count) = self.dropr_task_count() else {
            return;
        };
        if count == 0 {
            return;
        }
        let next = (task as isize + delta).clamp(0, count as isize - 1) as usize;
        self.dropr_task_focus = Some(DroprTaskFocus::List { task: next });
    }

    /// `Enter` on a task row: open its body.
    pub(in crate::ui) fn open_dropr_task_body(&mut self) {
        let Some(DroprTaskFocus::List { task }) = self.dropr_task_focus else {
            return;
        };
        let Some(count) = self.dropr_task_count() else {
            return;
        };
        if task >= count {
            return;
        }
        self.dropr_task_focus = Some(DroprTaskFocus::Body { task });
        self.preview_scroll = 0;
    }

    /// `Esc` / `h` / `Left` while a task body is focused: back to the list,
    /// on the same task.
    pub(in crate::ui) fn close_dropr_task_body(&mut self) {
        if let Some(DroprTaskFocus::Body { task }) = self.dropr_task_focus {
            self.dropr_task_focus = Some(DroprTaskFocus::List { task });
            self.preview_scroll = 0;
        }
    }

    fn dropr_task_count(&self) -> Option<usize> {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            return None;
        };
        let repo_node = self.registry.repos.get(repo)?;
        Some(dropr_tasks::selectable_tasks(&repo_node.dropr_tasks).len())
    }

    /// The launch key from the task body: claim the task, then create the
    /// worker in this process — immediately, the way `n` does.
    pub(in crate::ui) fn launch_dropr_task_from_body(&mut self) {
        let Some(DroprTaskFocus::Body { task }) = self.dropr_task_focus else {
            return;
        };
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            self.dropr_task_focus = None;
            return;
        };
        // `visible()` never emits `Selection::Repo(repo)` for an out-of-bounds
        // `repo`, and nothing mutates `self.registry` between that read and
        // this one, so `repo` indexes a live row here.
        let repo_node = &self.registry.repos[repo];
        let Some(candidate) = dropr_tasks::selectable_tasks(&repo_node.dropr_tasks)
            .get(task)
            .map(|candidate| (*candidate).clone())
        else {
            self.show_message(t(self.locale, "task is no longer listed"));
            return;
        };
        let Some(workspace_id) = repo_node.dropr.as_ref().map(|workspace| workspace.id.clone())
        else {
            self.show_message(t(self.locale, "no dropr workspace linked to this repo"));
            return;
        };
        if candidate.id.is_empty() {
            self.show_message(t(self.locale, "task is missing its dropr id"));
            return;
        }

        let bare_number = candidate.display_id.trim_start_matches('#');
        if let Some(existing) = repo_node
            .agents
            .iter()
            .find(|agent| agent.task_number.as_deref() == Some(bare_number))
        {
            self.show_message(fmt(
                self.locale,
                "{} already has a live worker: {}",
                &[&candidate.display_id, &existing.title],
            ));
            return;
        }

        let title = format!("{} {}", candidate.display_id, candidate.title);
        let branch = agent::worker_branch_name(&self.config, &repo_node.name, &title, None);
        match git::branch_exists(&repo_node.path, &branch) {
            Ok(true) => {
                self.show_message(fmt(
                    self.locale,
                    "{} already has a branch: {}",
                    &[&candidate.display_id, &branch],
                ));
                return;
            }
            Ok(false) => {}
            Err(err) => {
                self.show_message(err.to_string());
                return;
            }
        }

        // Reuses the subtree this repo's own INFO pane already fetched
        // (dropr:475): the launch must not wait on the network beyond the
        // claim below, and a root task's children are already flattened into
        // this same `DroprTaskFetch` when its subtree query succeeded (see
        // `dropr::repo_tasks`). Unknown when it did not — the close
        // directive then covers only the parent, same as a childless task.
        let subtasks: Vec<dropr::Subtask> = repo_node
            .dropr_tasks
            .tasks
            .iter()
            .filter(|other| other.parent_task_id.as_deref() == Some(candidate.id.as_str()))
            .map(|other| dropr::Subtask {
                display_id: other.display_id.clone(),
            })
            .collect();
        let repo_path = repo_node.path.clone();
        let task_id = candidate.id.clone();

        match dropr::claim_task(
            &workspace_id,
            &task_id,
            DIRECT_LAUNCH_AGENT_ID,
            COMMAND_TIMEOUT,
        ) {
            dropr::ClaimAttempt::Claimed => {}
            dropr::ClaimAttempt::Refused(reason) => {
                self.show_message(fmt(
                    self.locale,
                    "could not claim {}: {}",
                    &[&candidate.display_id, &reason],
                ));
                return;
            }
            dropr::ClaimAttempt::Unavailable => {
                self.show_message(fmt(
                    self.locale,
                    "could not reach dropr to claim {}",
                    &[&candidate.display_id],
                ));
                return;
            }
        }

        let prompt = worker_prompt(
            &candidate.display_id,
            &task_id,
            &candidate.title,
            &repo_node.name,
            &subtasks,
            self.config.language.as_deref(),
            self.config.overseer.worker_prompt_template.as_deref(),
        );

        match agent::create_agent(repo_node, &title, Some(&prompt), &self.config, None) {
            Ok(new_agent) => {
                let new_agent_id = new_agent.id.clone();
                let mut registered = false;
                let result = self.locked_registry_update(|registry| {
                    if let Some(repo) = registry.repos.iter_mut().find(|repo| repo.path == repo_path)
                    {
                        repo.agents.push(new_agent);
                        registered = true;
                    }
                });
                self.dropr_task_focus = None;
                match result {
                    Ok(()) if registered => {
                        self.restore_selection(Some(format!("agent:{new_agent_id}")));
                        self.show_message(fmt(
                            self.locale,
                            "launched {} for {}",
                            &[&title, &candidate.display_id],
                        ));
                    }
                    Ok(()) => {
                        self.show_message(fmt(
                            self.locale,
                            "launched {}, but its repository is no longer registered",
                            &[&title],
                        ));
                    }
                    Err(err) => self.show_message(err.to_string()),
                }
            }
            Err(err) => {
                // The claim was taken for a worker that never started;
                // holding it would park the task away from the next
                // operator or dispatch pass. Same recovery
                // `overseer::dispatch::worker::spawn_candidate` makes on its
                // own spawn failure.
                let _ = dropr::release_claim(
                    &workspace_id,
                    &task_id,
                    DIRECT_LAUNCH_AGENT_ID,
                    COMMAND_TIMEOUT,
                );
                self.show_message(err.to_string());
            }
        }
    }
}

#[cfg(test)]
#[path = "dropr_task_drill_tests.rs"]
mod tests;
