use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

#[cfg(test)]
use chrono::Utc;
use ratatui::text::Text;

use crate::{
    model::{MergeLifecycle, OverseerCategory, Status},
    overseer::{
        config::OverseerConfig, discord_channels::DiscordChannels, ledger::Ledger,
        logging::DecisionEntry, other_prs::OtherPrs,
    },
};

use super::App;

mod categories;
mod decisions;
mod discord_agents;
mod hold_reason;
mod inbox_rows;
mod render;
mod render_format;

pub(in crate::ui) use categories::{
    category_detail, category_summary, health_warnings, inbox_actionable_count,
};
pub(in crate::ui) use discord_agents::{
    channel_preview as discord_channel_preview,
    channel_preview_from as discord_channel_preview_from, ordered_channel_ids,
};
pub(in crate::ui) use inbox_rows::item_preview as inbox_item_preview;

/// Decisions a snapshot reads out of the append-only log
/// ([`crate::ui::actions::overseer_refresh`]).
///
/// The log keeps growing past this; the UI only ever sees this many of the
/// newest entries, which is why no decision list on screen can report how much
/// history it is leaving out.
pub(in crate::ui) const DECISION_SNAPSHOT_LIMIT: usize = 200;

/// Columns a detail row (an Inbox item, a Discord channel) spends on its own
/// marker before its content starts: one marker column, one gap column.
/// `overseer_frame::DETAIL_INDENT` nests every detail row under its category
/// label; this is the row's own share of the line, on top of that indent.
/// Every detail-row builder in this module must agree on it, or two rows
/// nested the same way visibly snap to different columns — the way Discord's
/// row drifted from Inbox's (dropr:497). A continuation line with no marker
/// of its own (a Discord channel's error line) pads out to this width too,
/// so it lines up under the row's content instead of under a blank marker
/// cell. A test in `tests.rs` pins this so a third builder cannot drift
/// again.
const ROW_LEFT_EDGE: usize = 2;

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
    /// Status glyph for the OVERSEER header row: `Running` while the daemon
    /// is alive, `Dead` otherwise. The daemon always has work to do now —
    /// merge polling, Discord/MCP commands, worker monitoring — so there is
    /// no separate "on but idle" state to distinguish (dropr:476).
    pub(in crate::ui) fn status(&self) -> Status {
        if self.daemon_alive {
            Status::Running
        } else {
            Status::Dead
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

    /// Whether `agent_id`'s pull request has been observed merged — through
    /// robco's own merge flow, or externally (`gh pr merge`, github.com).
    /// See [`crate::overseer::ledger::Ledger::observed_merged`]. Callers
    /// gate this on `Status::Dead` themselves, the same way
    /// [`Self::merge_lifecycle`] gates its own read on the session having
    /// gone quiet (dropr:563).
    pub(in crate::ui) fn observed_merged(&self, agent_id: &str) -> bool {
        self.ledger.observed_merged(agent_id)
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

    /// Why `agent_id`'s ledger entry stopped in a terminal phase the operator
    /// did not ask for. See [`decisions::terminal_reason`].
    pub(in crate::ui) fn terminal_reason(&self, agent_id: &str) -> Option<String> {
        decisions::terminal_reason(&self.ledger, &self.decisions, agent_id)
    }

    /// Why `agent_id`'s pull request is being held, worth a line on its own
    /// row, or `None` when the hold is quiet. See
    /// [`hold_reason::held_reason`].
    pub(in crate::ui) fn held_reason(&self, agent_id: &str) -> Option<String> {
        hold_reason::held_reason(&self.ledger, agent_id)
    }

    /// Whether `agent_id`'s worker has reported itself finished while its
    /// entry is still live. See [`decisions::worker_finished`].
    pub(in crate::ui) fn worker_finished(&self, agent_id: &str) -> bool {
        decisions::worker_finished(&self.ledger, agent_id)
    }
}

pub(super) fn category_preview(app: &App, category: OverseerCategory) -> (String, Text<'static>) {
    (
        format!("OVERSEER / {}", category.label()),
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
