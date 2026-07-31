//! Renders the OVERSEER Discord category (dropr:363): one selectable row per
//! retained per-channel ops agent, newest activity first — reusing the
//! marker-and-status row shape the Overseer control AI row and the Inbox
//! item rows already use (dropr:371), so Enter can attach the channel's live
//! tmux session the same way Enter attaches any other AI session.

use ratatui::text::{Line, Span};

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
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::registry::Registry;
    use chrono::Utc;

    fn agent(status: ChannelAgentStatus, turns: u64, minutes_ago: i64) -> ChannelAgent {
        let now = Utc::now();
        ChannelAgent {
            first_seen_at: now,
            last_active_at: now - chrono::Duration::minutes(minutes_ago),
            turn_count: turns,
            status,
            last_error: None,
            history: Vec::new(),
            channel_name: None,
        }
    }

    fn app_with_channels(channels: DiscordChannels) -> App {
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
        app.overseer_visible = true;
        app.overseer_snapshot.discord_channels = channels;
        app
    }

    #[test]
    fn no_channels_renders_an_explicit_empty_state() {
        let app = app_with_channels(DiscordChannels::default());
        let lines = detail_lines(&app);
        assert_eq!(lines.len(), 1);
        assert!(text_of(&lines[0]).contains("no retained channel agents"));
    }

    #[test]
    fn channels_are_ordered_newest_activity_first() {
        let mut channels = DiscordChannels::default();
        channels
            .channels
            .insert("older".into(), agent(ChannelAgentStatus::Idle, 3, 30));
        channels
            .channels
            .insert("newer".into(), agent(ChannelAgentStatus::Idle, 1, 1));

        assert_eq!(
            ordered_channel_ids(&channels),
            vec!["newer".to_string(), "older".to_string()]
        );
    }

    #[test]
    fn the_selected_channel_row_carries_the_marker() {
        let mut channels = DiscordChannels::default();
        channels
            .channels
            .insert("c1".into(), agent(ChannelAgentStatus::Running, 1, 0));
        let mut app = app_with_channels(channels);
        app.selected = 0;
        app.set_overseer_category_expanded(crate::model::OverseerCategory::Discord, true);
        app.selected = app
            .visible()
            .iter()
            .position(|row| matches!(row, Selection::DiscordChannel(0)))
            .expect("no discord channel row");

        let lines = detail_lines(&app);
        assert!(text_of(&lines[0]).starts_with('>'));
    }

    #[test]
    fn a_named_channel_renders_its_name_and_an_unnamed_one_falls_back_to_the_id() {
        let mut channels = DiscordChannels::default();
        let mut named = agent(ChannelAgentStatus::Idle, 1, 1);
        named.channel_name = Some("general".into());
        channels.channels.insert("111222333".into(), named);
        channels
            .channels
            .insert("999888777".into(), agent(ChannelAgentStatus::Idle, 1, 30));

        let app = app_with_channels(channels);
        let lines = detail_lines(&app);
        assert!(text_of(&lines[0]).contains("#general"));
        assert!(!text_of(&lines[0]).contains("111222333"));
        assert!(text_of(&lines[1]).contains("999888777"));
    }

    #[test]
    fn a_failed_channel_renders_its_error_on_the_next_line() {
        let mut channels = DiscordChannels::default();
        let mut failing = agent(ChannelAgentStatus::Failed, 2, 5);
        failing.last_error = Some("session timed out".into());
        channels.channels.insert("c1".into(), failing);

        let app = app_with_channels(channels);
        let lines = detail_lines(&app);
        assert_eq!(lines.len(), 2);
        assert!(text_of(&lines[0]).contains("dead"));
        assert!(text_of(&lines[1]).contains("session timed out"));
    }

    fn text_of(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}
