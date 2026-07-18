use crossterm::event::{KeyCode, KeyEvent};

pub(super) enum PromptAction {
    Stay,
    Cancel,
    Submit(String),
}

pub(super) fn prompt_action(input: &mut String, key: KeyEvent) -> PromptAction {
    match key.code {
        KeyCode::Esc => PromptAction::Cancel,
        KeyCode::Enter if input.trim().is_empty() => PromptAction::Stay,
        KeyCode::Enter => PromptAction::Submit(input.trim().to_string()),
        KeyCode::Backspace => {
            input.pop();
            PromptAction::Stay
        }
        KeyCode::Char(ch) => {
            input.push(ch);
            PromptAction::Stay
        }
        _ => PromptAction::Stay,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    #[test]
    fn enter_submits_trimmed_instruction() {
        let mut input = "  review task  ".to_string();
        let action = prompt_action(
            &mut input,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(matches!(action, PromptAction::Submit(text) if text == "review task"));
    }
}
