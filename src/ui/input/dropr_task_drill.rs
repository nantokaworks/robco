//! Key routing for the dropr task-list focus (dropr:475), while
//! `App::dropr_task_focus` is `Some`. Same guard-clause shape as
//! `input::overseer::handle_normal`: claims the keys that mean something
//! different at this focus level and returns `false` for everything else, so
//! `?` / `q` / repo-row actions on other tabs keep working unchanged.
//!
//! Movement and `Enter`/`Esc` are claimed unconditionally while focused —
//! the outer tree must not also react to them, or the operator would not be
//! able to tell which list a keypress just moved.
//!
//! `list_key` also claims `n` (dropr:482): it launches the selected task the
//! same way `s` does from the task-body reading dialog, one key sooner.
//! Since this module's `handle_normal` runs as a guard ahead of the outer
//! `Mode::Normal` match in `input.rs` (`code if
//! dropr_task_drill::handle_normal(self, code) => {}` before that match's own
//! `n` arm), claiming `n` here only changes what it does while the list is
//! focused — `n`'s "new agent" meaning elsewhere is untouched.
//!
//! Reading a task's body used to be a second focus level here, routed the
//! same guard-clause way; it is now `Mode::TaskBody`, a distinct `Mode` with
//! its own exclusive arm in `input.rs`'s top-level match (dropr:501) — see
//! that arm for its key routing.

use crossterm::event::KeyCode;

use super::super::App;

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    if app.dropr_task_focus.is_none() {
        return false;
    }
    list_key(app, code)
}

fn list_key(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_dropr_task_cursor(1);
            true
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_dropr_task_cursor(-1);
            true
        }
        KeyCode::Enter => {
            app.open_dropr_task_body();
            true
        }
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.leave_dropr_task_list();
            true
        }
        // The list-level launch shortcut (dropr:482): the same launch path
        // `s` uses from the body, one key sooner.
        KeyCode::Char('n') => {
            app.launch_dropr_task_from_list();
            true
        }
        // Open the selected task in the browser (dropr:499). `o` is free at
        // this focus level — `j`/`k`/`h`/`n` are the only other keys it
        // claims here.
        KeyCode::Char('o') => {
            app.open_dropr_task_from_list();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "dropr_task_drill_tests.rs"]
mod tests;
