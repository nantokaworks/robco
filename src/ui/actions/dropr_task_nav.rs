//! Task-list navigation for the dropr task drill-down (dropr:475): walking
//! the list, opening a task's body, and stepping back up one level. Split
//! out of `dropr_task_drill` (dropr:482) to keep that file, which grew a
//! second launch entry point, under the line-count limit. The launch keys
//! themselves — `s` from the body, `n` from the list — live there.

use crate::model::Selection;

use super::super::{App, DroprTaskFocus, summary::dropr_tasks};

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
}

#[cfg(test)]
#[path = "dropr_task_nav_tests.rs"]
mod tests;
