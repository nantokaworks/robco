//! Opening the selected dropr task in the operator's browser (dropr:499):
//! `o` from the task list, or the same key from the task-body reading dialog
//! (`Mode::TaskBody`, dropr:501) — one URL builder, two entry points, the
//! same list/reading pair `dropr_task_drill.rs`'s own launch keys (`n` / `s`)
//! already use.
//!
//! The console route this builds — `https://dropr.sh/{owner}/{repo}/tasks/
//! {task_id}` — was recovered from the console's own published JS bundle
//! (its client-side route table), not guessed; see the dropr:499 decision
//! scribble for the evidence trail. [`dropr::task_console_url`] only answers
//! for a GitHub-hosted repo, the same restriction the dropr workspace link
//! itself already has (see [`dropr::canonical_repo`]), so a repo this
//! cannot build a URL for never reaches this action with a linked workspace
//! in the first place.
//!
//! [`browser::open`] decides at run time whether the URL can go to a local
//! browser at all (dropr:511). Over SSH it cannot, so it goes to the
//! operator's clipboard instead, and this file says so rather than claiming a
//! browser opened.

use crate::{
    browser, dropr,
    locale::{fmt, t},
    model::Selection,
};

use super::super::{App, DroprTaskFocus, Mode, summary::dropr_tasks};

impl App {
    /// `o` at the task list focus level: open the selected task.
    pub(in crate::ui) fn open_dropr_task_from_list(&mut self) {
        let Some(DroprTaskFocus { task }) = self.dropr_task_focus else {
            return;
        };
        self.open_dropr_task(task);
    }

    /// `o` from the task-body reading dialog (`Mode::TaskBody`, dropr:501):
    /// open the same task the dialog shows. `task` comes straight from the
    /// mode, which already names the row.
    pub(in crate::ui) fn open_dropr_task_from_reading(&mut self, task: usize) {
        self.open_dropr_task(task);
    }

    /// Shared by both entry points above: resolve the task, build its console
    /// URL, and hand it to the browser launcher.
    fn open_dropr_task(&mut self, task: usize) {
        self.open_dropr_task_with(task, browser::open);
    }

    /// [`Self::open_dropr_task`] with the launcher taken as a parameter, so
    /// tests can pin the URL it gets called with — and the message it
    /// produces on failure — without actually spawning a browser. Mirrors
    /// `dropr_task_launch::resolve_launch_subtasks`'s injected-fetch shape.
    fn open_dropr_task_with<F>(&mut self, task: usize, launch: F)
    where
        F: FnOnce(&str) -> Result<browser::Opened, String>,
    {
        let Some(Selection::Repo(repo)) = self.selected_item() else {
            self.dropr_task_focus = None;
            self.mode = Mode::Normal;
            return;
        };
        // `visible()` never emits `Selection::Repo(repo)` for an out-of-bounds
        // `repo`, and nothing mutates `self.registry` between that read and
        // this one, so `repo` indexes a live row here (same reasoning as
        // `dropr_task_drill::launch_dropr_task`).
        let repo_node = &self.registry.repos[repo];
        let Some(candidate) = dropr_tasks::selectable_tasks(&repo_node.dropr_tasks)
            .get(task)
            .map(|candidate| (*candidate).clone())
        else {
            self.show_message(t(self.locale, "task is no longer listed"));
            return;
        };
        if candidate.id.is_empty() {
            self.show_message(t(self.locale, "task is missing its dropr id"));
            return;
        }
        let Some(remote_url) = repo_node.remote_url.as_deref() else {
            self.show_message(t(self.locale, "this repository has no git remote"));
            return;
        };
        let Some(url) = dropr::task_console_url(remote_url, &candidate.id) else {
            self.show_message(t(
                self.locale,
                "could not build a console URL for this repository",
            ));
            return;
        };
        match launch(&url) {
            Ok(browser::Opened::Launcher) => self.show_message(fmt(
                self.locale,
                "opened {} in the browser",
                &[&candidate.display_id],
            )),
            // Over SSH a browser would open on the server, so the URL went to
            // the operator's clipboard instead (dropr:511). The message also
            // carries the URL as plain text, for a terminal that ignored the
            // clipboard escape. That escape went straight to the terminal,
            // past ratatui's buffer, so repaint the frame in case a terminal
            // that does not know OSC 52 printed it instead of eating it —
            // `event_loop::drain_stdout_notifications` does the same after
            // OSC 777.
            Ok(browser::Opened::Clipboard) => {
                self.force_redraw = true;
                self.show_message(fmt(
                    self.locale,
                    "copied the URL for {}: {}",
                    &[&candidate.display_id, &url],
                ));
            }
            Err(err) => self.show_message(fmt(
                self.locale,
                "could not open {}: {}",
                &[&candidate.display_id, &err],
            )),
        }
    }
}

#[cfg(test)]
#[path = "dropr_task_open_tests.rs"]
mod tests;
