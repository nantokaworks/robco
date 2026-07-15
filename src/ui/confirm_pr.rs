use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{Result, config::Config};

pub(super) enum ConfirmPrAction {
    Stay,
    Cancel,
    Submit(String),
    Saved(Result<()>),
}

pub(super) fn confirm_pr_action(
    config: &mut Config,
    input: &mut String,
    key: KeyEvent,
    save: impl FnOnce(&Config) -> Result<()>,
) -> ConfirmPrAction {
    match key.code {
        KeyCode::Esc => ConfirmPrAction::Cancel,
        KeyCode::Enter => ConfirmPrAction::Submit(input.clone()),
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let previous = std::mem::replace(&mut config.pr_prompt, input.clone());
            let result = save(config);
            if result.is_err() {
                config.pr_prompt = previous;
            }
            ConfirmPrAction::Saved(result)
        }
        KeyCode::Backspace => {
            input.pop();
            ConfirmPrAction::Stay
        }
        KeyCode::Char(ch) => {
            input.push(ch);
            ConfirmPrAction::Stay
        }
        _ => ConfirmPrAction::Stay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_pr_ctrl_s_updates_config_and_stays_in_dialog() {
        let mut config = Config {
            pr_prompt: "default prompt".into(),
            ..Config::default()
        };
        let mut input = "edited prompt".to_string();
        let mut saved = None;

        let action = confirm_pr_action(
            &mut config,
            &mut input,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            |config| {
                saved = Some(config.pr_prompt.clone());
                Ok(())
            },
        );

        assert!(matches!(action, ConfirmPrAction::Saved(Ok(()))));
        assert_eq!(saved.as_deref(), Some("edited prompt"));
        assert_eq!(config.pr_prompt, "edited prompt");
    }

    #[test]
    fn confirm_pr_enter_submits_current_edited_input() {
        let mut config = Config {
            pr_prompt: "default prompt".into(),
            ..Config::default()
        };
        let mut input = "edited prompt".to_string();

        let action = confirm_pr_action(
            &mut config,
            &mut input,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |_| unreachable!("Enter must not save config"),
        );

        assert!(matches!(
            action,
            ConfirmPrAction::Submit(prompt) if prompt == "edited prompt"
        ));
        assert_eq!(config.pr_prompt, "default prompt");
    }

    #[test]
    fn confirm_pr_failed_save_rolls_back_config_and_stays_in_dialog() {
        let mut config = Config {
            pr_prompt: "default prompt".into(),
            ..Config::default()
        };
        let mut input = "edited prompt".to_string();

        let action = confirm_pr_action(
            &mut config,
            &mut input,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            |_| Err(std::io::Error::other("save failed").into()),
        );

        assert!(matches!(action, ConfirmPrAction::Saved(Err(_))));
        assert_eq!(config.pr_prompt, "default prompt");
        assert_eq!(input, "edited prompt");
    }
}
