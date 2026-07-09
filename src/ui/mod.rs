use std::{
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    Result, agent,
    config::Config,
    model::Selection,
    notify::{self, WatchTarget},
    registry::Registry,
    status,
};

/// How often the launch directory and each repo's worktrees are re-scanned to
/// pick up projects or worktrees created outside robco.
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);

mod actions;
mod dialog;
mod input;
mod layout;
mod list;
mod preview;
pub(crate) mod spinner;
mod summary;
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
    ConfirmMerge {
        repo: usize,
        agent: usize,
    },
    ConfirmDeleteBranch {
        repo: usize,
        agent: usize,
    },
    // Holds the session NAME, not an index into `App::orphans` — the orphan
    // list is rebuilt on every discovery tick, so an index captured when the
    // dialog opened could point at a different session by the time the user
    // confirms. The name pins the kill to exactly what the dialog displayed.
    ConfirmKillOrphan {
        session: String,
    },
    Message(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewPane {
    Info,
    Claude,
    Diff,
    Terminal,
}

/// Preview tabs available for a given tree selection, in display order. The
/// first entry is the default tab used when nothing has been remembered yet.
pub(crate) fn panes_for(selection: Option<Selection>) -> &'static [PreviewPane] {
    match selection {
        Some(Selection::Repo(_)) => &[
            PreviewPane::Info,
            PreviewPane::Claude,
            PreviewPane::Terminal,
        ],
        Some(Selection::Agent { .. }) => &[
            PreviewPane::Claude,
            PreviewPane::Diff,
            PreviewPane::Terminal,
        ],
        Some(Selection::Orphan(_)) => &[PreviewPane::Claude],
        Some(Selection::OtherHeader) | Some(Selection::OrphanHeader) | None => &[],
    }
}

fn default_pane(selection: Option<Selection>) -> PreviewPane {
    panes_for(selection)
        .first()
        .copied()
        .unwrap_or(PreviewPane::Claude)
}

pub struct App {
    pub(crate) registry: Registry,
    pub(crate) config: Config,
    pub(crate) launch_dir: PathBuf,
    pub(crate) selected: usize,
    pub(crate) expanded: Vec<bool>,
    /// Whether the "other locations" section (off-launch-dir repos that still
    /// have agents) is collapsed to its header row.
    other_collapsed: bool,
    /// Live robco-prefixed tmux sessions nothing in the registry accounts for.
    /// Runtime only; rebuilt by [`App::refresh_orphans`] on the discovery tick.
    orphans: Vec<crate::model::OrphanSession>,
    /// Whether the "orphan sessions" section is collapsed to its header row.
    orphans_collapsed: bool,
    pub(crate) preview: PreviewPane,
    /// Remembers the selected preview tab per tree item so switching selection
    /// restores the tab the user last viewed for that item. Keyed by repo path
    /// (repos) or agent id (agents) via [`App::item_key`].
    preview_tabs: HashMap<String, PreviewPane>,
    pub(crate) preview_scroll: u16,
    pub(crate) started: Instant,
    force_redraw: bool,
    mode: Mode,
}

impl App {
    pub fn new(registry: Registry, config: Config, launch_dir: PathBuf) -> Self {
        let expanded = vec![true; registry.repos.len()];
        let mut app = Self {
            registry,
            config,
            launch_dir,
            selected: 0,
            expanded,
            other_collapsed: false,
            orphans: Vec::new(),
            orphans_collapsed: false,
            preview: PreviewPane::Info,
            preview_tabs: HashMap::new(),
            preview_scroll: 0,
            started: Instant::now(),
            force_redraw: false,
            mode: Mode::Normal,
        };
        app.refresh_orphans();
        app.restore_preview();
        app
    }

    fn tick(&mut self) {
        let prefix = self.config.tmux_session_prefix.clone();
        for repo in &mut self.registry.repos {
            let main_session = agent::repo_claude_session_name(&prefix, repo);
            status::refresh_repo_main(&main_session, repo);
            for agent in &mut repo.agents {
                status::refresh_agent(&repo.path, agent, self.config.auto_accept);
            }
        }
    }
}

pub fn run(registry: Registry, config: Config, launch_dir: PathBuf) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (notify_tx, notify_rx) = mpsc::channel::<String>();
    let notify_targets = Arc::new(Mutex::new(watch_targets(&registry)));
    let notify_running = Arc::new(AtomicBool::new(true));
    let notify_handle = notify::spawn_watcher(
        notify_targets.clone(),
        config.notify,
        config.openclaw.clone(),
        notify_tx,
        notify_running.clone(),
        Duration::from_millis(config.poll_interval_ms),
    );
    let mut app = App::new(registry, config, launch_dir);

    let result = run_loop(&mut terminal, &mut app, &notify_rx, &notify_targets);

    notify_running.store(false, Ordering::Relaxed);
    let _ = notify_handle.join();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    notify_rx: &mpsc::Receiver<String>,
    notify_targets: &Arc<Mutex<Vec<WatchTarget>>>,
) -> Result<()> {
    let tick_interval = Duration::from_millis(app.config.poll_interval_ms);
    let mut last_tick = Instant::now() - tick_interval;
    let mut last_discovery = Instant::now();

    loop {
        if last_tick.elapsed() >= tick_interval {
            app.tick();
            last_tick = Instant::now();
        }
        if last_discovery.elapsed() >= DISCOVERY_INTERVAL {
            app.refresh_discovery();
            last_discovery = Instant::now();
        }
        if let Ok(mut targets) = notify_targets.lock() {
            *targets = watch_targets(&app.registry);
        }
        drain_stdout_notifications(notify_rx, app);

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
                app,
                // Section headers have no preview of their own.
                app.selected_item()
                    .filter(|sel| !matches!(sel, Selection::OtherHeader | Selection::OrphanHeader)),
            );
            dialog::draw(frame, app, &visible);
        })?;

        if event::poll(spinner::FRAME_INTERVAL)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && app.handle_key(key)?
        {
            return Ok(());
        }
    }
}

fn watch_targets(registry: &Registry) -> Vec<WatchTarget> {
    registry
        .repos
        .iter()
        .flat_map(|repo| {
            repo.agents.iter().map(|agent| WatchTarget {
                tmux_session: agent.tmux_session.clone(),
                agent_id: agent.id.clone(),
                repo: repo.name.clone(),
                label: agent.title.clone(),
                status: agent.status,
            })
        })
        .collect()
}

fn drain_stdout_notifications(notify_rx: &mpsc::Receiver<String>, app: &mut App) {
    let mut out = io::stdout();
    for payload in notify_rx.try_iter() {
        let _ = out.write_all(payload.as_bytes());
        let _ = out.flush();
        app.force_redraw = true;
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
