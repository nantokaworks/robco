//! Renders the OVERSEER Discord category (dropr:363): one row per retained
//! per-channel ops agent, read from the `DiscordChannels` snapshot the
//! background refresh loads off disk — see `crate::overseer::discord_channels`
//! for why this can only be read from a file rather than the daemon's memory.

use chrono::{DateTime, Utc};
use ratatui::text::Line;

use crate::overseer::discord_channels::{ChannelAgentStatus, DiscordChannels};

use super::render::{flags_line, warning};
use crate::ui::theme::DEFAULT as THEME;

/// Lists the retained per-channel Discord ops agents, newest activity first —
/// the same ordering `append_ledger`'s active-worker rows use for the same
/// reason: the channel an operator is most likely checking on belongs at the
/// top.
pub(super) fn append_discord(lines: &mut Vec<Line<'static>>, channels: &DiscordChannels) {
    if channels.channels.is_empty() {
        lines.push(Line::styled(
            "no retained channel agents yet",
            THEME.muted_style(),
        ));
        return;
    }
    let mut entries: Vec<_> = channels.channels.iter().collect();
    entries.sort_by_key(|(_, agent)| std::cmp::Reverse(agent.last_active_at));
    for (channel_id, agent) in entries {
        let failed = agent.status == ChannelAgentStatus::Failed;
        lines.push(flags_line(&[
            ("channel", channel_id.clone(), false),
            ("status", status_label(agent.status).into(), failed),
            ("turns", agent.turn_count.to_string(), false),
            ("last active", relative_age(agent.last_active_at), false),
        ]));
        if let Some(error) = &agent.last_error {
            lines.push(warning(error));
        }
    }
}

fn status_label(status: ChannelAgentStatus) -> &'static str {
    match status {
        ChannelAgentStatus::Running => "running",
        ChannelAgentStatus::Idle => "idle",
        ChannelAgentStatus::Failed => "failed",
    }
}

/// Coarse "Xm/Xh/Xd ago" rendering — no existing helper in this codebase
/// scales past raw seconds (`overseer.rs`'s heartbeat age deliberately stays
/// at that resolution since it is always fresh), but a channel's
/// `last_active_at` can be days old, where raw seconds stops being readable.
fn relative_age(at: DateTime<Utc>) -> String {
    let elapsed = (Utc::now() - at).num_seconds().max(0);
    if elapsed < 60 {
        "just now".into()
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::discord_channels::ChannelAgent;

    fn agent(status: ChannelAgentStatus, turns: u64, minutes_ago: i64) -> ChannelAgent {
        let now = Utc::now();
        ChannelAgent {
            first_seen_at: now,
            last_active_at: now - chrono::Duration::minutes(minutes_ago),
            turn_count: turns,
            status,
            last_error: None,
            history: Vec::new(),
        }
    }

    #[test]
    fn no_channels_renders_an_explicit_empty_state() {
        let mut lines = Vec::new();
        append_discord(&mut lines, &DiscordChannels::default());
        assert_eq!(lines.len(), 1);
        assert!(text_of(&lines[0]).contains("no retained channel agents"));
    }

    #[test]
    fn channels_are_listed_newest_activity_first() {
        let mut channels = DiscordChannels::default();
        channels
            .channels
            .insert("older".into(), agent(ChannelAgentStatus::Idle, 3, 30));
        channels
            .channels
            .insert("newer".into(), agent(ChannelAgentStatus::Idle, 1, 1));

        let mut lines = Vec::new();
        append_discord(&mut lines, &channels);

        assert_eq!(lines.len(), 2);
        assert!(text_of(&lines[0]).contains("newer"));
        assert!(text_of(&lines[1]).contains("older"));
    }

    #[test]
    fn a_failed_channel_renders_its_error_on_the_next_line() {
        let mut channels = DiscordChannels::default();
        let mut failing = agent(ChannelAgentStatus::Failed, 2, 5);
        failing.last_error = Some("session timed out".into());
        channels.channels.insert("c1".into(), failing);

        let mut lines = Vec::new();
        append_discord(&mut lines, &channels);

        assert_eq!(lines.len(), 2);
        assert!(text_of(&lines[0]).contains("failed"));
        assert!(text_of(&lines[1]).contains("session timed out"));
    }

    fn text_of(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}
