use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

#[cfg(test)]
use chrono::Utc;
use ratatui::text::Text;

use crate::{
    model::{ManagementMode, MergeLifecycle, OverseerCategory, Status},
    overseer::{
        config::OverseerConfig, discord_channels::DiscordChannels, ledger::Ledger,
        logging::DecisionEntry, other_prs::OtherPrs,
    },
};

use super::App;

mod categories;
mod decisions;
mod discord_agents;
mod inbox_rows;
mod render;
mod render_format;

#[cfg(test)]
pub(in crate::ui) use categories::health_warnings_from;
pub(in crate::ui) use categories::{category_detail, category_summary, health_warnings};
pub(in crate::ui) use discord_agents::ordered_channel_ids;
pub(in crate::ui) use inbox_rows::item_preview as inbox_item_preview;

pub(super) type WorkerManagement = (String, ManagementMode);

/// Decisions a snapshot reads out of the append-only log
/// ([`crate::ui::actions::overseer_refresh`]).
///
/// The log keeps growing past this; the UI only ever sees this many of the
/// newest entries, which is why no decision list on screen can report how much
/// history it is leaving out.
pub(in crate::ui) const DECISION_SNAPSHOT_LIMIT: usize = 200;

/// Point-in-time overseer state captured off the UI thread by the background
/// status worker ([`crate::ui::actions::background_refresh`]). The overseer
/// frame and previews render from this snapshot instead of reading
/// config / ledger / pidfile / decision-log from disk on every frame — that
/// per-frame disk I/O was the source of cursor-movement lag.
///
/// `overseer` is reloaded from disk by the background worker rather than read
/// from `app.config`. The in-memory `app.config` only refreshes via the `,`
/// settings editor, so it goes stale whenever dispatch is flipped elsewhere —
/// the `S` panic-stop, the daemon itself, a Discord action, or an external
/// edit. Rendering the Info pane from this disk-backed copy keeps the display
/// in sync with what the daemon will actually do.
#[derive(Default)]
pub(in crate::ui) struct OverseerSnapshot {
    pub(in crate::ui) overseer: OverseerConfig,
    pub(in crate::ui) ledger: Ledger,
    /// Pull requests discovered in a watched repository that Overseer did not
    /// dispatch. Reloaded the same way `ledger` is — see its doc.
    pub(in crate::ui) other_prs: OtherPrs,
    /// Retained per-channel Discord ops agents (dropr:363), read straight off
    /// disk the same way `ledger` and `other_prs` are — the gateway that
    /// writes this file runs inside the daemon, out of reach of this process.
    pub(in crate::ui) discord_channels: DiscordChannels,
    pub(in crate::ui) decisions: Vec<DecisionEntry>,
    pub(in crate::ui) daemon_alive: bool,
    pub(in crate::ui) heartbeat_age: Option<Duration>,
    /// The build the running daemon started from, as recorded in the heartbeat.
    /// `None` for a heartbeat written before the daemon recorded it.
    pub(in crate::ui) daemon_version: Option<String>,
    /// Whether the Overseer's own control tmux session is up, probed off the UI
    /// thread the same way every other session-backed status is. `None` means no
    /// session exists yet (the row shows no badge); `Some(Status::Running)`
    /// means Enter would attach rather than create.
    pub(in crate::ui) control_status: Option<Status>,
}

impl OverseerSnapshot {
    /// Status glyph for the OVERSEER header row.
    ///
    /// A dead daemon is `Dead`. A live daemon only counts as `Running` — the
    /// animated spinner — while dispatch is enabled. Once dispatch is turned
    /// off (the `S` panic-stop kills workers and flips dispatch off but leaves
    /// the daemon process alive) the row must stop animating, so a live daemon
    /// with dispatch off renders as the static `Idle` glyph.
    pub(in crate::ui) fn status(&self) -> Status {
        if !self.daemon_alive {
            Status::Dead
        } else if self.overseer.dispatch_enabled {
            Status::Running
        } else {
            Status::Idle
        }
    }

    /// The warning for a daemon whose build is not this binary's, or `None`
    /// when the two match.
    ///
    /// Gated on the daemon being up, like the CLI status warning: a dead daemon
    /// is already reported as such, and pointing at a restart it is going to
    /// need anyway says nothing the header does not.
    pub(in crate::ui) fn version_drift(&self) -> Option<String> {
        self.daemon_alive
            .then(|| crate::overseer::heartbeat::drift(self.daemon_version.as_deref()))
            .flatten()
    }

    pub(in crate::ui) fn circuit_open(&self) -> bool {
        self.ledger.counters.consecutive_failures >= self.overseer.failure_circuit_threshold
    }

    /// The reason `agent_id`'s worker still needs a human decision, or `None`
    /// once the Overseer has closed the loop on its own. See
    /// [`decisions::blocked_reason`] for why the ledger phase alone cannot
    /// answer this.
    pub(in crate::ui) fn blocked_reason(
        &self,
        locale: crate::locale::Locale,
        agent_id: &str,
    ) -> Option<String> {
        decisions::blocked_reason(locale, &self.ledger, &self.decisions, agent_id)
    }

    /// Where `agent_id`'s pull request stands once its AI session has gone
    /// quiet. See [`decisions::merge_lifecycle`].
    pub(in crate::ui) fn merge_lifecycle(&self, agent_id: &str) -> Option<MergeLifecycle> {
        decisions::merge_lifecycle(&self.ledger, agent_id)
    }

    /// The raw gate reason behind `merge_lifecycle`'s bucket. See
    /// [`decisions::merge_hold_detail`].
    pub(in crate::ui) fn merge_hold_detail(
        &self,
        locale: crate::locale::Locale,
        agent_id: &str,
    ) -> Option<String> {
        decisions::merge_hold_detail(locale, &self.ledger, agent_id)
    }
}

pub(super) fn active_worker_management(app: &App) -> Vec<WorkerManagement> {
    let mut seen_agent_ids = std::collections::HashSet::new();
    let workers = app
        .registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| crate::overseer::is_overseer_child(agent.parent_agent_id.as_deref()))
        .map(|agent| (agent.id.as_str(), agent.title.as_str(), agent.management))
        .collect::<Vec<_>>();

    app.overseer_snapshot
        .ledger
        .entries
        .iter()
        .filter(|entry| !render::terminal(entry.phase))
        .filter(|entry| seen_agent_ids.insert(entry.agent_id.as_str()))
        .filter_map(|entry| {
            workers
                .iter()
                .find(|(id, _, _)| *id == entry.agent_id)
                .map(|(_, title, management)| {
                    let label = if entry.display_id.is_empty() {
                        (*title).to_string()
                    } else {
                        entry.display_id.clone()
                    };
                    (label, *management)
                })
        })
        .collect()
}

pub(super) fn category_preview(app: &App, category: OverseerCategory) -> (String, Text<'static>) {
    (
        format!("OVERSEER / {}", category.display_label(app.locale)),
        category_detail(app, category).into(),
    )
}

pub(in crate::ui) fn heartbeat_is_fresh(path: &Path, poll_secs: u64) -> bool {
    heartbeat_is_fresh_at(path, poll_secs, SystemTime::now())
}

fn heartbeat_is_fresh_at(path: &Path, poll_secs: u64, now: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age <= heartbeat_limit(poll_secs))
}

fn heartbeat_limit(poll_secs: u64) -> Duration {
    Duration::from_secs(poll_secs.saturating_mul(2).max(5))
}

#[cfg(test)]
mod tests;
