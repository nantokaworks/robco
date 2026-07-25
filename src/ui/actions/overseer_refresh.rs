use std::{fs, time::Instant, time::SystemTime};

use crate::{
    config::Config,
    overseer::{ledger::Ledger, logging},
    registry::Registry,
};

use crate::ui::{
    App, inbox,
    overseer::{DECISION_SNAPSHOT_LIMIT, OverseerSnapshot, heartbeat_is_fresh},
};

use super::{background_refresh::StatusResult, background_support::merge_status};

pub(super) struct OverseerResult {
    pub(super) inbox: Vec<inbox::InboxItem>,
    pub(super) snapshot: OverseerSnapshot,
}

pub(super) fn capture_overseer(registry: &Registry, config: &Config) -> OverseerResult {
    let ledger = Ledger::load().unwrap_or_default();
    let decisions = logging::tail(DECISION_SNAPSHOT_LIMIT).unwrap_or_default();
    let reports = inbox::question_reports(registry);
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
    let daemon_version = heartbeat
        .as_ref()
        .and_then(|path| crate::overseer::heartbeat::recorded_version(path));
    OverseerResult {
        inbox,
        snapshot: OverseerSnapshot {
            overseer,
            ledger,
            decisions,
            daemon_alive,
            heartbeat_age,
            daemon_version,
        },
    }
}

impl App {
    pub(super) fn apply_status(&mut self, result: StatusResult, started: Instant) {
        merge_status(&mut self.registry.repos, result.repos);
        self.set_overseer_visibility(result.overseer_visible);
        // A reset or panic-stop may have synchronously refreshed the overseer
        // after this background capture began. Do not let its stale half overwrite
        // that newer snapshot, while still applying the repo and visibility data.
        if self
            .background_refresh
            .overseer_synced_at
            .is_none_or(|at| at <= started)
        {
            self.apply_overseer(result.overseer);
        }
    }

    pub(super) fn apply_overseer(&mut self, result: OverseerResult) {
        self.overseer_snapshot = result.snapshot;
        self.overseer_inbox = result.inbox;
        self.overseer_inbox_selected = self
            .overseer_inbox_selected
            .min(self.overseer_inbox.len().saturating_sub(1));
    }

    pub(in crate::ui) fn refresh_overseer_snapshot(&mut self) {
        // Operator commands only need overseer state immediately; deliberately
        // skip the potentially slow per-repo and per-agent status probes.
        let result = capture_overseer(&self.registry, &self.config);
        self.background_refresh.overseer_synced_at = Some(Instant::now());
        self.apply_overseer(result);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    #[test]
    fn newer_synchronous_overseer_refresh_wins_over_background_capture() {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        let stale_started = Instant::now();

        app.refresh_overseer_snapshot();
        let fresh_dispatch = app.overseer_snapshot.overseer.dispatch_enabled;
        app.apply_status(status_result(!fresh_dispatch), stale_started);
        assert_eq!(
            app.overseer_snapshot.overseer.dispatch_enabled,
            fresh_dispatch
        );

        let newer_started =
            app.background_refresh.overseer_synced_at.unwrap() + Duration::from_nanos(1);
        app.apply_status(status_result(!fresh_dispatch), newer_started);
        assert_eq!(
            app.overseer_snapshot.overseer.dispatch_enabled,
            !fresh_dispatch
        );
    }

    fn status_result(dispatch_enabled: bool) -> StatusResult {
        let mut snapshot = OverseerSnapshot::default();
        snapshot.overseer.dispatch_enabled = dispatch_enabled;
        StatusResult {
            repos: Vec::new(),
            overseer_visible: true,
            overseer: OverseerResult {
                inbox: Vec::new(),
                snapshot,
            },
        }
    }
}
