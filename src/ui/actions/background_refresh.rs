use std::{
    collections::HashMap,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::Instant,
};

use crate::{
    agent, config::Config, model::RepoNode, overseer::logging, registry::Registry, status,
};

use super::{
    background_support::*,
    discovery,
    discovery_capture::{DiscoveryResult, capture_discovery},
    dropr_overlay::{self, OverlayStatus},
    overseer_refresh::{OverseerResult, capture_overseer},
    registry_sync,
};
use crate::ui::{App, list};

type StatusMessage = (Instant, Option<StatusResult>);
type DiscoveryMessage = (Instant, Option<DiscoveryResult>);

pub(in crate::ui) struct BackgroundRefresh {
    status_tx: Sender<StatusMessage>,
    status_rx: Receiver<StatusMessage>,
    discovery_tx: Sender<DiscoveryMessage>,
    discovery_rx: Receiver<DiscoveryMessage>,
    status_in_flight: Option<Instant>,
    discovery_in_flight: Option<Instant>,
    pub(super) overseer_synced_at: Option<Instant>,
    dropr_overlay_load_started_at: Option<Instant>,
    pub(super) dropr_overlay_status: OverlayStatus,
    decision_cursor: Option<Arc<Mutex<logging::DigestCursor>>>,
    registry_saver: RegistrySaver,
}

pub(super) struct StatusResult {
    pub(super) repos: Vec<RepoNode>,
    pub(super) overseer_visible: bool,
    pub(super) overseer: OverseerResult,
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
            overseer_synced_at: None,
            dropr_overlay_load_started_at: None,
            dropr_overlay_status: OverlayStatus::default(),
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
        let started = Instant::now();
        let result = capture_status(clone_registry(&self.registry), &self.config);
        self.apply_status(result, started);
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
        let reload_overlay = self.config.dropr_overlay
            && dropr_overlay::reload_is_due(
                self.background_refresh.dropr_overlay_load_started_at,
                started,
            );
        if reload_overlay {
            // Charge the interval at dispatch: a load that fails, or whose
            // result is discarded, must not re-run the subprocess next tick.
            self.background_refresh.dropr_overlay_load_started_at = Some(started);
        }
        let sender = self.background_refresh.discovery_tx.clone();
        let registry = clone_registry(&self.registry);
        let config = self.config.clone();
        let roots = self.effective_roots().map(PathBuf::from).collect();
        let spawn = std::thread::Builder::new()
            .name("ui-discovery-refresh".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| {
                    capture_discovery(registry, config, roots, reload_overlay)
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
                    self.apply_status(result, started);
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

    fn apply_discovery(&mut self, mut result: DiscoveryResult) {
        if fingerprint(&self.registry) != result.fingerprint {
            return;
        }
        let selected = self.selected_item().map(|item| self.item_key(item));
        let dialog_agent = registry_sync::dialog_agent(&self.mode, &self.registry.repos);
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
        // A freshly loaded overlay is authoritative; carrying the previous link
        // forward would resurrect a workspace that was unlinked outside robco.
        let carry_dropr = result.overlay != Some(OverlayStatus::Loaded);
        carry_runtime(
            &self.registry.repos,
            &mut result.registry.repos,
            carry_dropr,
        );
        if let Some(status) = result.overlay {
            self.background_refresh.dropr_overlay_status = status;
        }
        self.registry = result.registry;
        self.expanded = expanded;
        if let Some(orphans) = result.orphans {
            self.orphans = orphans;
        }
        registry_sync::restore_dialog_agent(&mut self.mode, &self.registry.repos, dialog_agent);
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
        status::refresh_main_drift(repo);
        for tracked in &mut repo.agents {
            status::refresh_agent(&repo.path, tracked, config.auto_accept, processes.as_ref());
        }
    }
    StatusResult {
        overseer: capture_overseer(&registry, config),
        repos: registry.repos,
        overseer_visible: list::overseer_is_visible(),
    }
}
