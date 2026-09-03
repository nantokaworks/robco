use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    Result, agent,
    config::Config,
    locale::{fmt, t},
    model::Selection,
};

use super::{
    App, Mode, PreviewPane,
    confirm_pr::{ConfirmPrAction, confirm_pr_action},
    help,
    text_input::TextInput,
};

mod confirm;
mod dropr_task_body;
mod dropr_task_drill;
mod host_connect;
mod inbox_dismiss;
mod inbox_respond;
mod mouse;
mod overseer;
#[cfg(test)]
pub(in crate::ui) use overseer::remote_chat_target;
mod prompt_agent;
mod tree_nav;

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        let server = self.config.tmux_server.clone();
        self.handle_key_with_pr_sender(key, |session, prompt| {
            crate::tmux::send_literal_text(&server, session, prompt)
                .and_then(|()| crate::tmux::send_keys(&server, session, &["Enter"]))
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
                                    fmt(self.locale, "created agent {}", &[&title])
                                } else {
                                    // The worktree and tmux session are up; only
                                    // the repo row is gone from the stored
                                    // registry, so say so rather than reporting
                                    // a clean create that left nothing behind.
                                    fmt(
                                        self.locale,
                                        "created agent {}, but its repository is no longer registered",
                                        &[&title],
                                    )
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
            Mode::PromptRenameRepo { path, input } => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let new_name = input.text().trim().to_string();
                    let path = path.clone();
                    self.mode = Mode::Normal;
                    if !new_name.is_empty() {
                        self.rename_repo(&path, &new_name);
                    }
                }
                _ => {
                    input.handle_key(key);
                }
            },
            Mode::PromptHostConnect { .. } => host_connect::handle_prompt(self, key),
            Mode::PromptOverseer { input } => match overseer::prompt_action(input, key) {
                overseer::PromptAction::Stay => {}
                overseer::PromptAction::Cancel => self.mode = Mode::Normal,
                overseer::PromptAction::Submit(instruction) => {
                    self.mode = Mode::Normal;
                    self.instruct_overseer(&instruction);
                }
            },
            Mode::PromptSession {
                session,
                host,
                input,
            } => match overseer::prompt_action(input, key) {
                overseer::PromptAction::Stay => {}
                overseer::PromptAction::Cancel => self.mode = Mode::Normal,
                overseer::PromptAction::Submit(instruction) => {
                    let session = session.clone();
                    let host = host.clone();
                    self.mode = Mode::Normal;
                    self.instruct_prompt_session(host.as_ref(), &session, &instruction);
                }
            },
            Mode::PromptInbox { item, input } => match overseer::prompt_action(input, key) {
                overseer::PromptAction::Stay => {}
                overseer::PromptAction::Cancel => self.mode = Mode::Normal,
                overseer::PromptAction::Submit(answer) => {
                    let item = item.clone();
                    self.mode = Mode::Normal;
                    self.answer_inbox(&item, &answer);
                }
            },
            Mode::PrPrecheck { .. } => {
                if matches!(key.code, KeyCode::Esc) {
                    self.pr_precheck_job = None;
                    self.mode = Mode::Normal;
                }
            }
            Mode::ConfirmPr {
                repo_path,
                agent_id,
                input,
                approval_head,
                ..
            } => {
                let action = confirm_pr_action(&mut self.config, input, key, Config::save);
                match action {
                    ConfirmPrAction::Stay => {}
                    ConfirmPrAction::Cancel => self.mode = Mode::Normal,
                    ConfirmPrAction::Submit(prompt) => {
                        let repo_path = repo_path.clone();
                        let agent_id = agent_id.clone();
                        let approval_head = approval_head.clone();
                        self.mode = Mode::Normal;
                        self.request_pr(&repo_path, &agent_id, &prompt, approval_head, send)?;
                    }
                    ConfirmPrAction::Saved(result) => match result {
                        Ok(()) => self.show_message(t(self.locale, "saved PR prompt to config")),
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
            // The task-body reading dialog (dropr:501); its own key routing
            // lives in `dropr_task_body` to keep this file under the
            // line-count limit.
            Mode::TaskBody { .. } => dropr_task_body::handle(self, key.code),
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
                code if host_connect::handle_normal(self, code) => {}
                code if overseer::handle_normal(self, code) => {}
                code if dropr_task_drill::handle_normal(self, code) => {}
                KeyCode::Char('q') | KeyCode::Esc => {
                    let merging = self.merging_branches();
                    let launching = self.launching_tasks();
                    if !merging.is_empty() {
                        self.show_message(fmt(
                            self.locale,
                            "merge in progress: {} — wait or ctrl-c to force quit",
                            &[&merging.join(", ")],
                        ));
                    } else if !launching.is_empty() {
                        self.show_message(fmt(
                            self.locale,
                            "launch in progress: {} — wait or ctrl-c to force quit",
                            &[&launching.join(", ")],
                        ));
                    } else if matches!(key.code, KeyCode::Esc) && self.dismiss_merge_outcome() {
                        self.show_message(t(self.locale, "dismissed merge notice"));
                    } else {
                        return Ok(true);
                    }
                }
                // Matched ahead of the plain arrow arms below: those dispatch on
                // `key.code` alone, so without this a shift-modified arrow would
                // fall into them and move the cursor instead of the row.
                KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.move_selected_repo(-1);
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.move_selected_repo(1);
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
                    Some(Selection::OverseerAi) => self.attach_control_selected(),
                    Some(Selection::OverseerInbox(index)) => self.answer_inbox_selected(index),
                    Some(Selection::DiscordChannel(index)) => {
                        self.attach_discord_channel_selected(index);
                    }
                    Some(
                        selection @ (Selection::RemoteControlAi(_)
                        | Selection::RemoteDiscordChannel { .. }),
                    ) => {
                        overseer::attach_remote_chat(self, selection);
                    }
                    // The drill-down's entry point (dropr:475): INFO is the
                    // only tab that can show the task list, so this leaves
                    // every other tab's `enter` (Claude/Terminal attach)
                    // untouched.
                    Some(Selection::Repo(_)) if self.preview == PreviewPane::Info => {
                        self.enter_dropr_task_list();
                    }
                    Some(Selection::Orphan(_)) => self.attach_orphan_selected(),
                    _ => match self.preview {
                        PreviewPane::Terminal => self.attach_shell_selected()?,
                        PreviewPane::Claude => self.attach_claude_selected()?,
                        _ => self.attach_selected()?,
                    },
                },
                KeyCode::Char('r') => self.restart_selected()?,
                KeyCode::Char('m') => self.merge_selected(),
                KeyCode::Char('u') => self.update_branch_selected(),
                KeyCode::Char('c') => self.checkout_main_selected(),
                KeyCode::Char('C') => self.clear_chat_selected(),
                // Only the Claude tab has a live session to type into (see
                // `panes_for`: it is offered only for Repo/Agent/Orphan rows),
                // so gating on the tab rather than the selection type covers
                // exactly those rows without naming them here.
                KeyCode::Char('i') if self.preview == PreviewPane::Claude => {
                    match super::scrollback::live_session(self) {
                        Some(session) => {
                            self.mode = Mode::PromptSession {
                                session,
                                host: None,
                                input: TextInput::new(),
                            };
                        }
                        None => {
                            self.show_message(t(self.locale, "no live session for this tab"));
                        }
                    }
                }
                KeyCode::Char('p') => self.confirm_pr_selected(),
                KeyCode::Char('x') => self.confirm_kill_selected(),
                KeyCode::Char('g') => self.open_rename_prompt(),
                KeyCode::Char(',') => self.open_settings_editor(),
                KeyCode::Char(ch) if !ch.is_ascii() => {
                    self.show_message(t(self.locale, "IME is on; switch to ASCII input"));
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
            | Mode::ConfirmDaemonStop
            | Mode::ConfirmInboxDismissAll { .. }
            | Mode::ConfirmRemoveDiscordChannel { .. }
            | Mode::ConfirmClearChat { .. } => unreachable!("handled above"),
        }

        self.clamp_selection();
        Ok(false)
    }
}
