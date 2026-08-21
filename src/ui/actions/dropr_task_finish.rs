//! The result half of a dropr task launch, split out of `dropr_task_drill`
//! (dropr:517) to keep that file under the line-count limit: draining a
//! finished [`super::dropr_task_worker::TaskLaunchJob`], registering the new
//! agent on success, and undoing the optimistic row flip on failure.
//!
//! `dropr_task_drill::App::begin_launch` is the other half of this pair — it
//! marks the row `in_progress` and files the job at the keypress. This file
//! only ever reads that job back once its worker thread answers.

use crate::locale::fmt;

use super::super::App;
use super::dropr_task_launch::revert_task_in_progress;
use super::dropr_task_worker::{TaskLaunchFailure, TaskLaunchJob};

impl App {
    /// Every task a launch is currently working on, sorted so the quit-guard
    /// message reads the same on every frame — `HashMap` iteration order does
    /// not (dropr:517, mirrors `App::merging_branches`). The launch now
    /// outlives the key press, so quitting mid-launch would leave a claim
    /// taken in dropr with nothing behind it — the same reason an in-flight
    /// merge holds the quit key.
    pub(in crate::ui) fn launching_tasks(&self) -> Vec<String> {
        let mut tasks: Vec<String> = self
            .task_launch_jobs
            .values()
            .map(|job| job.display_id.clone())
            .collect();
        tasks.sort();
        tasks
    }

    /// Takes every finished launch back onto the UI thread. Called every tick
    /// from the event loop, next to the merge and pre-check drains. Mirrors
    /// `App::drain_merge_events`'s two-pass shape: collect results against an
    /// immutable borrow of the map first, then mutate once that borrow ends.
    pub(in crate::ui) fn drain_task_launch_events(&mut self) {
        use std::sync::mpsc::TryRecvError;

        let mut finished = Vec::new();
        for (task_id, job) in &self.task_launch_jobs {
            match job.try_recv() {
                Ok(result) => finished.push((task_id.clone(), result)),
                Err(TryRecvError::Disconnected) => {
                    finished.push((task_id.clone(), Err(TaskLaunchFailure::WorkerTerminated)));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        for (task_id, result) in finished {
            let Some(job) = self.task_launch_jobs.remove(&task_id) else {
                continue;
            };
            match result {
                Ok(new_agent) => self.finish_task_launch(&job, new_agent),
                // Every message names the task: by the time one of these lands
                // the operator has moved on, and a bare error names nothing.
                Err(failure) => {
                    self.revert_failed_launch(&job);
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
    }

    /// Undoes the optimistic `in_progress` flip `App::begin_launch` made, for
    /// a launch that did not land (dropr:517). The dropr claim itself needs
    /// no matching undo here: a claim refusal or an unreachable dropr never
    /// took one, and a spawn failure already released it on the worker
    /// thread (`dropr_task_worker::run_launch`, dropr:508) — this only ever
    /// undoes the row's own optimistic mark.
    fn revert_failed_launch(&mut self, job: &TaskLaunchJob) {
        if let Some(repo) = self
            .registry
            .repos
            .iter_mut()
            .find(|repo| repo.path == job.repo_path)
        {
            revert_task_in_progress(&mut repo.dropr_tasks, &job.task_id, &job.original_status);
        }
    }

    /// Registers an agent the worker created: on disk, and in the registry's
    /// in-memory cache. Does *not* move the outer selection to it (dropr:517
    /// dropped that): the operator's cursor is theirs to keep exactly where
    /// they left it, whether that is still the task list or somewhere else
    /// entirely.
    fn finish_task_launch(&mut self, job: &TaskLaunchJob, new_agent: crate::model::AgentNode) {
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
        match result {
            Ok(()) if registered => {
                // The reload above may have shifted `self.selected` off the
                // repository it used to sit on (another process could have
                // changed the registry meanwhile) — clamp for safety, the way
                // every other registry-mutating action here does. This never
                // repoints selection at the new agent.
                self.clamp_selection();
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
#[path = "dropr_task_finish_tests.rs"]
mod tests;
