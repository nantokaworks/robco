//! Key routing for the task-body reading dialog (`Mode::TaskBody`,
//! dropr:501), split out of `ui::input` to keep that file under this
//! project's line-count limit.
//!
//! Scroll keys mutate this mode's own `scroll` in place. `s`/`o` copy `task`
//! out first so the borrow of `app.mode` ends before the `&mut App` call —
//! the same shape `ui::input`'s own `Mode::ConfirmPr` `Submit` arm uses.
//! Every other key is swallowed: this mode owns input exclusively while
//! open, unlike the old `DroprTaskFocus::Body` guard clause it replaces,
//! which only claimed specific keys and let the rest fall through to
//! `Mode::Normal`'s own dispatch.

use crossterm::event::KeyCode;

use super::super::{App, Mode};

pub(super) fn handle(app: &mut App, code: KeyCode) {
    let Mode::TaskBody { task, scroll } = &mut app.mode else {
        return;
    };
    let task = *task;
    match code {
        KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
        KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
        KeyCode::PageDown => *scroll = scroll.saturating_add(10),
        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            app.mode = Mode::Normal;
        }
        // Deliberately not `Enter`: the operator opened this dialog with
        // `Enter` already, and a second press starting real work — a
        // worktree, a branch, a live tmux session — on the same key that
        // also means "open" one level up is exactly the kind of accidental
        // action this dialog exists to prevent.
        KeyCode::Char('s') => app.launch_dropr_task_from_reading(task),
        KeyCode::Char('o') => app.open_dropr_task_from_reading(task),
        _ => {}
    }
}

#[cfg(test)]
#[path = "dropr_task_body_tests.rs"]
mod tests;
