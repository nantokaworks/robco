use ratatui::text::Line;

use crate::{
    locale::{Locale, fmt, t},
    model::OverseerCategory,
    overseer::{
        discord_channels::{ChannelAgentStatus, DiscordChannels},
        ledger::{Ledger, LedgerPhase},
    },
};

use super::{
    App,
    decisions::{DETAIL_LIMIT, append_decisions},
    discord_agents, inbox_rows,
    render::{append_health, append_ledger},
};

pub(in crate::ui) fn category_detail(app: &App, category: OverseerCategory) -> Vec<Line<'static>> {
    let snapshot = &app.overseer_snapshot;
    let config = &snapshot.overseer;
    let mut lines = Vec::new();
    match category {
        OverseerCategory::Health => append_health(
            &mut lines,
            config,
            snapshot.daemon_alive,
            snapshot.heartbeat_age,
            snapshot.daemon_version.as_deref(),
            snapshot.version_drift().as_deref(),
            app.locale,
        ),
        OverseerCategory::Ledger => {
            append_ledger(
                &mut lines,
                &snapshot.ledger,
                &snapshot.decisions,
                &app.registry,
            );
        }
        OverseerCategory::Inbox => lines.extend(inbox_rows::detail_lines(app)),
        OverseerCategory::Decisions => {
            append_decisions(&mut lines, &snapshot.decisions, app.locale);
        }
        OverseerCategory::Discord => lines.extend(discord_agents::detail_lines(app)),
    }
    while lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

/// How many Inbox items resolve to something other than `WATCH`. Actionable is
/// derived per row from its resolved remedy — not whether it has a live
/// session — so a `Merge`, `Reset`, `Retry`, or `Review` row counts even with
/// no worker to answer into. Shared by the category summary text and the row
/// indicator (dropr:469) so they can never disagree.
pub(in crate::ui) fn inbox_actionable_count(app: &App) -> usize {
    app.overseer_inbox
        .iter()
        .filter(|item| item.actionable())
        .count()
}

pub(in crate::ui) fn category_summary(app: &App, category: OverseerCategory) -> (String, bool) {
    let snapshot = &app.overseer_snapshot;
    match category {
        OverseerCategory::Health => {
            health_summary_from(snapshot.daemon_alive, snapshot.version_drift().is_some())
        }
        OverseerCategory::Ledger => ledger_summary_from(&snapshot.ledger),
        OverseerCategory::Inbox => (
            fmt(
                app.locale,
                "{}/{} actionable",
                &[
                    &inbox_actionable_count(app).to_string(),
                    &app.overseer_inbox.len().to_string(),
                ],
            ),
            false,
        ),
        // Count what expanding the category will actually list, so the row and
        // the list below it cannot disagree about how much is on offer.
        OverseerCategory::Decisions => (
            fmt(
                app.locale,
                "{} recent",
                &[&snapshot.decisions.len().min(DETAIL_LIMIT).to_string()],
            ),
            false,
        ),
        OverseerCategory::Discord => discord_summary_from(app.locale, &snapshot.discord_channels),
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

pub(super) fn ledger_summary_from(ledger: &Ledger) -> (String, bool) {
    let active = ledger
        .entries
        .iter()
        .filter(|entry| {
            !matches!(
                entry.phase,
                LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
            )
        })
        .count();
    (format!("{active} active"), false)
}

pub(super) fn health_summary_from(alive: bool, version_drift: bool) -> (String, bool) {
    let warnings = health_warnings_from(alive, version_drift);
    if warnings.is_empty() {
        ("daemon online".into(), false)
    } else {
        (
            warnings
                .into_iter()
                .map(|warning| format!("[{warning}]"))
                .collect::<Vec<_>>()
                .join(" "),
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
    // The terse label only; the sentence naming both builds belongs to the
    // Health detail, which has the width to carry it.
    if version_drift {
        warnings.push(crate::overseer::heartbeat::DRIFT_LABEL);
    }
    warnings
}
