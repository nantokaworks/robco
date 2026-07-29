use std::{fs, time::Instant, time::SystemTime};

use crate::{
    config::Config,
    model::Status,
    overseer::{
        discord_channels::DiscordChannels, dismissals::Dismissals, ledger::Ledger, logging,
        other_prs::OtherPrs,
    },
    registry::Registry,
    tmux,
};

use crate::ui::{
    App, inbox,
    overseer::{DECISION_SNAPSHOT_LIMIT, OverseerSnapshot, heartbeat_is_fresh},
};

use super::{background_refresh::StatusResult, background_support::merge_status};

pub(super) struct OverseerResult {
    pub(super) inbox: inbox::Inbox,
    pub(super) snapshot: OverseerSnapshot,
}

pub(super) fn capture_overseer(registry: &Registry, config: &Config) -> OverseerResult {
    let ledger = Ledger::load().unwrap_or_default();
    let other_prs = OtherPrs::load().unwrap_or_default();
    let discord_channels = crate::overseer::discord_ops_dir()
        .ok()
        .and_then(|dir| DiscordChannels::load(&dir.join("channels.json")).ok())
        .unwrap_or_default();
    let decisions = logging::tail(DECISION_SNAPSHOT_LIMIT).unwrap_or_default();
    let reports = inbox::question_reports(registry);
    let inbox = inbox::aggregate(
        &ledger,
        &decisions,
        &reports,
        &Dismissals::load().unwrap_or_default(),
        registry,
    );
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
    // A cheap existence probe, not the full capture/classify pipeline an agent
    // or repo main session gets: the row only needs to say whether Enter would
    // attach or create, not track spinner motion.
    let control_session = crate::overseer::control_session_name(&config.tmux_session_prefix);
    let control_status = tmux::has_session(&control_session)
        .ok()
        .and_then(|exists| exists.then_some(Status::Running));
    OverseerResult {
        inbox,
        snapshot: OverseerSnapshot {
            overseer,
            ledger,
            other_prs,
            discord_channels,
            decisions,
            daemon_alive,
            heartbeat_age,
            daemon_version,
            control_status,
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
        // The inbox is re-aggregated and re-sorted newest-first on every refresh,
        // so the row under the cursor is re-anchored by identity. Clamping an
        // index instead would leave the cursor on whatever a newly arrived
        // escalation pushed into that slot — and `y` would approve that worker.
        let selected_identity = self.selected_item().map(|sel| self.item_key(sel));
        self.overseer_snapshot = result.snapshot;
        self.overseer_inbox = result.inbox.items;
        self.overseer_inbox_targets = result.inbox.targets;
        self.restore_selection(selected_identity);
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

    #[test]
    fn a_newly_arrived_escalation_does_not_slide_the_cursor_onto_another_worker() {
        use crate::{
            model::{OverseerCategory, Selection},
            overseer::logging::{DecisionEntry, DecisionKind},
        };

        fn escalation(task: &str, second: u32) -> DecisionEntry {
            let mut decision = DecisionEntry::new(DecisionKind::Escalate, "needs user");
            decision.at = chrono::Utc::now() - chrono::Duration::seconds(second.into());
            decision.task = Some(task.into());
            decision
        }

        fn refresh(app: &mut App, decisions: &[DecisionEntry]) {
            app.apply_overseer(OverseerResult {
                inbox: inbox::aggregate(
                    &Ledger::default(),
                    decisions,
                    &[],
                    &Dismissals::default(),
                    &Registry::default(),
                ),
                snapshot: OverseerSnapshot::default(),
            });
        }

        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.orphans = Vec::new();
        app.set_overseer_category_expanded(OverseerCategory::Inbox, true);

        let older = escalation("task-older", 10);
        refresh(&mut app, std::slice::from_ref(&older));
        app.selected = app
            .visible()
            .iter()
            .position(|row| matches!(row, Selection::OverseerInbox(_)))
            .expect("no inbox item row");
        let identity = app.item_key(app.selected_item().unwrap());

        // The list is re-aggregated newest-first, so this pushes the selected
        // item down a slot. A clamped index would leave the cursor on the new
        // escalation — and `y` would approve a worker the operator never chose.
        refresh(&mut app, &[escalation("task-newer", 0), older]);

        assert_eq!(app.selected_item(), Some(Selection::OverseerInbox(1)));
        assert_eq!(app.item_key(app.selected_item().unwrap()), identity);
    }

    fn status_result(dispatch_enabled: bool) -> StatusResult {
        let mut snapshot = OverseerSnapshot::default();
        snapshot.overseer.dispatch_enabled = dispatch_enabled;
        StatusResult {
            repos: Vec::new(),
            overseer_visible: true,
            overseer: OverseerResult {
                inbox: inbox::Inbox {
                    items: Vec::new(),
                    targets: std::collections::HashSet::new(),
                },
                snapshot,
            },
        }
    }
}
