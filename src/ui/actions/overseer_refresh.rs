use std::{fs, time::Instant, time::SystemTime};

use crate::{
    config::Config,
    model::Status,
    overseer::{
        discord_channels::DiscordChannels, dismissals::Dismissals, ledger::Ledger, logging,
        other_prs::OtherPrs,
    },
    registry::Registry,
    status::{self, WatchStatusState},
};

use crate::ui::{
    App, inbox,
    overseer::{DECISION_SNAPSHOT_LIMIT, OverseerSnapshot, heartbeat_is_fresh},
};

use super::{background_refresh::StatusResult, background_support::merge_status, lifecycle};

/// Persisted classification state for the Overseer's own control tmux
/// session, carried across refresh ticks the same way a [`WatchStatusState`]
/// is carried inside `AgentNode` / `RepoNode` — but the control row has no
/// long-lived model node of its own to hold it, so it lives on
/// [`super::background_refresh::BackgroundRefresh`] instead. `status` is the
/// previously reported glyph, fed back in so a transient `tmux` failure can
/// keep it rather than flipping the row (see
/// [`crate::status::classify_session_status`]).
#[derive(Debug, Default, Clone)]
pub(in crate::ui) struct ControlWatch {
    pub(in crate::ui) status: Option<Status>,
    pub(in crate::ui) state: WatchStatusState,
}

pub(in crate::ui) struct OverseerResult {
    pub(in crate::ui) inbox: inbox::Inbox,
    pub(in crate::ui) snapshot: OverseerSnapshot,
    pub(in crate::ui) control_watch: ControlWatch,
}

pub(in crate::ui) fn capture_overseer(
    registry: &Registry,
    config: &Config,
    control_watch: &ControlWatch,
) -> OverseerResult {
    let ledger = Ledger::load().unwrap_or_default();
    let other_prs = OtherPrs::load().unwrap_or_default();
    let discord_channels = crate::overseer::discord_ops_dir()
        .ok()
        .and_then(|dir| DiscordChannels::load(&dir.join("channels.json")).ok())
        .unwrap_or_default();
    let decisions = logging::tail(DECISION_SNAPSHOT_LIMIT).unwrap_or_default();
    let reports = inbox::agent_session_reports(registry);
    let inbox = inbox::aggregate(
        &ledger,
        &decisions,
        &reports,
        &Dismissals::load().unwrap_or_default(),
        registry,
        &crate::overseer::row_summaries::RowSummaries::load().unwrap_or_default(),
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
    // Classify via the same capture/classify pipeline an agent or repo main
    // session gets, so the row reflects real activity instead of merely
    // whether the session exists (dropr:420).
    let control_session = crate::overseer::control_session_name(&config.tmux_session_prefix);
    let mut watch_state = control_watch.state.clone();
    let control_status =
        status::classify_session_status(&control_session, control_watch.status, &mut watch_state);
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
        control_watch: ControlWatch {
            status: control_status,
            state: watch_state,
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
        self.run_auto_cleanup(result.auto_cleanup);
    }

    /// Runs the existing `CleanOnly` sequence for every candidate the
    /// background capture found — see
    /// `auto_cleanup::merged_cleanup_candidates`. Skips a repository that
    /// already has a merge job running: `start_cleanup` would only show a
    /// "merge already in progress" toast for a case nobody asked about,
    /// since nothing here is operator-initiated (dropr:563).
    fn run_auto_cleanup(&mut self, candidates: Vec<(std::path::PathBuf, String)>) {
        for (repo_path, agent_id) in candidates {
            if self.merge_job(&repo_path).is_some() {
                continue;
            }
            let Some((repo, agent)) =
                lifecycle::resolve_agent(&self.registry.repos, &repo_path, &agent_id)
            else {
                continue;
            };
            self.start_cleanup(repo, agent);
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
        self.background_refresh.control_watch = result.control_watch;
        self.restore_selection(selected_identity);
    }

    pub(in crate::ui) fn refresh_overseer_snapshot(&mut self) {
        // Operator commands only need overseer state immediately; deliberately
        // skip the potentially slow per-repo and per-agent status probes.
        let result = self.backend.capture_overseer(
            &self.registry,
            &self.config,
            &self.background_refresh.control_watch,
        );
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
        let fresh_auto_merge = app.overseer_snapshot.overseer.auto_merge;
        app.apply_status(status_result(!fresh_auto_merge), stale_started);
        assert_eq!(app.overseer_snapshot.overseer.auto_merge, fresh_auto_merge);

        let newer_started =
            app.background_refresh.overseer_synced_at.unwrap() + Duration::from_nanos(1);
        app.apply_status(status_result(!fresh_auto_merge), newer_started);
        assert_eq!(app.overseer_snapshot.overseer.auto_merge, !fresh_auto_merge);
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
                    &crate::overseer::row_summaries::RowSummaries::default(),
                ),
                snapshot: OverseerSnapshot::default(),
                control_watch: ControlWatch::default(),
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

    fn status_result(auto_merge: bool) -> StatusResult {
        let mut snapshot = OverseerSnapshot::default();
        snapshot.overseer.auto_merge = auto_merge;
        StatusResult {
            repos: Vec::new(),
            overseer_visible: true,
            auto_cleanup: Vec::new(),
            overseer: OverseerResult {
                inbox: inbox::Inbox {
                    items: Vec::new(),
                    targets: std::collections::HashSet::new(),
                },
                snapshot,
                control_watch: ControlWatch::default(),
            },
        }
    }
}

#[cfg(test)]
#[path = "overseer_refresh_auto_cleanup_tests.rs"]
mod auto_cleanup_tests;
