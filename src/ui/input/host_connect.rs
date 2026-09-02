use crossterm::event::{KeyCode, KeyEvent};

use crate::locale::fmt;

use super::{
    super::{App, Mode, text_input::TextInput},
    overseer::{PromptAction, prompt_action},
};

pub(super) fn handle_normal(app: &mut App, code: KeyCode) -> bool {
    if code != KeyCode::Char('H') {
        return false;
    }
    app.mode = Mode::PromptHostConnect {
        input: TextInput::new(),
    };
    true
}

pub(super) fn handle_prompt(app: &mut App, key: KeyEvent) {
    let Mode::PromptHostConnect { input } = &mut app.mode else {
        return;
    };
    match prompt_action(input, key) {
        PromptAction::Stay => {}
        PromptAction::Cancel => app.mode = Mode::Normal,
        PromptAction::Submit(ssh) => {
            let duplicate = app.hosts.iter().any(|slot| slot.label.ssh == ssh);
            app.mode = Mode::Normal;
            if duplicate {
                app.show_message(fmt(app.locale, "already connected to {}", &[&ssh]));
            } else {
                app.connect_host(ssh.clone());
                app.show_message(fmt(app.locale, "connecting to {}", &[&ssh]));
            }
        }
    }
}

#[cfg(test)]
#[path = "host_connect_tests.rs"]
mod tests;
