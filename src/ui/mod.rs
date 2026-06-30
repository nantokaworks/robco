use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    Result, agent,
    config::Config,
    model::{Selection, Status},
    registry::Registry,
    status, tmux,
};

mod preview;
mod tree;

enum Mode {
    Normal,
    PromptAgent {
        repo: usize,
        with_prompt: bool,
        input: String,
    },
    PromptRepo {
        input: String,
    },
    ConfirmKill {
        repo: usize,
        agent: usize,
    },
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewPane {
    Terminal,
    Diff,
    Help,
}

pub struct App {
    pub(crate) registry: Registry,
    pub(crate) config: Config,
    pub(crate) selected: usize,
    pub(crate) expanded: Vec<bool>,
    pub(crate) preview: PreviewPane,
    pub(crate) preview_scroll: u16,
    mode: Mode,
}

impl App {
    pub fn new(registry: Registry, config: Config) -> Self {
        let expanded = vec![true; registry.repos.len()];
        Self {
            registry,
            config,
            selected: 0,
            expanded,
            preview: PreviewPane::Terminal,
            preview_scroll: 0,
            mode: Mode::Normal,
        }
    }

    fn visible(&self) -> Vec<Selection> {
        let mut visible = Vec::new();
        for (repo_idx, repo) in self.registry.repos.iter().enumerate() {
            visible.push(Selection::Repo(repo_idx));
            if self.expanded.get(repo_idx).copied().unwrap_or(true) {
                for (agent_idx, _) in repo.agents.iter().enumerate() {
                    visible.push(Selection::Agent {
                        repo: repo_idx,
                        agent: agent_idx,
                    });
                }
            }
        }
        visible
    }

    fn selected_item(&self) -> Option<Selection> {
        self.visible().get(self.selected).copied()
    }

    fn clamp_selection(&mut self) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    fn move_selection_down(&mut self) {
        let len = self.visible().len();
        if self.selected + 1 < len {
            self.selected += 1;
            self.preview_scroll = 0;
        }
    }

    fn move_selection_up(&mut self) {
        let previous = self.selected;
        self.selected = self.selected.saturating_sub(1);
        if self.selected != previous {
            self.preview_scroll = 0;
        }
    }

    fn tick(&mut self) {
        for repo in &mut self.registry.repos {
            for agent in &mut repo.agents {
                status::refresh_agent(agent, self.config.auto_accept);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
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
            Mode::Message(_) => self.mode = Mode::Normal,
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
                KeyCode::Char('?') => self.preview = PreviewPane::Help,
                KeyCode::Enter => self.attach_selected()?,
                KeyCode::Char('r') => self.restart_selected()?,
                KeyCode::Char('s') => self.ship_selected(),
                KeyCode::Char('x') => self.confirm_kill_selected(),
                _ => {}
            },
        }

        self.clamp_selection();
        Ok(false)
    }

    fn selected_repo(&self) -> Option<usize> {
        match self.selected_item() {
            Some(Selection::Repo(repo)) => Some(repo),
            Some(Selection::Agent { repo, .. }) => Some(repo),
            None => None,
        }
    }

    fn attach_selected(&mut self) -> Result<()> {
        let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        else {
            return Ok(());
        };
        let selected = self.registry.repos[repo].agents[agent_idx].clone();
        match agent::ensure_agent_session(&selected) {
            Ok(()) => {
                let session = selected.tmux_session.clone();
                suspend_terminal(|| tmux::attach(&session))?;
            }
            Err(err) => self.mode = Mode::Message(err.to_string()),
        }
        Ok(())
    }

    fn restart_selected(&mut self) -> Result<()> {
        if let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        {
            let selected = self.registry.repos[repo].agents[agent_idx].clone();
            match agent::restart_agent(&selected) {
                Ok(()) => self.mode = Mode::Message(format!("restarted {}", selected.title)),
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
        Ok(())
    }

    fn confirm_kill_selected(&mut self) {
        if let Some(Selection::Agent { repo, agent }) = self.selected_item() {
            self.mode = Mode::ConfirmKill { repo, agent };
        }
    }

    fn kill_agent(&mut self, repo: usize, agent_idx: usize) -> Result<()> {
        if repo < self.registry.repos.len() && agent_idx < self.registry.repos[repo].agents.len() {
            let selected_repo = self.registry.repos[repo].clone();
            let selected_agent = selected_repo.agents[agent_idx].clone();
            match agent::kill_agent(&selected_repo, &selected_agent) {
                Ok(()) => {
                    self.registry.repos[repo].agents.remove(agent_idx);
                    self.registry.save()?;
                    self.mode = Mode::Message(format!("killed {}", selected_agent.title));
                }
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
        Ok(())
    }

    fn ship_selected(&mut self) {
        if let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        {
            let selected = self.registry.repos[repo].agents[agent_idx].clone();
            match agent::ship_agent(&selected) {
                Ok(()) => self.mode = Mode::Message(format!("pushed {}", selected.branch)),
                Err(err) => self.mode = Mode::Message(err.to_string()),
            }
        }
    }

    fn add_repo_path(&mut self, path: &str) {
        let path = std::path::PathBuf::from(path);
        if !crate::discover::is_git_repo(&path) {
            self.mode = Mode::Message("path is not a git repository".to_string());
            return;
        }

        let Ok(path) = path.canonicalize() else {
            self.mode = Mode::Message("could not resolve path".to_string());
            return;
        };

        if self.registry.repos.iter().any(|repo| repo.path == path) {
            self.mode = Mode::Message("repository already listed".to_string());
            return;
        }

        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
            .to_string();
        let remote_url = crate::git::remote_url(&path).ok();
        self.registry.repos.push(crate::model::RepoNode {
            path,
            name,
            remote_url,
            agents: Vec::new(),
            dropr: None,
        });
        self.expanded.push(true);
        if let Err(err) = self.registry.save() {
            self.mode = Mode::Message(err.to_string());
        } else {
            self.mode = Mode::Message("repository added".to_string());
        }
    }

    fn toggle_preview(&mut self) {
        self.preview = match self.preview {
            PreviewPane::Terminal => PreviewPane::Diff,
            PreviewPane::Diff => PreviewPane::Terminal,
            PreviewPane::Help => PreviewPane::Terminal,
        };
        self.preview_scroll = 0;
    }
}

pub fn run(registry: Registry, config: Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(registry, config);

    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        app.tick();
        terminal.draw(|frame| {
            let visible = app.visible();
            let message = match &app.mode {
                Mode::PromptAgent {
                    input, with_prompt, ..
                } => {
                    if *with_prompt {
                        Some(format!("title | initial prompt: {input}"))
                    } else {
                        Some(format!("agent title: {input}"))
                    }
                }
                Mode::PromptRepo { input } => Some(format!("repo path: {input}")),
                Mode::ConfirmKill { repo, agent } => Some(format!(
                    "kill {}? y/N",
                    app.registry.repos[*repo].agents[*agent].title
                )),
                Mode::Message(message) => Some(message.clone()),
                Mode::Normal => None,
            };
            tree::draw(frame, app, &visible, message.as_deref());
            preview::draw(
                frame,
                app.selected_item(),
                &app.registry,
                app.preview,
                app.preview_scroll,
            );
        })?;

        if event::poll(Duration::from_millis(app.config.poll_interval_ms))?
            && let Event::Key(key) = event::read()?
            && app.handle_key(key)?
        {
            return Ok(());
        }
    }
}

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

fn suspend_terminal(action: impl FnOnce() -> Result<()>) -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    let result = action();
    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    result
}

fn status_style(status: Status) -> ratatui::style::Style {
    use ratatui::style::{Color, Style};
    match status {
        Status::Running => Style::default().fg(Color::Green),
        Status::Waiting => Style::default().fg(Color::Yellow),
        Status::Idle => Style::default().fg(Color::Gray),
        Status::Dead => Style::default().fg(Color::Red),
    }
}
