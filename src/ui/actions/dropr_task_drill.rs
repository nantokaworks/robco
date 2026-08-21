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
//! worker thread; `dropr_task_finish` (split out to keep this file under the
//! line-count limit too) drains the result and registers or reverts.
//!
//! dropr:517 changed what happens once the refusals are past: the operator
//! can now fire several launches in a row without the cursor moving off the
//! task list (`n` no longer clears `dropr_task_focus`, and a finished launch
//! no longer re-points the outer selection at the new agent — see
//! `dropr_task_finish`), and the row flips to `in_progress` at the keypress
//! instead of at completion, because that is the whole reason the operator
//! wants to see it flip. [`App::begin_launch`] is where the flip and the
//! per-task tracking happen, kept as its own step so a test can drive it with
//! a fake job instead of a real one.

use crate::{
    agent, git,
    locale::{fmt, t},
    model::Selection,
};

use super::super::{App, DroprTaskFocus, Mode, summary::dropr_tasks};
use super::dropr_task_launch::mark_task_in_progress;
use super::dropr_task_worker::{TaskLaunchJob, TaskLaunchTarget, spawn};

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
        // This task, specifically. A second `n` on the *same* row while it is
        // still launching would start a duplicate worker, or race two `git
        // worktree add` runs and two registry writes against each other — but
        // a different row's `n` is a different task and must go straight
        // through (dropr:517 replaced the old single global slot with this
        // per-task one).
        if self.task_launch_jobs.contains_key(&candidate.id) {
            self.show_message(fmt(
                self.locale,
                "a launch is already in progress: {}",
                &[&candidate.display_id],
            ));
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
        let job = spawn(TaskLaunchTarget {
            repo: repo_node.clone(),
            config: self.config.clone(),
            workspace_id,
            candidate,
            title,
        });
        self.begin_launch(repo, job);
        // Closes the reading dialog when the launch key was `s`; a no-op when
        // it was already `n` from the list, where the mode is Normal already.
        // `dropr_task_focus` is deliberately left alone either way (dropr:517):
        // the operator stays on the task list, ready to fire the next one.
        self.mode = Mode::Normal;
        self.show_message(fmt(self.locale, "launching {}…", &[&display_id]));
    }

    /// The point a launch stops being a refusal candidate and starts being
    /// tracked: marks the row `in_progress` right away (dropr:517, so the
    /// operator sees it flip before the claim round trip even starts) and
    /// files the job under its task id so a second press on the same row is
    /// refused. Split out of [`Self::launch_dropr_task`] so a test can drive
    /// it with [`super::dropr_task_worker::test_job`] instead of a real,
    /// network-touching launch.
    fn begin_launch(&mut self, repo: usize, job: TaskLaunchJob) {
        if let Some(repo_node) = self.registry.repos.get_mut(repo) {
            mark_task_in_progress(&mut repo_node.dropr_tasks, &job.task_id);
        }
        self.task_launch_jobs.insert(job.task_id.clone(), job);
    }
}

#[cfg(test)]
#[path = "dropr_task_drill_tests.rs"]
mod tests;
