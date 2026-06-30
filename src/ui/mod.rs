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
    PromptAgent { repo: usize, input: String },
    Message(String),
}

pub struct App {
    registry: Registry,
    config: Config,
    selected: usize,
    expanded: Vec<bool>,
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

    fn tick(&mut self) {
        for repo in &mut self.registry.repos {
            for agent in &mut repo.agents {
                status::refresh_agent(agent);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        match &mut self.mode {
            Mode::PromptAgent { repo, input } => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    let title = input.trim().to_string();
                    let repo_idx = *repo;
                    self.mode = Mode::Normal;
                    if !title.is_empty() {
                        match agent::create_agent(
                            &self.registry.repos[repo_idx],
                            &title,
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
            Mode::Message(_) => self.mode = Mode::Normal,
            Mode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(true);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = self.visible().len();
                    if self.selected + 1 < len {
                        self.selected += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = self.selected.saturating_sub(1);
                }
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
                    let repo = match self.selected_item() {
                        Some(Selection::Repo(repo)) => repo,
                        Some(Selection::Agent { repo, .. }) => repo,
                        None => 0,
                    };
                    if repo < self.registry.repos.len() {
                        self.mode = Mode::PromptAgent {
                            repo,
                            input: String::new(),
                        };
                    }
                }
                KeyCode::Enter => self.attach_selected()?,
                KeyCode::Char('r') => self.restart_selected()?,
                KeyCode::Char('x') => self.kill_selected()?,
                _ => {}
            },
        }

        self.clamp_selection();
        Ok(false)
    }

    fn attach_selected(&mut self) -> Result<()> {
        let Some(Selection::Agent { repo, agent }) = self.selected_item() else {
            return Ok(());
        };
        let session = self.registry.repos[repo].agents[agent].tmux_session.clone();
        suspend_terminal(|| tmux::attach(&session))?;
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

    fn kill_selected(&mut self) -> Result<()> {
        if let Some(Selection::Agent {
            repo,
            agent: agent_idx,
        }) = self.selected_item()
        {
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
                Mode::PromptAgent { input, .. } => Some(format!("agent title: {input}")),
                Mode::Message(message) => Some(message.clone()),
                Mode::Normal => None,
            };
            tree::draw(frame, app, &visible, message.as_deref());
            preview::draw(frame, app.selected_item(), &app.registry);
        })?;

        if event::poll(Duration::from_millis(app.config.poll_interval_ms))?
            && let Event::Key(key) = event::read()?
            && app.handle_key(key)?
        {
            return Ok(());
        }
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
