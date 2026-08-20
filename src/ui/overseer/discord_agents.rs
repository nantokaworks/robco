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
use crate::ui::tree::indicator::{self, IndicatorState, select, select_supplementary};

/// The separator between a channel's label and its status column, matching
/// the gap `overseer_frame`'s category and control-AI rows use.
const GAP: &str = "  ";

/// The primary indicator's own column width for a channel row — one cell,
/// the same width `control_ai_line` and `category_indicator` reserve for
/// their own single-glyph indicators in this same narrow OVERSEER sidebar.
/// A PROJECTS agent row reserves up to two cells (`label::labeled_row`)
/// because it shares that budget with a wider pane; the glyph and its style
/// are what the shared vocabulary owns and keeps identical, not the padding.
const INDICATOR_WIDTH: usize = 1;

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
            app.started.elapsed(),
        ));
        if let Some(error) = &agent.last_error {
            // No marker of its own, so it pads out to `super::ROW_LEFT_EDGE`
            // instead — the same column the channel label above it starts
            // at, not the column a copied row prefix would land on.
            lines.push(Line::styled(
                format!("{}⚠ {error}", " ".repeat(super::ROW_LEFT_EDGE)),
                THEME.failure_style(),
            ));
        }
    }
    lines
}

/// Built the way `overseer_frame::control_ai_line` builds the control AI
/// row: an `IndicatorState` mapped from the channel's own status, handed to
/// the shared `indicator::primary_span` / `supplementary_spans` so a
/// channel row carries the same glyph vocabulary, precedence, running
/// animation, and selection inversion as every PROJECTS row. Turn count and
/// last-activity age stay — they are this row's own content, not part of
/// the shared vocabulary — but they render after the indicator's own spans
/// rather than before, the position `repo_row`/`tree`'s agent row place
/// their own supplementary content.
///
/// The marker prefix itself does NOT copy `control_ai_line`: that row sits
/// at the top of the frame and pads out to the category label's own column,
/// while this one is a detail row that `overseer_frame::indent_detail`
/// already nests under its category — so it only spends `super::
/// ROW_LEFT_EDGE` columns on marker plus gap, the same as an Inbox item row,
/// not the wider padding a top-level row needs.
fn channel_line(
    label: &str,
    agent: &ChannelAgent,
    selected: bool,
    locale: Locale,
    elapsed: std::time::Duration,
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
    let indicator_state = IndicatorState::with_status(Some(status));
    let primary = select(indicator_state);
    let mut spans = vec![
        // Marker + one gap space = `super::ROW_LEFT_EDGE` columns.
        Span::styled(format!("{marker} {label}{GAP}"), row_style),
        indicator::primary_span(primary, selected, elapsed, INDICATOR_WIDTH),
    ];
    let mut right = indicator::supplementary_spans(
        primary,
        select_supplementary(indicator_state),
        selected,
        GAP,
    );
    let value_style = if selected {
        THEME.selection_style()
    } else {
        THEME.status_style(status)
    };
    let prefix = if right.is_empty() { GAP } else { " " };
    right.push(Span::styled(
        format!(
            "{prefix}{}t · {}",
            agent.turn_count,
            relative_age(locale, agent.last_active_at)
        ),
        value_style,
    ));
    spans.extend(right);
    Line::from(spans)
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
