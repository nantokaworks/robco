use std::{
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
    config::Config,
    model::Selection,
    notify::{self, WatchTarget},
    overseer::logging,
    registry::Registry,
};

use super::{App, DISCOVERY_INTERVAL, dialog, layout, preview, spinner, tree};

pub fn run(registry: Registry, config: Config, ephemeral_root: Option<PathBuf>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new_with_ephemeral(registry, config, ephemeral_root);
    // Establish live status before the watcher records its first baseline.
    // Otherwise deserialized `worktree_missing = false` can race the first tick.
    app.initial_tick();

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

    let decision_cursor = logging::DigestCursor::at_end()?;
    app.set_decision_cursor(decision_cursor);
    let result = run_loop(
        &mut terminal,
        &mut app,
        &notify_rx,
        &notify_tx,
        &notify_targets,
    );
    app.shutdown_saves();

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
) -> Result<()> {
    const MESSAGE_DURATION: Duration = Duration::from_secs(4);

    let tick_interval = Duration::from_millis(app.config.poll_interval_ms);
    let mut last_tick = Instant::now() - tick_interval;
    let mut last_discovery = Instant::now();

    loop {
        app.ingest_background_refreshes();
        app.ingest_preview_captures();
        if let Ok(size) = terminal.size() {
            app.schedule_preview_capture(Rect::new(0, 0, size.width, size.height));
        }
        if last_tick.elapsed() >= tick_interval {
            app.schedule_status_refresh(notify_tx.clone());
            last_tick = Instant::now();
        }
        if last_discovery.elapsed() >= DISCOVERY_INTERVAL {
            app.schedule_discovery_refresh();
            last_discovery = Instant::now();
        }
        if let Ok(mut targets) = notify_targets.lock() {
            *targets = watch_targets(&app.registry);
        }
        drain_stdout_notifications(notify_rx, app);
        app.drain_merge_events()?;
        app.drain_pr_precheck_events();
        app.drain_task_launch_events();
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
            let footer_caret = layout::footer(layout::root(frame.area()).footer).caret;
            tree::draw(frame, app, &visible, message);
            preview::draw(
                frame,
                app,
                // Section headers have no preview of their own.
                app.selected_item()
                    .filter(|sel| !matches!(sel, Selection::OtherHeader | Selection::OrphanHeader)),
            );
            // Pinning the Normal-mode cursor gives IME preedit a stable home
            // per #189; #201 moves it right of the ROBCO ident to keep the brand clean.
            let cursor = dialog::draw(frame, app).unwrap_or(footer_caret);
            frame.set_cursor_position(cursor);
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
