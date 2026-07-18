use std::{
    collections::HashSet,
    fs::OpenOptions,
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
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};

use crate::{
    Result,
    overseer::ledger::{Ledger, LedgerPhase},
    config::Config,
    model::Selection,
    notify::{self, WatchTarget},
    registry::Registry,
};

use super::{App, DISCOVERY_INTERVAL, dialog, preview, spinner, tree};

pub fn run(registry: Registry, config: Config, ephemeral_root: Option<PathBuf>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new_with_ephemeral(registry, config, ephemeral_root);
    // Establish live status before the watcher records its first baseline.
    // Otherwise deserialized `worktree_missing = false` can race the first tick.
    app.tick();

    let (notify_tx, notify_rx) = mpsc::channel::<String>();
    let notify_targets = Arc::new(Mutex::new(watch_targets(&app.registry)));
    let notify_running = Arc::new(AtomicBool::new(true));
    let notify_handle = notify::spawn_watcher(
        notify_targets.clone(),
        app.config.notify,
        app.config.openclaw.clone(),
        notify_tx.clone(),
        notify_running.clone(),
        Duration::from_millis(app.config.poll_interval_ms),
    );

    let mut overseer_escalations = escalation_keys();
    let result = run_loop(
        &mut terminal,
        &mut app,
        &notify_rx,
        &notify_tx,
        &notify_targets,
        &mut overseer_escalations,
    );

    notify_running.store(false, Ordering::Relaxed);
    let _ = notify_handle.join();

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    notify_rx: &mpsc::Receiver<String>,
    notify_tx: &mpsc::Sender<String>,
    notify_targets: &Arc<Mutex<Vec<WatchTarget>>>,
    overseer_escalations: &mut HashSet<String>,
) -> Result<()> {
    const MESSAGE_DURATION: Duration = Duration::from_secs(4);

    let tick_interval = Duration::from_millis(app.config.poll_interval_ms);
    let mut last_tick = Instant::now() - tick_interval;
    let mut last_discovery = Instant::now();

    loop {
        if last_tick.elapsed() >= tick_interval {
            app.tick();
            notify_new_escalations(overseer_escalations, notify_tx, app.config.notify.enabled);
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
        app.drain_merge_events()?;
        app.drain_clone_events();

        if app
            .message
            .as_ref()
            .is_some_and(|(_, shown_at)| shown_at.elapsed() >= MESSAGE_DURATION)
        {
            app.message = None;
        }

        if app.force_redraw {
            terminal.autoresize()?;
            terminal.clear()?;
            app.force_redraw = false;
        }
        terminal.draw(|frame| {
            let visible = app.visible();
            let message = app.message.as_ref().map(|(message, _)| message.as_str());
            tree::draw(frame, app, &visible, message);
            preview::draw(
                frame,
                app,
                // Section headers have no preview of their own.
                app.selected_item()
                    .filter(|sel| !matches!(sel, Selection::OtherHeader | Selection::OrphanHeader)),
            );
            dialog::draw(frame, app, &visible);
        })?;

        if event::poll(spinner::FRAME_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.handle_key(key)? {
                        return Ok(());
                    }
                }
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    app.handle_mouse(mouse, Rect::new(0, 0, size.width, size.height));
                }
                _ => {}
            }
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
                worktree_missing: agent.worktree_missing,
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

fn escalation_keys() -> HashSet<String> {
    Ledger::load()
        .map(|ledger| {
            ledger
                .entries
                .into_iter()
                .filter(|entry| entry.phase == LedgerPhase::Escalated)
                .map(|entry| format!("{}@{}", entry.task_id, entry.dispatched_at))
                .collect()
        })
        .unwrap_or_default()
}

fn notify_new_escalations(
    seen: &mut HashSet<String>,
    notify_tx: &mpsc::Sender<String>,
    enabled: bool,
) {
    let Ok(ledger) = Ledger::load() else {
        return;
    };
    for entry in ledger
        .entries
        .into_iter()
        .filter(|entry| entry.phase == LedgerPhase::Escalated)
    {
        let key = format!("{}@{}", entry.task_id, entry.dispatched_at);
        if seen.insert(key) && enabled {
            let body = format!("Overseer escalation: {} ({})", entry.display_id, entry.repo);
            route_notification(notify_tx, "robco", &body);
        }
    }
}

fn route_notification(stdout_tx: &mpsc::Sender<String>, title: &str, body: &str) {
    let payload = notify::osc777(title, body);
    let ttys = notify::attached_client_ttys();
    if ttys.is_empty() {
        let _ = stdout_tx.send(payload);
        return;
    }
    for tty in ttys {
        if let Ok(mut file) = OpenOptions::new().write(true).open(tty) {
            let _ = file.write_all(payload.as_bytes());
            let _ = file.flush();
        }
    }
}
