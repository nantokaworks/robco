use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::{Result, agent, config::Config, model::Selection};

use super::{
    App, Mode, PreviewPane,
    confirm_pr::{ConfirmPrAction, confirm_pr_action},
    help, layout,
};

/// Lines moved per wheel notch; smaller than PageUp/PageDown for fine scrubbing.
const WHEEL_SCROLL_STEP: u16 = 3;

fn parse_agent_input(input: &str, with_prompt: bool) -> (String, Option<String>) {
    if with_prompt {
        let mut parts = input.splitn(2, '|');
        let title = parts.next().unwrap_or_default().trim().to_string();
        let prompt = parts
            .next()
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .map(str::to_string);
        (title, prompt)
    } else {
        (input.trim().to_string(), None)
    }
}

impl App {
    /// Wheel-only mouse handling: the tree scrolls the selection, the preview
    /// scrolls its capture. Clicks and drags are intentionally unhandled, and
    /// any open dialog/prompt swallows mouse input entirely.
    pub(crate) fn handle_mouse(&mut self, event: MouseEvent, area: Rect) {
        if !matches!(self.mode, Mode::Normal) {
            return;
        }
        let up = match event.kind {
            MouseEventKind::ScrollUp => true,
            MouseEventKind::ScrollDown => false,
            _ => return,
        };

        let panes = layout::panes(layout::root(area).body);
        let position = Position::new(event.column, event.row);
        if panes.tree.contains(position) {
            if up {
                self.move_selection_up();
            } else {
                self.move_selection_down();
            }
            self.clamp_selection();
        } else if panes.preview.contains(position) {
            self.scroll_preview(up, WHEEL_SCROLL_STEP);
        }
    }

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

        match &mut self.mode {
            Mode::PromptAgent {
                repo,
                with_prompt,
                input,
            } => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let (title, prompt) = parse_agent_input(input, *with_prompt);
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
                                self.registry.repos[repo_idx].agents.push(agent);
                                self.registry.save()?;
                                self.show_message(format!("created agent {title}"));
                            }
                            Err(err) => self.show_message(err.to_string()),
                        }
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(ch) => input.push(ch),
                _ => {}
            },
            Mode::PromptRepo { input } => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let path = input.trim().to_string();
                    self.mode = Mode::Normal;
                    if !path.is_empty() {
                        self.add_repo_path(&path);
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(ch) => input.push(ch),
                _ => {}
            },
            Mode::ConfirmKill { repo, agent } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let repo = *repo;
                    let agent = *agent;
                    self.mode = Mode::Normal;
                    self.kill_agent(repo, agent)?;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Normal,
                _ => {}
            },
            Mode::ConfirmRemoveRepo { path } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let path = path.clone();
                    self.mode = Mode::Normal;
                    self.remove_pinned_repo(&path)?;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Normal,
                _ => {}
            },
            Mode::ConfirmMerge { repo, agent } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let repo = *repo;
                    let agent = *agent;
                    self.mode = Mode::Normal;
                    self.perform_merge(repo, agent)?;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Normal,
                _ => {}
            },
            Mode::ConfirmPr {
                repo_path,
                agent_id,
                input,
                ..
            } => {
                let action = confirm_pr_action(&mut self.config, input, key, Config::save);
                match action {
                    ConfirmPrAction::Stay => {}
                    ConfirmPrAction::Cancel => self.mode = Mode::Normal,
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
            Mode::ConfirmDeleteBranch { repo, agent } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let repo = *repo;
                    let agent = *agent;
                    self.mode = Mode::Normal;
                    self.delete_agent_branch(repo, agent)?;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.mode = Mode::Normal;
                    self.show_message("kept branch");
                }
                _ => {}
            },
            Mode::ConfirmKillOrphan { session } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let session = session.clone();
                    self.mode = Mode::Normal;
                    self.kill_orphan(&session);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Normal,
                _ => {}
            },
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
                KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(true);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.move_selection_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.move_selection_up();
                }
                KeyCode::PageDown => self.scroll_preview(false, 10),
                KeyCode::PageUp => self.scroll_preview(true, 10),
                KeyCode::Right | KeyCode::Char('l') => match self.selected_item() {
                    Some(Selection::Repo(repo)) => {
                        if let Some(expanded) = self.expanded.get_mut(repo) {
                            *expanded = true;
                        }
                    }
                    Some(Selection::OtherHeader) => self.set_other_collapsed(false),
                    Some(Selection::OrphanHeader) => self.set_orphans_collapsed(false),
                    _ => {}
                },
                KeyCode::Left | KeyCode::Char('h') => match self.selected_item() {
                    Some(Selection::Repo(repo)) => {
                        if let Some(expanded) = self.expanded.get_mut(repo) {
                            *expanded = false;
                        }
                    }
                    Some(Selection::OtherHeader) => self.set_other_collapsed(true),
                    Some(Selection::OrphanHeader) => self.set_orphans_collapsed(true),
                    _ => {}
                },
                KeyCode::Char('n') => {
                    if let Some(repo) = self.selected_repo() {
                        self.mode = Mode::PromptAgent {
                            repo,
                            with_prompt: false,
                            input: String::new(),
                        };
                    } else {
                        self.mode = Mode::PromptRepo {
                            input: String::new(),
                        };
                    }
                }
                KeyCode::Char('N') => {
                    if let Some(repo) = self.selected_repo() {
                        self.mode = Mode::PromptAgent {
                            repo,
                            with_prompt: true,
                            input: String::new(),
                        };
                    }
                }
                KeyCode::Char('a') => {
                    self.mode = Mode::PromptRepo {
                        input: String::new(),
                    };
                }
                KeyCode::Tab => self.toggle_preview(),
                KeyCode::BackTab => self.toggle_preview_back(),
                KeyCode::Char('?') => self.mode = Mode::Help { scroll: 0 },
                KeyCode::Enter => match self.selected_item() {
                    Some(Selection::OtherHeader) => {
                        self.set_other_collapsed(!self.other_collapsed);
                    }
                    Some(Selection::OrphanHeader) => {
                        self.set_orphans_collapsed(!self.orphans_collapsed);
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
                KeyCode::Char('p') => self.confirm_pr_selected(),
                KeyCode::Char('x') => self.confirm_kill_selected(),
                KeyCode::Char(',') => self.open_settings_editor(),
                _ => {}
            },
        }

        self.clamp_selection();
        Ok(false)
    }
}
