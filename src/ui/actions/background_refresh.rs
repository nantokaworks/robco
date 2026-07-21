use std::{
    collections::{HashMap, HashSet},
    fs,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::{Instant, SystemTime},
};

use crate::{
    agent,
    config::Config,
    discover, dropr, git,
    model::{OrphanSession, RepoNode},
    overseer::{ledger::Ledger, logging},
    registry::Registry,
    status,
    subagents::claude::ClaudeSubagentReader,
};

use super::{background_support::*, children, discovery, orphans, subagents};
use crate::ui::{
    App, inbox, list,
    overseer::{OverseerSnapshot, heartbeat_is_fresh},
};

type StatusMessage = (Instant, Option<StatusResult>);
type DiscoveryMessage = (Instant, Option<DiscoveryResult>);

pub(in crate::ui) struct BackgroundRefresh {
    status_tx: Sender<StatusMessage>,
    status_rx: Receiver<StatusMessage>,
    discovery_tx: Sender<DiscoveryMessage>,
    discovery_rx: Receiver<DiscoveryMessage>,
    status_in_flight: Option<Instant>,
    discovery_in_flight: Option<Instant>,
    decision_cursor: Option<Arc<Mutex<logging::DigestCursor>>>,
    registry_saver: RegistrySaver,
}

struct StatusResult {
    repos: Vec<RepoNode>,
    overseer_visible: bool,
    inbox: Vec<inbox::InboxItem>,
    snapshot: OverseerSnapshot,
}

struct DiscoveryResult {
    registry: Registry,
    fingerprint: Vec<u8>,
    orphans: Option<Vec<OrphanSession>>,
    save: bool,
}

impl BackgroundRefresh {
    pub(in crate::ui) fn new() -> Self {
        let (status_tx, status_rx) = mpsc::channel();
        let (discovery_tx, discovery_rx) = mpsc::channel();
        Self {
            status_tx,
            status_rx,
            discovery_tx,
            discovery_rx,
            status_in_flight: None,
            discovery_in_flight: None,
            decision_cursor: None,
            registry_saver: RegistrySaver::new(),
        }
    }
}

impl App {
    pub(in crate::ui) fn set_decision_cursor(&mut self, cursor: logging::DigestCursor) {
        self.background_refresh.decision_cursor = Some(Arc::new(Mutex::new(cursor)));
    }

    pub(in crate::ui) fn initial_tick(&mut self) {
        let result = capture_status(clone_registry(&self.registry), &self.config);
        self.apply_status(result);
    }

    pub(in crate::ui) fn schedule_status_refresh(&mut self, notify_tx: Sender<String>) {
        // A hung worker intentionally holds its slot until it sends a result, preventing
        // replacement workers and their child processes from accumulating.
        if self.background_refresh.status_in_flight.is_some() {
            return;
        }
        let Some(cursor) = self.background_refresh.decision_cursor.clone() else {
            return;
        };
        let started = Instant::now();
        self.background_refresh.status_in_flight = Some(started);
        let sender = self.background_refresh.status_tx.clone();
        let registry = clone_registry(&self.registry);
        let config = self.config.clone();
        let spawn = std::thread::Builder::new()
            .name("ui-status-refresh".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    if let Ok(mut cursor) = cursor.lock() {
                        notify_new_decisions(&mut cursor, &notify_tx, config.notify.enabled);
                    }
                    capture_status(registry, &config)
                }))
                .ok();
                let _ = sender.send((started, result));
            });
        if spawn.is_err() {
            self.background_refresh.status_in_flight = None;
        }
    }

    pub(in crate::ui) fn schedule_discovery_refresh(&mut self) {
        // A hung worker intentionally holds its slot until it sends a result, preventing
        // replacement workers and their child processes from accumulating.
        if self.background_refresh.discovery_in_flight.is_some() {
            return;
        }
        let started = Instant::now();
        self.background_refresh.discovery_in_flight = Some(started);
        let sender = self.background_refresh.discovery_tx.clone();
        let registry = clone_registry(&self.registry);
        let config = self.config.clone();
        let roots = self.effective_roots().map(PathBuf::from).collect();
        let spawn = std::thread::Builder::new()
            .name("ui-discovery-refresh".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    capture_discovery(registry, config, roots)
                }))
                .ok();
                let _ = sender.send((started, result));
            });
        if spawn.is_err() {
            self.background_refresh.discovery_in_flight = None;
        }
    }

    pub(in crate::ui) fn ingest_background_refreshes(&mut self) {
        while let Ok((started, result)) = self.background_refresh.status_rx.try_recv() {
            if self.background_refresh.status_in_flight == Some(started) {
                self.background_refresh.status_in_flight = None;
                if let Some(result) = result {
                    self.apply_status(result);
                }
            }
        }
        while let Ok((started, result)) = self.background_refresh.discovery_rx.try_recv() {
            if self.background_refresh.discovery_in_flight == Some(started) {
                self.background_refresh.discovery_in_flight = None;
                if let Some(result) = result {
                    self.apply_discovery(result);
                }
            }
        }
    }

    fn apply_status(&mut self, result: StatusResult) {
        merge_status(&mut self.registry.repos, result.repos);
        self.set_overseer_visibility(result.overseer_visible);
        self.overseer_snapshot = result.snapshot;
        self.overseer_inbox = result.inbox;
        self.overseer_inbox_selected = self
            .overseer_inbox_selected
            .min(self.overseer_inbox.len().saturating_sub(1));
    }

    fn apply_discovery(&mut self, mut result: DiscoveryResult) {
        if fingerprint(&self.registry) != result.fingerprint {
            return;
        }
        let selected = self.selected_item().map(|item| self.item_key(item));
        let expanded_by_path = self
            .registry
            .repos
            .iter()
            .zip(&self.expanded)
            .map(|(repo, value)| (discovery::path_key(&repo.path), *value))
            .collect::<HashMap<_, _>>();
        let expanded = result
            .registry
            .repos
            .iter()
            .map(|repo| {
                expanded_by_path
                    .get(&discovery::path_key(&repo.path))
                    .copied()
                    .unwrap_or(true)
            })
            .collect();
        carry_runtime(&self.registry.repos, &mut result.registry.repos);
        self.registry = result.registry;
        self.expanded = expanded;
        if let Some(orphans) = result.orphans {
            self.orphans = orphans;
        }
        self.restore_selection(selected);
        if result.save {
            self.background_refresh
                .registry_saver
                .save(clone_registry(&self.registry), result.fingerprint);
        }
        self.refresh_dropr_tasks(false);
    }

    pub(in crate::ui) fn shutdown_saves(&mut self) {
        self.background_refresh.registry_saver.shutdown();
    }
}

fn capture_status(mut registry: Registry, config: &Config) -> StatusResult {
    let processes = config
        .process_indicator
        .then(status::proc::ProcSnapshot::capture)
        .and_then(crate::Result::ok);
    for repo in &mut registry.repos {
        let session = agent::repo_claude_session_name(&config.tmux_session_prefix, repo);
        status::refresh_repo_main(&session, repo, processes.as_ref());
        for tracked in &mut repo.agents {
            status::refresh_agent(&repo.path, tracked, config.auto_accept, processes.as_ref());
        }
    }
    let ledger = Ledger::load().unwrap_or_default();
    let decisions = logging::tail(200).unwrap_or_default();
    let reports = inbox::question_reports(&registry);
    let inbox = inbox::aggregate(&ledger, &decisions, &reports);
    let heartbeat = crate::overseer::heartbeat_path().ok();
    let heartbeat_age = heartbeat
        .as_ref()
        .and_then(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    // Reload the overseer config from disk so the Info pane reflects flips made
    // outside the `,` settings editor (the `S` panic-stop, the daemon, Discord,
    // external edits). `app.config` only refreshes via the editor, so reading it
    // here would keep showing stale `dispatch` / `auto-merge` state. Fall back to
    // the in-memory copy when the reload fails.
    //
    // Load it before the liveness check so the whole snapshot — including the
    // heartbeat-freshness window that decides `daemon_alive` — is derived from
    // one consistent view of the config rather than mixing fresh and stale.
    let overseer = Config::load()
        .map(|reloaded| reloaded.overseer)
        .unwrap_or_else(|_| config.overseer.clone());
    let daemon_alive = crate::overseer::daemon_pid_alive()
        && heartbeat
            .as_ref()
            .is_some_and(|path| heartbeat_is_fresh(path, overseer.poll_interval_secs));
    StatusResult {
        repos: registry.repos,
        overseer_visible: list::overseer_is_visible(),
        inbox,
        snapshot: OverseerSnapshot {
            overseer,
            ledger,
            decisions,
            daemon_alive,
            heartbeat_age,
        },
    }
}

fn capture_discovery(
    mut registry: Registry,
    config: Config,
    roots: Vec<PathBuf>,
) -> DiscoveryResult {
    let fingerprint = fingerprint(&registry);
    if !config.subagent_indicator {
        subagents::ingest(
            &mut registry.repos,
            false,
            &ClaudeSubagentReader::default(),
            SystemTime::now(),
        );
    }
    let mut discovered = discover::discover_all(roots.iter().map(PathBuf::as_path));
    let removed = discovery::prune_unmanaged(&mut registry.repos, &config.worktree_root);
    let expected = discovered
        .iter()
        .map(|repo| discovery::path_key(&repo.path))
        .chain(
            registry
                .repos
                .iter()
                .filter(|repo| !repo.agents.is_empty() || repo.pinned)
                .map(|repo| discovery::path_key(&repo.path)),
        )
        .collect::<HashSet<_>>();
    let current = registry
        .repos
        .iter()
        .map(|repo| discovery::path_key(&repo.path))
        .collect::<HashSet<_>>();
    let repos_changed = expected != current;
    if repos_changed {
        if config.dropr_overlay {
            let overlay = dropr::DroprOverlay::load_best_effort();
            for repo in &mut discovered {
                if let Some(remote) = &repo.remote_url {
                    repo.dropr = overlay.find_by_repo_url(remote).cloned();
                }
            }
        }
        registry.merge_discovered(discovered);
    }
    let mut added = false;
    for repo in &mut registry.repos {
        if let Ok(worktrees) = git::list_worktrees(&repo.path) {
            added |= children::reconcile(repo, &config, worktrees).0;
        }
    }
    if config.subagent_indicator {
        subagents::ingest(
            &mut registry.repos,
            true,
            &ClaudeSubagentReader::default(),
            SystemTime::now(),
        );
    }
    let found_orphans = orphans::discover_orphans(
        &registry.repos,
        &config.tmux_session_prefix,
        &config.worktree_root,
    );
    DiscoveryResult {
        registry,
        fingerprint,
        orphans: found_orphans,
        save: repos_changed || added || removed,
    }
}
