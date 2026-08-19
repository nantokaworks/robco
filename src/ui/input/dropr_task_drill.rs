//! Key routing for the dropr task drill-down (dropr:475), while
//! `App::dropr_task_focus` is `Some`. Same guard-clause shape as
//! `input::overseer::handle_normal`: claims the keys that mean something
//! different at this focus level and returns `false` for everything else, so
//! `?` / `q` / repo-row actions on other tabs keep working unchanged.
//!
//! Movement and `Enter`/`Esc` are claimed unconditionally while focused —
//! the outer tree must not also react to them, or the operator would not be
//! able to tell which list a keypress just moved.

use crossterm::event::KeyCode;

use super::super::{App, DroprTaskFocus};

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    match app.dropr_task_focus {
        Some(DroprTaskFocus::List { .. }) => list_key(app, code),
        Some(DroprTaskFocus::Body { .. }) => body_key(app, code),
        None => false,
    }
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
        _ => false,
    }
}

fn body_key(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.scroll_preview(false, 1);
            true
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.scroll_preview(true, 1);
            true
        }
        KeyCode::PageDown => {
            app.scroll_preview(false, 10);
            true
        }
        KeyCode::PageUp => {
            app.scroll_preview(true, 10);
            true
        }
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.close_dropr_task_body();
            true
        }
        // The launch key (dropr:475 Level 3), deliberately not `enter`: the
        // operator drilled in with two `enter` presses already, and a third
        // one starting real work — a worktree, a branch, a live tmux
        // session — on a keystroke that also means "open" one level up is
        // exactly the kind of accidental action this drill-down exists to
        // prevent.
        KeyCode::Char('s') => {
            app.launch_dropr_task_from_body();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "dropr_task_drill_tests.rs"]
mod tests;
