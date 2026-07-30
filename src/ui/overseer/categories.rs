use ratatui::text::Line;

use crate::{
    model::OverseerCategory,
    overseer::{
        discord_channels::{ChannelAgentStatus, DiscordChannels},
        ledger::{Ledger, LedgerPhase},
    },
};

use super::{
    App, active_worker_management,
    decisions::{DETAIL_LIMIT, append_decisions},
    discord_agents, inbox_rows,
    render::{append_health, append_ledger, append_worker_management},
};

pub(in crate::ui) fn category_detail(app: &App, category: OverseerCategory) -> Vec<Line<'static>> {
    let snapshot = &app.overseer_snapshot;
    let config = &snapshot.overseer;
    let mut lines = Vec::new();
    match category {
        OverseerCategory::Health => append_health(
            &mut lines,
            config,
            &snapshot.ledger,
            snapshot.daemon_alive,
            snapshot.heartbeat_age,
            snapshot.daemon_version.as_deref(),
            snapshot.version_drift().as_deref(),
            app.locale,
        ),
        OverseerCategory::Ledger => {
            let management = active_worker_management(app);
            append_ledger(
                &mut lines,
                config,
                &snapshot.ledger,
                &snapshot.decisions,
                &management,
                &app.registry,
            );
            while lines.last().is_some_and(|line| line.spans.is_empty()) {
                lines.pop();
            }
            append_worker_management(&mut lines, &management);
        }
        OverseerCategory::Inbox => lines.extend(inbox_rows::detail_lines(app)),
        OverseerCategory::Decisions => {
            append_decisions(&mut lines, &snapshot.decisions);
        }
        OverseerCategory::Discord => lines.extend(discord_agents::detail_lines(app)),
    }
    while lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

pub(in crate::ui) fn category_summary(app: &App, category: OverseerCategory) -> (String, bool) {
    let snapshot = &app.overseer_snapshot;
    match category {
        OverseerCategory::Health => health_summary_from(
            &snapshot.overseer,
            &snapshot.ledger,
            snapshot.daemon_alive,
            snapshot.version_drift().is_some(),
        ),
        OverseerCategory::Ledger => ledger_summary_from(&snapshot.ledger),
        OverseerCategory::Inbox => {
            // Actionable is derived per row from its resolved remedy — not
            // whether it has a live session — so a `Merge`, `Reset`, `Retry`,
            // or `Review` row counts even with no worker to answer into.
            // Only `Watch` rows (nothing to do yet) are excluded.
            let actionable = app
                .overseer_inbox
                .iter()
                .filter(|item| item.actionable())
                .count();
            (
                format!("{actionable}/{} actionable", app.overseer_inbox.len()),
                false,
            )
        }
        // Count what expanding the category will actually list, so the row and
        // the list below it cannot disagree about how much is on offer.
        OverseerCategory::Decisions => (
            format!("{} recent", snapshot.decisions.len().min(DETAIL_LIMIT)),
            false,
        ),
        OverseerCategory::Discord => discord_summary_from(&snapshot.discord_channels),
    }
}

pub(super) fn discord_summary_from(channels: &DiscordChannels) -> (String, bool) {
    let failed = channels
        .channels
        .values()
        .filter(|agent| agent.status == ChannelAgentStatus::Failed)
        .count();
    let total = channels.channels.len();
    if total == 0 {
        return ("no retained channels".into(), false);
    }
    if failed == 0 {
        (format!("{total} retained"), false)
    } else {
        (format!("{total} retained, {failed} failed"), true)
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

pub(super) fn health_summary_from(
    config: &crate::overseer::config::OverseerConfig,
    ledger: &Ledger,
    alive: bool,
    version_drift: bool,
) -> (String, bool) {
    let warnings = health_warnings_from(config, ledger, alive, version_drift);
    if warnings.is_empty() {
        ("daemon online · circuit closed".into(), false)
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
    health_warnings_from(
        &snapshot.overseer,
        &snapshot.ledger,
        snapshot.daemon_alive,
        snapshot.version_drift().is_some(),
    )
}

pub(in crate::ui) fn health_warnings_from(
    config: &crate::overseer::config::OverseerConfig,
    ledger: &Ledger,
    alive: bool,
    version_drift: bool,
) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    if !alive {
        warnings.push("STALE/OFFLINE");
    }
    if ledger.counters.consecutive_failures >= config.failure_circuit_threshold {
        warnings.push("circuit OPEN");
    }
    if config.dispatch_enabled && !alive {
        warnings.push("dispatch/no daemon");
    }
    // The terse label only; the sentence naming both builds belongs to the
    // Health detail, which has the width to carry it.
    if version_drift {
        warnings.push(crate::overseer::heartbeat::DRIFT_LABEL);
    }
    warnings
}
