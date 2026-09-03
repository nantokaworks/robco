use ratatui::text::Line;

use crate::{
    locale::{Locale, fmt, t},
    model::OverseerCategory,
    overseer::discord_channels::{ChannelAgentStatus, DiscordChannels},
};

use super::{App, discord_agents};

pub(in crate::ui) fn category_detail(app: &App, category: OverseerCategory) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match category {
        OverseerCategory::Discord => lines.extend(discord_agents::detail_lines(app)),
    }
    while lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

pub(in crate::ui) fn category_summary(app: &App, category: OverseerCategory) -> (String, bool) {
    match category {
        OverseerCategory::Discord => {
            discord_summary_from(app.locale, &app.overseer_snapshot.discord_channels)
        }
    }
}

pub(super) fn discord_summary_from(locale: Locale, channels: &DiscordChannels) -> (String, bool) {
    let failed = channels
        .channels
        .values()
        .filter(|agent| agent.status == ChannelAgentStatus::Failed)
        .count();
    let total = channels.channels.len();
    if total == 0 {
        return (t(locale, "no retained channels").to_string(), false);
    }
    if failed == 0 {
        (fmt(locale, "{} retained", &[&total.to_string()]), false)
    } else {
        (
            fmt(
                locale,
                "{} retained, {} failed",
                &[&total.to_string(), &failed.to_string()],
            ),
            true,
        )
    }
}

pub(in crate::ui) fn health_warnings(app: &App) -> Vec<&'static str> {
    let snapshot = &app.overseer_snapshot;
    health_warnings_from(snapshot.daemon_alive, snapshot.version_drift().is_some())
}

pub(in crate::ui) fn health_warnings_from(alive: bool, version_drift: bool) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    if !alive {
        warnings.push("STALE/OFFLINE");
    }
    // The header carries only the terse label; `robco status --debug` names
    // both builds in full.
    if version_drift {
        warnings.push(crate::overseer::heartbeat::DRIFT_LABEL);
    }
    warnings
}
