//! The launch step of the dropr task drill-down (dropr:475): `s` from the
//! task body, or `n` from the task list (dropr:482) — the same launch path
//! one key sooner, for an operator who already knows which task they want.
//! Walking the list and opening a body — the steps that get an operator to
//! either entry point — live in `dropr_task_nav`, split out to keep this
//! file under the line-count limit.
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
use super::dropr_task_launch::{mark_task_in_progress, resolve_launch_subtasks};

/// Identifies a task claim taken by this drill-down, distinct from the
/// Overseer daemon's own `OVERSEER_AGENT_ID` — this launch is operator-issued
/// and in-process, not a dispatch decision, so it should not read as one.
const DIRECT_LAUNCH_AGENT_ID: &str = "robco-ui";

impl App {
    /// The launch key from the task body: claim the task, then create the
    /// worker in this process — immediately, the way `n` (new agent) does.
    pub(in crate::ui) fn launch_dropr_task_from_body(&mut self) {
        let Some(DroprTaskFocus::Body { task }) = self.dropr_task_focus else {
            return;
        };
        self.launch_dropr_task(task);
    }

    /// `n` at the list focus level (dropr:482): the same launch path as
    /// [`Self::launch_dropr_task_from_body`], one key sooner — for an
    /// operator who already knows which task they want and does not need to
    /// read its body first.
    pub(in crate::ui) fn launch_dropr_task_from_list(&mut self) {
        let Some(DroprTaskFocus::List { task }) = self.dropr_task_focus else {
            return;
        };
        self.launch_dropr_task(task);
    }

    /// Shared by both entry points above: claim the task, then create the
    /// worker in this process — immediately, the way `n` (new agent) does.
    fn launch_dropr_task(&mut self, task: usize) {
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
        let Some(workspace_id) = repo_node
            .dropr
            .as_ref()
            .map(|workspace| workspace.id.clone())
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
        // (dropr:475) whenever that fetch actually answered for this parent
        // (`subtrees_known`), so the launch usually does not wait on the
        // network beyond the claim below. When the fetch's own
        // `SUBTREE_QUERY_LIMIT` (8 parents) skipped this task, `s` is still a
        // deliberate operator action — a single scoped fetch for the one task
        // being launched is an acceptable wait (dropr:479).
        let subtasks = match resolve_launch_subtasks(
            repo_node,
            &candidate,
            &workspace_id,
            COMMAND_TIMEOUT,
            dropr::fetch_subtasks,
        ) {
            Ok(subtasks) => subtasks,
            Err(()) => {
                self.show_message(fmt(
                    self.locale,
                    "could not confirm {}'s subtasks — refresh the task list and try again",
                    &[&candidate.display_id],
                ));
                return;
            }
        };
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
                    if let Some(repo) = registry
                        .repos
                        .iter_mut()
                        .find(|repo| repo.path == repo_path)
                    {
                        repo.agents.push(new_agent);
                        registered = true;
                    }
                });
                // `dropr_tasks` is runtime-only (`#[serde(skip)]`);
                // `locked_registry_update` above only ever carries the
                // pre-launch value back over a disk reload (see
                // `registry_write::carry_repo`), so the claim this launch
                // just took has to land here — directly on the in-memory
                // cache, the same way a background fetch updates it.
                if let Some(repo) = self
                    .registry
                    .repos
                    .iter_mut()
                    .find(|repo| repo.path == repo_path)
                {
                    mark_task_in_progress(&mut repo.dropr_tasks, &task_id);
                }
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
