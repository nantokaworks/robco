use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ansi_to_tui::IntoText;
use ratatui::text::{Line, Text};
use serde_json::json;

use crate::{
    config::Config,
    model::OrphanSession,
    registry::Registry,
    remote::{RemoteClient, RemoteError},
};

use super::Backend;
use crate::ui::{
    actions::{
        background_refresh::StatusResult,
        discovery_capture::{DiscoveryResult, remote_result},
        overseer_refresh::{ControlWatch, OverseerResult},
        preview_capture::PreviewCapture,
    },
    inbox,
    overseer::OverseerSnapshot,
};

#[path = "remote_wire.rs"]
mod wire;
use wire::{DiscoveryWire, OverseerWire};

#[derive(Default)]
struct PaneCache {
    target: Option<(String, u16, u16, u16)>,
    text: Option<Text<'static>>,
    in_flight: bool,
    last_at: Option<Instant>,
}

pub(in crate::ui) struct RemoteBackend {
    client: RemoteClient,
    pane: Arc<Mutex<PaneCache>>,
    error: Arc<Mutex<Option<RemoteError>>>,
}

impl RemoteBackend {
    pub(in crate::ui) fn connect(host: &str) -> Result<Self, RemoteError> {
        Ok(Self {
            client: RemoteClient::ssh(host)?,
            pane: Arc::new(Mutex::new(PaneCache::default())),
            error: Arc::new(Mutex::new(None)),
        })
    }

    pub(in crate::ui) fn initial_snapshot(
        &self,
    ) -> Result<(Registry, Option<Vec<OrphanSession>>), RemoteError> {
        self.discovery()
    }

    pub(in crate::ui) fn client(&self) -> RemoteClient {
        self.client.clone()
    }

    fn discovery(&self) -> Result<(Registry, Option<Vec<OrphanSession>>), RemoteError> {
        let value = self.client.call("robco_discovery_snapshot", json!({}))?;
        let wire: DiscoveryWire = serde_json::from_value(value)
            .map_err(|error| RemoteError::Protocol(format!("discovery snapshot: {error}")))?;
        wire.into_parts()
            .map_err(|error| RemoteError::Protocol(format!("discovery registry: {error}")))
    }

    fn overseer(&self, registry: &Registry) -> Result<(OverseerResult, bool), RemoteError> {
        let value = self.client.call("robco_overseer_snapshot", json!({}))?;
        let wire: OverseerWire = serde_json::from_value(value)
            .map_err(|error| RemoteError::Protocol(format!("overseer snapshot: {error}")))?;
        let control_status = wire.control_status.as_deref().and_then(wire::status);
        let visible = wire.daemon_pid_alive
            || wire.heartbeat_age.is_some()
            || !wire.ledger.entries.is_empty();
        let reports = inbox::agent_session_reports(registry);
        let aggregate = inbox::aggregate(
            &wire.ledger,
            &wire.decisions,
            &reports,
            &wire.dismissals,
            registry,
            &wire.row_summaries,
        );
        Ok((
            OverseerResult {
                inbox: aggregate,
                snapshot: OverseerSnapshot {
                    overseer: wire.overseer,
                    ledger: wire.ledger,
                    other_prs: wire.other_prs,
                    discord_channels: wire.discord_channels,
                    decisions: wire.decisions,
                    daemon_alive: wire.daemon_alive,
                    daemon_version: wire.daemon_version,
                    control_status,
                },
                control_watch: ControlWatch {
                    status: control_status,
                    state: Default::default(),
                },
            },
            visible,
        ))
    }

    fn remember_error(&self, error: RemoteError) {
        *self.error.lock().unwrap() = Some(error);
    }

    fn fallback_overseer(&self) -> OverseerResult {
        OverseerResult {
            inbox: inbox::Inbox {
                items: Vec::new(),
                targets: Default::default(),
            },
            snapshot: OverseerSnapshot::default(),
            control_watch: ControlWatch::default(),
        }
    }

    fn schedule_pane(&self, target: (String, u16, u16, u16)) {
        let mut pane = self.pane.lock().unwrap();
        let due = pane
            .last_at
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(250));
        if pane.in_flight || (pane.target.as_ref() == Some(&target) && !due) {
            return;
        }
        let target_changed = pane.target.as_ref() != Some(&target);
        pane.in_flight = true;
        pane.last_at = Some(Instant::now());
        pane.target = Some(target.clone());
        if target_changed {
            pane.text = None;
        }
        drop(pane);
        let client = self.client.clone();
        let cache = self.pane.clone();
        let error_slot = self.error.clone();
        std::thread::spawn(move || {
            let result = client.call(
                "robco_pane_capture",
                json!({
                    "session": target.0, "width": target.1,
                    "height": target.2, "offset": target.3,
                }),
            );
            let mut cache = cache.lock().unwrap();
            if cache.target.as_ref() == Some(&target) {
                cache.in_flight = false;
                match result {
                    Ok(value) => {
                        cache.text = value.as_str().and_then(|text| text.into_text().ok());
                        *error_slot.lock().unwrap() = None;
                    }
                    Err(error) => {
                        cache.text = Some(Text::from(Line::from(error.to_string())));
                        *error_slot.lock().unwrap() = Some(error);
                    }
                }
            }
        });
    }

    /// Drive the remote capture worker explicitly because the local preview
    /// scheduler must never send a remote session to the local tmux server.
    pub(in crate::ui) fn schedule_remote_pane(
        &self,
        session: &str,
        width: u16,
        height: u16,
        offset: u16,
    ) {
        self.schedule_pane((session.to_string(), width, height, offset));
    }

    #[cfg(test)]
    pub(in crate::ui) fn test(client: RemoteClient) -> Self {
        Self {
            client,
            pane: Arc::new(Mutex::new(PaneCache::default())),
            error: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn test_pane_target(&self) -> Option<(String, u16, u16, u16)> {
        self.pane.lock().unwrap().target.clone()
    }
}

fn cached_pane(pane: &PaneCache, session: &str) -> Option<Text<'static>> {
    pane.target
        .as_ref()
        .is_some_and(|target| target.0 == session)
        .then(|| pane.text.clone())
        .flatten()
}

impl Backend for RemoteBackend {
    fn capture_status(
        &self,
        registry: Registry,
        _config: &Config,
        _watch: &ControlWatch,
    ) -> StatusResult {
        let repos = match self.discovery() {
            Ok((remote, _)) => remote.repos,
            Err(error) => {
                self.remember_error(error);
                registry.repos
            }
        };
        let remote_registry = Registry {
            version: 1,
            repos: repos.clone(),
        };
        let (overseer, overseer_visible) =
            self.overseer(&remote_registry).unwrap_or_else(|error| {
                self.remember_error(error);
                (self.fallback_overseer(), false)
            });
        StatusResult {
            repos,
            overseer_visible,
            overseer,
            auto_cleanup: Vec::new(),
        }
    }

    fn capture_overseer(
        &self,
        registry: &Registry,
        _config: &Config,
        _watch: &ControlWatch,
    ) -> OverseerResult {
        self.overseer(registry)
            .map(|(result, _)| result)
            .unwrap_or_else(|error| {
                self.remember_error(error);
                self.fallback_overseer()
            })
    }

    fn capture_discovery(
        &self,
        registry: Registry,
        _config: Config,
        _roots: Vec<PathBuf>,
        _reload: bool,
    ) -> DiscoveryResult {
        let fingerprint = serde_json::to_vec(&registry).unwrap_or_default();
        match self.discovery() {
            Ok((registry, orphans)) => remote_result(registry, fingerprint, orphans),
            Err(error) => {
                self.remember_error(error.clone());
                remote_result(
                    registry,
                    fingerprint,
                    Some(vec![OrphanSession {
                        name: format!("REMOTE ERROR: {error}"),
                        cwd: PathBuf::from("/"),
                    }]),
                )
            }
        }
    }

    fn cached_tmux(&self, _preview: &PreviewCapture, session: &str) -> Option<Text<'static>> {
        let pane = self.pane.lock().unwrap();
        cached_pane(&pane, session)
    }

    fn cached_diff(&self, _preview: &PreviewCapture, _path: &Path) -> Option<Text<'static>> {
        None
    }
}

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
