use ratatui::text::Line;

use crate::{
    model::OverseerCategory,
    overseer::ledger::{Ledger, LedgerPhase},
};

use super::{
    App, active_worker_management,
    decisions::{DETAIL_LIMIT, DecisionList, append_decisions},
    render::{append_health, append_inbox, append_ledger, append_worker_management},
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
        ),
        OverseerCategory::Ledger => {
            let management = active_worker_management(app);
            append_ledger(
                &mut lines,
                config,
                &snapshot.ledger,
                &snapshot.decisions,
                &management,
            );
            while lines.last().is_some_and(|line| line.spans.is_empty()) {
                lines.pop();
            }
            append_worker_management(&mut lines, &management);
        }
        OverseerCategory::Inbox => append_inbox(&mut lines, app),
        OverseerCategory::Decisions => {
            append_decisions(&mut lines, &snapshot.decisions, DecisionList::Detail);
        }
    }
    while lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

pub(in crate::ui) fn category_summary(app: &App, category: OverseerCategory) -> (String, bool) {
    let snapshot = &app.overseer_snapshot;
    match category {
        OverseerCategory::Health => {
            health_summary_from(&snapshot.overseer, &snapshot.ledger, snapshot.daemon_alive)
        }
        OverseerCategory::Ledger => ledger_summary_from(&snapshot.ledger),
        OverseerCategory::Inbox => {
            let actionable = app
                .overseer_inbox
                .iter()
                .filter(|item| item.target_session.is_some())
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
) -> (String, bool) {
    let warnings = health_warnings_from(config, ledger, alive);
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
    health_warnings_from(&snapshot.overseer, &snapshot.ledger, snapshot.daemon_alive)
}

pub(in crate::ui) fn health_warnings_from(
    config: &crate::overseer::config::OverseerConfig,
    ledger: &Ledger,
    alive: bool,
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
    warnings
}
