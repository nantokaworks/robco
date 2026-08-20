//! The launch step of the dropr task drill-down (dropr:475): `s` from the
//! task-body reading dialog (`Mode::TaskBody`, dropr:501), or `n` from the
//! task list (dropr:482) — the same launch path one key sooner, for an
//! operator who already knows which task they want. Walking the list and
//! opening the reading dialog — the steps that get an operator to either
//! entry point — live in `dropr_task_nav`, split out to keep this file under
//! the line-count limit.
//!
//! The launch itself replaces PR #373's queued `RuntimeRequest::RunTask` (see
//! `overseer::runtime_request`, still used by Discord's `!run` and kept for
//! that): it creates the agent in this process, the way `n` does
//! (`agent::create_agent`), so it works with the daemon stopped and nothing
//! waits on its dispatch schedule. Two checks the daemon's own dispatch gate
//! makes survive here because losing them causes duplicate work — a live
//! worker or an existing branch for the task refuses the launch, and the
//! dropr claim is taken before the session starts.
//!
//! What this file keeps is the half that must answer within one frame: the
//! refusals, and handing the slow steps to [`super::dropr_task_worker`]
//! (dropr:508). The claim, the prompt and the agent creation all run on that
//! worker thread; [`App::drain_task_launch_events`] takes the result back.

use crate::{
    agent, git,
    locale::{fmt, t},
    model::Selection,
};

use super::super::{App, DroprTaskFocus, Mode, summary::dropr_tasks};
use super::dropr_task_launch::mark_task_in_progress;
use super::dropr_task_worker::{TaskLaunchFailure, TaskLaunchJob, TaskLaunchTarget, spawn};

impl App {
    /// The launch key from the task-body reading dialog (`Mode::TaskBody`,
    /// dropr:501): claim the task, then create the worker in this process —
    /// immediately, the way `n` (new agent) does. `task` comes straight from
    /// the mode, which already names the row without needing a
    /// `DroprTaskFocus` unwrap.
    pub(in crate::ui) fn launch_dropr_task_from_reading(&mut self, task: usize) {
        self.launch_dropr_task(task);
    }

    /// `n` at the list focus level (dropr:482): the same launch path as
    /// [`Self::launch_dropr_task_from_reading`], one key sooner — for an
    /// operator who already knows which task they want and does not need to
    /// read its body first.
    pub(in crate::ui) fn launch_dropr_task_from_list(&mut self) {
        let Some(DroprTaskFocus { task }) = self.dropr_task_focus else {
            return;
        };
        self.launch_dropr_task(task);
    }

    /// Shared by both entry points above: refuse what can be refused here and
    /// now, then hand the rest to the launch worker.
    fn launch_dropr_task(&mut self, task: usize) {
        // One launch at a time. A second press while one is in flight would
        // otherwise start a duplicate worker for the same task, or race two
        // `git worktree add` runs and two registry writes against each other.
        if let Some(running) = self
            .task_launch_job
            .as_ref()
            .map(|job| job.display_id.clone())
        {
            self.show_message(fmt(
                self.locale,
                "a launch is already in progress: {}",
                &[&running],
            ));
            return;
        }
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            self.dropr_task_focus = None;
            self.mode = Mode::Normal;
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
        // A local `show-ref`, so this last refusal still answers within the
        // frame the key was pressed in.
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

        let display_id = candidate.display_id.clone();
        self.task_launch_job = Some(spawn(TaskLaunchTarget {
            repo: repo_node.clone(),
            config: self.config.clone(),
            workspace_id,
            candidate,
            title,
        }));
        self.dropr_task_focus = None;
        // Closes the reading dialog too when the launch key was `s`; a no-op
        // when it was already `n` from the list, where the mode is Normal
        // already.
        self.mode = Mode::Normal;
        self.show_message(fmt(self.locale, "launching {}…", &[&display_id]));
    }

    /// The task a launch is currently working on, for the quit guard. The
    /// launch now outlives the key press, so quitting in the middle of one
    /// would leave a claim taken in dropr with nothing behind it — the same
    /// reason an in-flight merge holds the quit key.
    pub(in crate::ui) fn launching_task(&self) -> Option<String> {
        self.task_launch_job
            .as_ref()
            .map(|job| job.display_id.clone())
    }

    /// Takes one finished launch back onto the UI thread. Called every tick
    /// from the event loop, next to the merge and pre-check drains.
    pub(in crate::ui) fn drain_task_launch_events(&mut self) {
        use std::sync::mpsc::TryRecvError;

        let received = match self.task_launch_job.as_ref().map(|job| job.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err(TaskLaunchFailure::WorkerTerminated)),
            Some(Err(TryRecvError::Empty)) | None => None,
        };
        let Some(result) = received else {
            return;
        };
        let Some(job) = self.task_launch_job.take() else {
            return;
        };
        match result {
            Ok(new_agent) => self.finish_task_launch(&job, new_agent),
            // Every message names the task: by the time one of these lands the
            // operator has moved on, and a bare error names nothing.
            Err(failure) => {
                let message = match failure {
                    TaskLaunchFailure::SubtasksUnconfirmed => fmt(
                        self.locale,
                        "could not confirm {}'s subtasks — refresh the task list and try again",
                        &[&job.display_id],
                    ),
                    TaskLaunchFailure::ClaimRefused(reason) => fmt(
                        self.locale,
                        "could not claim {}: {}",
                        &[&job.display_id, &reason],
                    ),
                    TaskLaunchFailure::DroprUnreachable => fmt(
                        self.locale,
                        "could not reach dropr to claim {}",
                        &[&job.display_id],
                    ),
                    TaskLaunchFailure::Spawn(detail) => fmt(
                        self.locale,
                        "could not launch {}: {}",
                        &[&job.display_id, &detail],
                    ),
                    TaskLaunchFailure::WorkerTerminated => fmt(
                        self.locale,
                        "launch worker for {} terminated unexpectedly",
                        &[&job.display_id],
                    ),
                };
                self.show_message(message);
            }
        }
    }

    /// Registers an agent the worker created: on disk, in the task-row cache,
    /// and as the new selection.
    fn finish_task_launch(&mut self, job: &TaskLaunchJob, new_agent: crate::model::AgentNode) {
        let new_agent_id = new_agent.id.clone();
        let repo_path = job.repo_path.clone();
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
        // `locked_registry_update` above only ever carries the pre-launch
        // value back over a disk reload (see `registry_write::carry_repo`), so
        // the claim this launch just took has to land here — directly on the
        // in-memory cache, the same way a background fetch updates it.
        if let Some(repo) = self
            .registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == job.repo_path)
        {
            mark_task_in_progress(&mut repo.dropr_tasks, &job.task_id);
        }
        match result {
            Ok(()) if registered => {
                self.restore_selection(Some(format!("agent:{new_agent_id}")));
                self.show_message(fmt(
                    self.locale,
                    "launched {} for {}",
                    &[&job.title, &job.display_id],
                ));
            }
            Ok(()) => {
                self.show_message(fmt(
                    self.locale,
                    "launched {}, but its repository is no longer registered",
                    &[&job.title],
                ));
            }
            Err(err) => {
                self.show_message(fmt(
                    self.locale,
                    "launched {}, but could not save the registry: {}",
                    &[&job.display_id, &err.to_string()],
                ));
            }
        }
    }
}

#[cfg(test)]
#[path = "dropr_task_drill_tests.rs"]
mod tests;
