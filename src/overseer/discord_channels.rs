//! One retained record per Discord channel that has ever talked to the ops
//! agent (dropr:363). Retention here means identity plus conversation
//! continuity, not a resident process: each turn is still a spawned,
//! `EphemeralSession`-backed OS thread (`ops_session.rs`), bounded by
//! `chat_concurrency_cap` exactly as before. What survives between turns is
//! this on-disk record — first contact, last activity, turn count, last
//! outcome, and a bounded transcript — read back into the next turn's
//! briefing for continuity.
//!
//! Lives at the `overseer` level rather than inside the private `discord`
//! module because the TUI reads it too: the gateway runs inside the daemon
//! process and the TUI is a separate process with no access to the daemon's
//! memory, so the Overseer info pane's Discord category can only render what
//! this file persists under `discord_ops_dir()`.

use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

/// Turns kept per channel for the conversation-continuity block in the next
/// briefing. Bounded so a long-lived channel's history file, and the prompt
/// built from it, both stay a fixed size rather than growing without limit.
const HISTORY_LIMIT: usize = 6;

/// Character cap applied to a stored message or reply. Generous enough for a
/// real exchange to stay legible in the briefing and the info pane, tight
/// enough that one verbose turn cannot dominate the token budget of the next.
const SNIPPET_LIMIT: usize = 600;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAgentStatus {
    Running,
    Idle,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelTurn {
    pub user_message: String,
    pub reply: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelAgent {
    pub first_seen_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub turn_count: u64,
    pub status: ChannelAgentStatus,
    pub last_error: Option<String>,
    #[serde(default)]
    pub history: Vec<ChannelTurn>,
    /// Human-readable channel name fetched over REST when a turn starts —
    /// `MESSAGE_CREATE` does not carry it. `None` for records written before
    /// this field existed, or when every fetch so far has failed; readers
    /// fall back to the channel id then.
    #[serde(default)]
    pub channel_name: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DiscordChannels {
    pub channels: BTreeMap<String, ChannelAgent>,
}

impl DiscordChannels {
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(raw) => serde_json::from_slice(&raw).map_err(|error| error.to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// A `Running` status is only ever true while its session thread is alive
    /// in this process. Nothing about that thread survives a daemon restart,
    /// so any record loaded as `Running` belongs to a turn that no longer
    /// exists and would otherwise read as permanently stuck in the info pane.
    pub fn reconcile_restart(&mut self, path: &Path) -> Result<(), String> {
        let mut changed = false;
        for entry in self.channels.values_mut() {
            if entry.status == ChannelAgentStatus::Running {
                entry.status = ChannelAgentStatus::Idle;
                changed = true;
            }
        }
        if changed { self.save(path) } else { Ok(()) }
    }

    /// Marks `channel_id` mid-turn, creating its retained record on first
    /// contact. Called before a session spawns so the info pane shows
    /// `Running` for the turn's whole duration, not only after it lands.
    pub fn begin_turn(&mut self, path: &Path, channel_id: &str) -> Result<(), String> {
        let now = Utc::now();
        let entry = self
            .channels
            .entry(channel_id.to_string())
            .or_insert_with(|| ChannelAgent {
                first_seen_at: now,
                last_active_at: now,
                turn_count: 0,
                status: ChannelAgentStatus::Running,
                last_error: None,
                history: Vec::new(),
                channel_name: None,
            });
        entry.last_active_at = now;
        entry.status = ChannelAgentStatus::Running;
        self.save(path)
    }

    /// Records a finished turn. A successful reply is appended to the bounded
    /// history so the next briefing can recall it; a failure updates status
    /// and `last_error` but leaves history untouched, since there is no reply
    /// worth recalling. A no-op for a channel `begin_turn` never saw, so a
    /// stale or out-of-order completion cannot fabricate a record.
    pub fn end_turn(
        &mut self,
        path: &Path,
        channel_id: &str,
        user_message: &str,
        outcome: Result<&str, &str>,
    ) -> Result<(), String> {
        let Some(entry) = self.channels.get_mut(channel_id) else {
            return Ok(());
        };
        entry.last_active_at = Utc::now();
        entry.turn_count = entry.turn_count.saturating_add(1);
        match outcome {
            Ok(reply) => {
                entry.status = ChannelAgentStatus::Idle;
                entry.last_error = None;
                entry.history.push(ChannelTurn {
                    user_message: truncate(user_message),
                    reply: truncate(reply),
                });
                if entry.history.len() > HISTORY_LIMIT {
                    let overflow = entry.history.len() - HISTORY_LIMIT;
                    entry.history.drain(0..overflow);
                }
            }
            Err(error) => {
                entry.status = ChannelAgentStatus::Failed;
                entry.last_error = Some(truncate(error));
            }
        }
        self.save(path)
    }

    /// Stores the channel's human-readable name, refreshed on each turn so a
    /// rename is picked up. A no-op for a channel `begin_turn` never saw, and
    /// a no-op (no disk write) when the name has not changed. Fetch failures
    /// never reach here — the caller keeps whatever name is already stored.
    pub fn set_channel_name(
        &mut self,
        path: &Path,
        channel_id: &str,
        name: &str,
    ) -> Result<(), String> {
        let Some(entry) = self.channels.get_mut(channel_id) else {
            return Ok(());
        };
        if entry.channel_name.as_deref() == Some(name) {
            return Ok(());
        }
        entry.channel_name = Some(name.to_string());
        self.save(path)
    }

    /// The operator-facing label for `channel_id`: `#name` when a name is
    /// stored, the raw id otherwise (records from before names were fetched,
    /// or channels whose every fetch failed).
    pub fn display_label(&self, channel_id: &str) -> String {
        match self
            .channels
            .get(channel_id)
            .and_then(|entry| entry.channel_name.as_deref())
        {
            Some(name) => format!("#{name}"),
            None => channel_id.to_string(),
        }
    }

    /// Recent turns for `channel_id`, oldest first — the conversation history
    /// the next briefing folds in so the agent can refer to what it just said
    /// or did. Empty for a channel with no retained record yet.
    pub fn history(&self, channel_id: &str) -> &[ChannelTurn] {
        self.channels
            .get(channel_id)
            .map(|entry| entry.history.as_slice())
            .unwrap_or(&[])
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let raw = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, path)) {
            let _ = fs::remove_file(temp);
            return Err(error.to_string());
        }
        Ok(())
    }
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= SNIPPET_LIMIT {
        text.to_string()
    } else {
        text.chars().take(SNIPPET_LIMIT).collect::<String>() + "…"
    }
}

#[cfg(test)]
#[path = "discord_channels_tests.rs"]
mod tests;
