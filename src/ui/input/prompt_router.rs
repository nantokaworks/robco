//! Prompt-mode key arms, split out so `ui::input` only routes mode families.

use crossterm::event::{KeyCode, KeyEvent};

use crate::{Result, agent, locale::fmt};

use super::{App, Mode, host_connect, overseer, prompt_agent};

pub(super) fn handle(app: &mut App, key: KeyEvent) -> Option<Result<()>> {
    if !matches!(
        app.mode,
        Mode::PromptAgent { .. }
            | Mode::PromptRepo { .. }
            | Mode::PromptRenameRepo { .. }
            | Mode::PromptHostConnect { .. }
            | Mode::PromptOverseer { .. }
            | Mode::PromptSession { .. }
    ) {
        return None;
    }

    Some(handle_prompt(app, key))
}

fn handle_prompt(app: &mut App, key: KeyEvent) -> Result<()> {
    match &mut app.mode {
        Mode::PromptAgent { repo, input } => match key.code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Enter if key.modifiers.is_empty() => {
                let (title, prompt) = prompt_agent::parse(input.text());
                let repo_idx = *repo;
                app.mode = Mode::Normal;
                if !title.is_empty() {
                    match agent::create_agent(
                        &app.registry.repos[repo_idx],
                        &title,
                        prompt.as_deref(),
                        &app.config,
                        None,
                    ) {
                        Ok(agent) => {
                            let repo_path = app.registry.repos[repo_idx].path.clone();
                            let mut registered = false;
                            app.locked_registry_update(|registry| {
                                if let Some(repo) = registry
                                    .repos
                                    .iter_mut()
                                    .find(|repo| repo.path == repo_path)
                                {
                                    repo.agents.push(agent);
                                    registered = true;
                                }
                            })?;
                            app.show_message(if registered {
                                fmt(app.locale, "created agent {}", &[&title])
                            } else {
                                // The worktree and tmux session are up; only
                                // the repo row is gone from the stored
                                // registry, so say so rather than reporting
                                // a clean create that left nothing behind.
                                fmt(
                                    app.locale,
                                    "created agent {}, but its repository is no longer registered",
                                    &[&title],
                                )
                            });
                        }
                        Err(err) => app.show_message(err.to_string()),
                    }
                }
            }
            _ => {
                input.handle_key(key);
            }
        },
        Mode::PromptRepo { input } => match key.code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Enter if key.modifiers.is_empty() => {
                let value = input.text().trim().to_string();
                app.mode = Mode::Normal;
                if !value.is_empty() {
                    app.add_repo_input(&value);
                }
            }
            _ => {
                input.handle_key(key);
            }
        },
        Mode::PromptRenameRepo { path, input } => match key.code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Enter if key.modifiers.is_empty() => {
                let new_name = input.text().trim().to_string();
                let path = path.clone();
                app.mode = Mode::Normal;
                if !new_name.is_empty() {
                    app.rename_repo(&path, &new_name);
                }
            }
            _ => {
                input.handle_key(key);
            }
        },
        Mode::PromptHostConnect { .. } => host_connect::handle_prompt(app, key),
        Mode::PromptOverseer { input } => match overseer::instruction_prompt_action(input, key) {
            overseer::PromptAction::Stay => {}
            overseer::PromptAction::Cancel => app.mode = Mode::Normal,
            overseer::PromptAction::Submit(instruction) => {
                app.mode = Mode::Normal;
                app.instruct_overseer(&instruction);
            }
        },
        Mode::PromptSession {
            session,
            host,
            input,
        } => match overseer::instruction_prompt_action(input, key) {
            overseer::PromptAction::Stay => {}
            overseer::PromptAction::Cancel => app.mode = Mode::Normal,
            overseer::PromptAction::Submit(instruction) => {
                let session = session.clone();
                let host = host.clone();
                app.mode = Mode::Normal;
                app.instruct_prompt_session(host.as_ref(), &session, &instruction);
            }
        },
        _ => unreachable!("checked by prompt router"),
    }

    Ok(())
}
