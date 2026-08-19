//! `↵` on a dropr task row: launch it.
//!
//! The TUI is a separate process from the Overseer daemon, so this cannot
//! call `dispatch::run_named` in-process the way the daemon's own tick does
//! for a polled candidate — the daemon owns the live `Ledger` gate needs, and
//! a second process racing it with its own load/mutate/save would lose
//! updates. Instead this queues a [`RuntimeRequest::RunTask`], the same
//! cross-process request `overseer::runtime_request` already uses for other
//! operator-issued signals (dropr:470) — the daemon's next tick (woken
//! immediately, not waited out) feeds it into the exact same
//! `dispatch::run_named` call `!run <task>` uses from Discord. One dispatch
//! path, three callers.

use chrono::Utc;

use crate::{
    locale::{fmt, t},
    overseer::runtime_request::{self, RuntimeRequest},
};

use super::super::{App, summary::dropr_tasks};

impl App {
    pub(in crate::ui) fn launch_dropr_task_selected(&mut self, repo: usize, task: usize) {
        let Some(repo_node) = self.registry.repos.get(repo) else {
            self.show_message(t(self.locale, "repository changed, nothing to launch"));
            return;
        };
        let Some(candidate) = dropr_tasks::selectable_tasks(&repo_node.dropr_tasks)
            .get(task)
            .copied()
        else {
            self.show_message(t(self.locale, "task is no longer listed"));
            return;
        };
        let display_id = candidate.display_id.clone();
        let request = RuntimeRequest::RunTask {
            source: "tui".into(),
            task: display_id.clone(),
            at: Utc::now(),
        };
        match runtime_request::enqueue(request) {
            Ok(()) => self.show_message(fmt(self.locale, "queued {} for dispatch", &[&display_id])),
            Err(err) => self.show_message(err.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "launch_dropr_task_tests.rs"]
mod tests;
