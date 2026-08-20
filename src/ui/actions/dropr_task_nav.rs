//! Task-list navigation for the dropr task drill-down (dropr:475): walking
//! the list and opening a task's body. Split out of `dropr_task_drill`
//! (dropr:482) to keep that file, which grew a second launch entry point,
//! under the line-count limit. The launch keys themselves — `s` from the
//! body, `n` from the list — live there.
//!
//! Opening a body used to move `DroprTaskFocus` into a second `Body` state;
//! it now opens `Mode::TaskBody` instead, a dialog drawn over this list
//! (dropr:501) — `dropr_task_focus` stays on the list the whole time it is
//! open, so closing it (`ui::input`'s own `Mode::TaskBody` arm) needs no
//! matching "close" method here.

use crate::model::Selection;

use super::super::{App, DroprTaskFocus, Mode, summary::dropr_tasks};

impl App {
    /// `Enter` on a repository row with the INFO pane showing: move focus
    /// into its task list, starting on the first row.
    pub(in crate::ui) fn enter_dropr_task_list(&mut self) {
        self.dropr_task_focus = Some(DroprTaskFocus { task: 0 });
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
        let Some(DroprTaskFocus { task }) = self.dropr_task_focus else {
            return;
        };
        let Some(count) = self.dropr_task_count() else {
            return;
        };
        if count == 0 {
            return;
        }
        let next = (task as isize + delta).clamp(0, count as isize - 1) as usize;
        self.dropr_task_focus = Some(DroprTaskFocus { task: next });
    }

    /// `Enter` on a task row: open a dialog reading its full body, over the
    /// list (dropr:501). The list itself — cursor and scroll both — is
    /// untouched by this; only `self.mode` changes.
    pub(in crate::ui) fn open_dropr_task_body(&mut self) {
        let Some(DroprTaskFocus { task }) = self.dropr_task_focus else {
            return;
        };
        let Some(count) = self.dropr_task_count() else {
            return;
        };
        if task >= count {
            return;
        }
        self.mode = Mode::TaskBody { task, scroll: 0 };
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
