use std::{
    io,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    Result,
    config::Config,
    model::{Selection, Status},
    registry::Registry,
    status,
};

mod actions;
mod dialog;
mod input;
mod layout;
mod preview;
mod spinner;
mod theme;
mod tree;

enum Mode {
    Normal,
    Help,
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
    ConfirmDeleteBranch {
        repo: usize,
        agent: usize,
    },
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewPane {
    Claude,
    Diff,
    Terminal,
}

pub struct App {
    pub(crate) registry: Registry,
    pub(crate) config: Config,
    pub(crate) selected: usize,
    pub(crate) expanded: Vec<bool>,
    pub(crate) preview: PreviewPane,
    pub(crate) preview_scroll: u16,
    pub(crate) started: Instant,
    force_redraw: bool,
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
            preview: PreviewPane::Claude,
            preview_scroll: 0,
            started: Instant::now(),
            force_redraw: false,
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
                status::refresh_agent(&repo.path, agent, self.config.auto_accept);
            }
        }
    }

    fn selected_repo(&self) -> Option<usize> {
        match self.selected_item() {
            Some(Selection::Repo(repo)) => Some(repo),
            Some(Selection::Agent { repo, .. }) => Some(repo),
            None => None,
        }
    }

    fn toggle_preview(&mut self) {
        self.preview = match self.preview {
            PreviewPane::Claude => PreviewPane::Diff,
            PreviewPane::Diff => PreviewPane::Terminal,
            PreviewPane::Terminal => PreviewPane::Claude,
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
    let tick_interval = Duration::from_millis(app.config.poll_interval_ms);
    let mut last_tick = Instant::now() - tick_interval;

    loop {
        if last_tick.elapsed() >= tick_interval {
            app.tick();
            last_tick = Instant::now();
        }

        if app.force_redraw {
            terminal.autoresize()?;
            terminal.clear()?;
            app.force_redraw = false;
        }
        terminal.draw(|frame| {
            let visible = app.visible();
            let message = match &app.mode {
                Mode::Message(message) => Some(message.clone()),
                _ => None,
            };
            tree::draw(frame, app, &visible, message.as_deref());
            preview::draw(
                frame,
                app.selected_item(),
                &app.registry,
                app.preview,
                app.preview_scroll,
            );
            dialog::draw(frame, app, &visible);
        })?;

        if event::poll(spinner::FRAME_INTERVAL)?
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
    theme::DEFAULT.status_style(status)
}
