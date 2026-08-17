//! Renders the OVERSEER Discord category (dropr:363): one selectable row per
//! retained per-channel ops agent, newest activity first — reusing the
//! marker-and-status row shape the Overseer control AI row and the Inbox
//! item rows already use (dropr:371), so Enter can attach the channel's live
//! tmux session the same way Enter attaches any other AI session.

use ratatui::text::{Line, Span, Text};

use crate::{
    locale::{Locale, fmt, t},
    model::{Selection, Status},
    overseer::discord_channels::{ChannelAgent, ChannelAgentStatus, DiscordChannels},
};

use super::super::App;
use crate::ui::theme::DEFAULT as THEME;

/// The separator between a channel's label and its status column, matching
/// the gap `overseer_frame`'s category and control-AI rows use.
const GAP: &str = "  ";

/// Channel ids in the order their rows render, newest activity first — the
/// same order [`Selection::DiscordChannel`] indexes into. A pure function of
/// `channels`, so every caller that needs the order (row building, `visible()`
/// row counting, attach, live-session mirroring) derives it independently
/// from the same snapshot instead of threading a materialized list around.
pub(in crate::ui) fn ordered_channel_ids(channels: &DiscordChannels) -> Vec<String> {
    let mut entries: Vec<_> = channels.channels.iter().collect();
    entries.sort_by_key(|(_, agent)| std::cmp::Reverse(agent.last_active_at));
    entries.into_iter().map(|(id, _)| id.clone()).collect()
}

/// The Discord category's detail: one row per retained channel, and nothing
/// else. Item `n` is detail row `n`, the same contract `inbox_rows::detail_lines`
/// keeps for its own item rows.
pub(in crate::ui) fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let channels = &app.overseer_snapshot.discord_channels;
    let ids = ordered_channel_ids(channels);
    if ids.is_empty() {
        return vec![Line::styled(
            t(app.locale, "no retained channel agents yet"),
            THEME.muted_style(),
        )];
    }
    let selected = match app.selected_item() {
        Some(Selection::DiscordChannel(index)) => Some(index),
        _ => None,
    };
    let mut lines = Vec::new();
    for (index, channel_id) in ids.iter().enumerate() {
        let Some(agent) = channels.channels.get(channel_id) else {
            continue;
        };
        lines.push(channel_line(
            &channels.display_label(channel_id),
            agent,
            selected == Some(index),
            app.locale,
        ));
        if let Some(error) = &agent.last_error {
            lines.push(Line::styled(
                format!("    ⚠ {error}"),
                THEME.failure_style(),
            ));
        }
    }
    lines
}

fn channel_line(
    label: &str,
    agent: &ChannelAgent,
    selected: bool,
    locale: Locale,
) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    let row_style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    let status = match agent.status {
        ChannelAgentStatus::Running => Status::Running,
        ChannelAgentStatus::Idle => Status::Idle,
        ChannelAgentStatus::Failed => Status::Dead,
    };
    let status_style = if selected {
        THEME.selection_style()
    } else {
        THEME.status_style(status)
    };
    Line::from(vec![
        Span::styled(format!("{marker}   {label}{GAP}"), row_style),
        Span::styled(
            format!(
                "{} · {}t · {}",
                status.badge(),
                agent.turn_count,
                relative_age(locale, agent.last_active_at)
            ),
            status_style,
        ),
    ])
}

/// The Discord channel row's own preview: its retained conversation turns,
/// oldest first, the same thread the phone shows (dropr:451). `preview.rs`'s
/// `Selection::DiscordChannel` arm reaches this only once it has already
/// checked there is no live tmux session to mirror for the channel — a turn
/// in progress still shows the live session, exactly as before.
pub(in crate::ui) fn channel_preview(app: &App, index: usize) -> (String, Text<'static>) {
    let channels = &app.overseer_snapshot.discord_channels;
    let ids = ordered_channel_ids(channels);
    let Some(channel_id) = ids.get(index) else {
        return (
            "OVERSEER / Discord".to_string(),
            vec![Line::styled(
                t(app.locale, "channel is no longer listed"),
                THEME.muted_style(),
            )]
            .into(),
        );
    };
    let title = format!("Discord / {}", channels.display_label(channel_id));
    let turns = channels.history(channel_id);
    if turns.is_empty() {
        return (
            title,
            vec![Line::styled(
                t(app.locale, "no turns yet"),
                THEME.muted_style(),
            )]
            .into(),
        );
    }
    let mut lines = Vec::new();
    for (index, turn) in turns.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        lines.push(turn_label("User"));
        lines.extend(turn_body(&turn.user_message));
        lines.push(turn_label("Agent"));
        lines.extend(turn_body(&turn.reply));
    }
    (title, lines.into())
}

/// `User:` / `Agent:` are UI item labels, not prose — English regardless of
/// `language`, the same rule `inbox_rows::field` follows for its own labels.
fn turn_label(label: &'static str) -> Line<'static> {
    Line::styled(format!("{label}:"), THEME.muted_style())
}

fn turn_body(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|line| Line::styled(line.to_string(), THEME.accent_style()))
        .collect()
}

/// Coarse "Xm/Xh/Xd ago" rendering — no existing helper in this codebase
/// scales past raw seconds (`overseer.rs`'s heartbeat age deliberately stays
/// at that resolution since it is always fresh), but a channel's
/// `last_active_at` can be days old, where raw seconds stops being readable.
fn relative_age(locale: Locale, at: chrono::DateTime<chrono::Utc>) -> String {
    let elapsed = (chrono::Utc::now() - at).num_seconds().max(0);
    if elapsed < 60 {
        t(locale, "just now").to_string()
    } else if elapsed < 3600 {
        fmt(locale, "{}m ago", &[&(elapsed / 60).to_string()])
    } else if elapsed < 86_400 {
        fmt(locale, "{}h ago", &[&(elapsed / 3600).to_string()])
    } else {
        fmt(locale, "{}d ago", &[&(elapsed / 86_400).to_string()])
    }
}

#[cfg(test)]
#[path = "discord_agents_tests.rs"]
mod tests;
