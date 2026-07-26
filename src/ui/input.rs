use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{Result, agent, config::Config, model::Selection};

use super::{
    App, Mode, PreviewPane,
    confirm_pr::{ConfirmPrAction, confirm_pr_action},
    help,
    text_input::TextInput,
};

mod confirm;
mod inbox_dismiss;
// `dialog` phrases the bulk-toggle confirmation from the same helper the
// resulting message uses, so the prompt and the outcome cannot drift apart.
pub(in crate::ui) mod management;
mod mouse;
mod overseer;
mod prompt_agent;
mod tree_nav;

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        self.handle_key_with_pr_sender(key, |session, prompt| {
            crate::tmux::send_literal_text(session, prompt)
                .and_then(|()| crate::tmux::send_keys(session, &["Enter"]))
        })
    }
    pub(in crate::ui) fn handle_key_with_pr_sender(
        &mut self,
        key: KeyEvent,
        send: impl FnOnce(&str, &str) -> Result<()>,
    ) -> Result<bool> {
        self.message = None;
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(true);
        }
        if let Some(result) = confirm::handle_confirm(self, key) {
            result?;
            self.clamp_selection();
            return Ok(false);
        }

        match &mut self.mode {
            Mode::PromptAgent { repo, input } => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let (title, prompt) = prompt_agent::parse(input.text());
                    let repo_idx = *repo;
                    self.mode = Mode::Normal;
                    if !title.is_empty() {
                        match agent::create_agent(
                            &self.registry.repos[repo_idx],
                            &title,
                            prompt.as_deref(),
                            &self.config,
                            None,
                        ) {
                            Ok(agent) => {
                                let repo_path = self.registry.repos[repo_idx].path.clone();
                                let mut registered = false;
                                self.locked_registry_update(|registry| {
                                    if let Some(repo) = registry
                                        .repos
                                        .iter_mut()
                                        .find(|repo| repo.path == repo_path)
                                    {
                                        repo.agents.push(agent);
                                        registered = true;
                                    }
                                })?;
                                self.show_message(if registered {
                                    format!("created agent {title}")
                                } else {
                                    // The worktree and tmux session are up; only
                                    // the repo row is gone from the stored
                                    // registry, so say so rather than reporting
                                    // a clean create that left nothing behind.
                                    format!("created agent {title}, but its repository is no longer registered")
                                });
                            }
                            Err(err) => self.show_message(err.to_string()),
                        }
                    }
                }
                _ => {
                    input.handle_key(key);
                }
            },
            Mode::PromptRepo { input } => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let value = input.text().trim().to_string();
                    self.mode = Mode::Normal;
                    if !value.is_empty() {
                        self.add_repo_input(&value);
                    }
                }
                _ => {
                    input.handle_key(key);
                }
            },
            Mode::PromptOverseer { input } => match overseer::prompt_action(input, key) {
                overseer::PromptAction::Stay => {}
                overseer::PromptAction::Cancel => self.mode = Mode::Normal,
                overseer::PromptAction::Submit(instruction) => {
                    self.mode = Mode::Normal;
                    self.instruct_overseer(&instruction);
                }
            },
            Mode::PromptInbox {
                target_session,
                input,
                ..
            } => match overseer::prompt_action(input, key) {
                overseer::PromptAction::Stay => {}
                overseer::PromptAction::Cancel => self.mode = Mode::Normal,
                overseer::PromptAction::Submit(answer) => {
                    let session = target_session.clone();
                    self.mode = Mode::Normal;
                    self.answer_inbox(&session, &answer);
                }
            },
            Mode::ConfirmPr {
                repo_path,
                agent_id,
                input,
                ..
            } => {
                let checking = self.pr_precheck_job.is_some();
                let action = confirm_pr_action(&mut self.config, input, key, Config::save);
                match action {
                    ConfirmPrAction::Stay => {}
                    ConfirmPrAction::Cancel => {
                        self.pr_precheck_job = None;
                        self.mode = Mode::Normal;
                    }
                    ConfirmPrAction::Submit(_) if checking => {}
                    ConfirmPrAction::Submit(prompt) => {
                        let repo_path = repo_path.clone();
                        let agent_id = agent_id.clone();
                        self.mode = Mode::Normal;
                        self.request_pr(&repo_path, &agent_id, &prompt, send)?;
                    }
                    ConfirmPrAction::Saved(result) => match result {
                        Ok(()) => self.show_message("saved PR prompt to config"),
                        Err(err) => self.show_message(err.to_string()),
                    },
                }
            }
            Mode::ErrorDialog { force_kill, .. } => {
                let target = force_kill.clone();
                self.mode = Mode::Normal;
                if matches!(key.code, KeyCode::Char('f') | KeyCode::Char('F'))
                    && let Some(target) = target
                {
                    self.force_kill(target)?;
                }
            }
            Mode::Help { scroll } => {
                let height = help::terminal_height();
                if help::max_scroll(height) == 0 {
                    self.mode = Mode::Normal;
                } else {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => {
                            *scroll = help::scroll_down(*scroll, height);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            *scroll = help::scroll_up(*scroll, height);
                        }
                        _ => self.mode = Mode::Normal,
                    }
                }
            }
            Mode::Normal => match key.code {
                code if overseer::handle_normal(self, code) => {}
                KeyCode::Char('q') | KeyCode::Esc => {
                    let merging = self.merging_branches();
                    if !merging.is_empty() {
                        self.show_message(format!(
                            "merge in progress: {} — wait or ctrl-c to force quit",
                            merging.join(", ")
                        ));
                    } else if matches!(key.code, KeyCode::Esc) && self.dismiss_merge_outcome() {
                        self.show_message("dismissed merge notice");
                    } else {
                        return Ok(true);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.move_selection_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.move_selection_up();
                }
                KeyCode::PageDown => self.scroll_preview(false, 10),
                KeyCode::PageUp => self.scroll_preview(true, 10),
                KeyCode::Right | KeyCode::Char('l') => self.expand_selected_tree_item(),
                KeyCode::Left | KeyCode::Char('h') => self.collapse_selected_tree_item(),
                KeyCode::Char('n') => {
                    if let Some(repo) = self.selected_repo() {
                        self.mode = Mode::PromptAgent {
                            repo,
                            input: TextInput::new(),
                        };
                    } else {
                        self.mode = Mode::PromptRepo {
                            input: TextInput::new(),
                        };
                    }
                }
                KeyCode::Char('a') => {
                    self.mode = Mode::PromptRepo {
                        input: TextInput::new(),
                    };
                }
                KeyCode::Tab => self.toggle_preview(),
                KeyCode::BackTab => self.toggle_preview_back(),
                KeyCode::Char('?') => self.mode = Mode::Help { scroll: 0 },
                KeyCode::Enter => match self.selected_item() {
                    Some(selection) if self.toggle_selected_tree_header(selection) => {}
                    Some(Selection::OverseerInbox(index)) => self.answer_inbox_selected(index),
                    Some(Selection::Orphan(_)) => self.attach_orphan_selected(),
                    _ => match self.preview {
                        PreviewPane::Terminal => self.attach_shell_selected()?,
                        PreviewPane::Claude => self.attach_claude_selected()?,
                        _ => self.attach_selected()?,
                    },
                },
                KeyCode::Char('r') => self.restart_selected()?,
                KeyCode::Char('g') => management::cycle_selected(self)?,
                KeyCode::Char('m') => self.merge_selected(),
                KeyCode::Char('p') => self.confirm_pr_selected(),
                KeyCode::Char('x') => self.confirm_kill_selected(),
                KeyCode::Char(',') => self.open_settings_editor(),
                KeyCode::Char(ch) if !ch.is_ascii() => {
                    self.show_message("IME is on; switch to ASCII input");
                }
                _ => {}
            },
            Mode::ConfirmKill { .. }
            | Mode::ConfirmRemoveRepo { .. }
            | Mode::ConfirmMerge { .. }
            | Mode::ConfirmCleanup { .. }
            | Mode::ConfirmDeleteBranch { .. }
            | Mode::ConfirmKillOrphan { .. }
            | Mode::ConfirmOverseerPanic
            | Mode::ConfirmOverseerReset
            | Mode::ConfirmInboxDismissAll { .. }
            | Mode::ConfirmOverseerBulkToggle { .. } => unreachable!("handled above"),
        }

        self.clamp_selection();
        Ok(false)
    }
}
