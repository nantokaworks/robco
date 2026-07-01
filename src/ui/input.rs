use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{Result, agent, model::Selection};

use super::{App, Mode, PreviewPane, parse_agent_input};

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
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
                        ) {
                            Ok(agent) => {
                                self.registry.repos[repo_idx].agents.push(agent);
                                self.registry.save()?;
                                self.mode = Mode::Message(format!("created agent {title}"));
                            }
                            Err(err) => self.mode = Mode::Message(err.to_string()),
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
            Mode::ConfirmDeleteBranch { repo, agent } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let repo = *repo;
                    let agent = *agent;
                    self.mode = Mode::Normal;
                    self.delete_agent_branch(repo, agent)?;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.mode = Mode::Message("kept branch".to_string());
                }
                _ => {}
            },
            Mode::Message(_) => self.mode = Mode::Normal,
            Mode::Help => self.mode = Mode::Normal,
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
                KeyCode::PageDown => self.preview_scroll = self.preview_scroll.saturating_add(10),
                KeyCode::PageUp => self.preview_scroll = self.preview_scroll.saturating_sub(10),
                KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(Selection::Repo(repo)) = self.selected_item()
                        && let Some(expanded) = self.expanded.get_mut(repo)
                    {
                        *expanded = true;
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(Selection::Repo(repo)) = self.selected_item()
                        && let Some(expanded) = self.expanded.get_mut(repo)
                    {
                        *expanded = false;
                    }
                }
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
                KeyCode::Char('?') => self.mode = Mode::Help,
                KeyCode::Enter => match self.preview {
                    PreviewPane::Terminal => self.attach_shell_selected()?,
                    PreviewPane::Claude => self.attach_claude_selected()?,
                    _ => self.attach_selected()?,
                },
                KeyCode::Char('r') => self.restart_selected()?,
                KeyCode::Char('s') => self.ship_selected(),
                KeyCode::Char('m') => self.merge_selected(),
                KeyCode::Char('x') => self.confirm_kill_selected(),
                KeyCode::Char(',') => self.open_settings_editor(),
                _ => {}
            },
        }

        self.clamp_selection();
        Ok(false)
    }
}
