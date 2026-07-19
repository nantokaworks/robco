use ratatui::text::Line;

use crate::{
    config::Config,
    model::OverseerCategory,
    overseer::{heartbeat_path, ledger::Ledger},
};

use super::{
    App, heartbeat_is_fresh,
    render::{append_decisions, append_health, append_inbox, append_ledger, detail, warning},
};

pub(in crate::ui) fn category_detail(app: &App, category: OverseerCategory) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match category {
        OverseerCategory::Health => health_detail(&mut lines),
        OverseerCategory::Ledger => ledger_detail(&mut lines),
        OverseerCategory::Inbox => append_inbox(&mut lines, app),
        OverseerCategory::Decisions => append_decisions(&mut lines),
    }
    while lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

pub(in crate::ui) fn category_summary(app: &App, category: OverseerCategory) -> (String, bool) {
    match category {
        OverseerCategory::Health => health_summary(),
        OverseerCategory::Ledger => ledger_summary(),
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
        OverseerCategory::Decisions => match crate::overseer::logging::tail(3) {
            Ok(entries) => (format!("{} recent", entries.len()), false),
            Err(_) => ("unavailable".into(), true),
        },
    }
}

fn health_detail(lines: &mut Vec<Line<'static>>) {
    match load_health() {
        Ok((config, ledger, heartbeat)) => append_health(lines, &config, &ledger, &heartbeat),
        Err((label, error)) => {
            lines.push(warning("Overseer health could not be read."));
            lines.push(detail(label, error));
        }
    }
}

fn ledger_detail(lines: &mut Vec<Line<'static>>) {
    match (Config::load().map(|config| config.overseer), Ledger::load()) {
        (Ok(config), Ok(ledger)) => append_ledger(lines, &config, &ledger),
        (config, ledger) => {
            lines.push(warning("Overseer ledger could not be read."));
            if let Err(error) = config {
                lines.push(detail("config", error));
            }
            if let Err(error) = ledger {
                lines.push(detail("ledger", error));
            }
        }
    }
}

fn health_summary() -> (String, bool) {
    let Ok((config, ledger, heartbeat)) = load_health() else {
        return ("[STALE/OFFLINE]".into(), true);
    };
    let alive = crate::overseer::daemon_pid_alive()
        && heartbeat_is_fresh(&heartbeat, config.poll_interval_secs);
    health_summary_from(&config, &ledger, alive)
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

pub(in crate::ui) fn health_warnings(_app: &App) -> Vec<&'static str> {
    let Ok((config, ledger, heartbeat)) = load_health() else {
        return vec!["STALE/OFFLINE"];
    };
    let alive = crate::overseer::daemon_pid_alive()
        && heartbeat_is_fresh(&heartbeat, config.poll_interval_secs);
    health_warnings_from(&config, &ledger, alive)
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

fn ledger_summary() -> (String, bool) {
    match Ledger::load() {
        Ok(ledger) => {
            let active = ledger
                .entries
                .iter()
                .filter(|entry| {
                    !matches!(
                        entry.phase,
                        crate::overseer::ledger::LedgerPhase::Merged
                            | crate::overseer::ledger::LedgerPhase::Failed
                            | crate::overseer::ledger::LedgerPhase::Escalated
                    )
                })
                .count();
            (format!("{active} active"), false)
        }
        Err(_) => ("unavailable".into(), true),
    }
}

fn load_health() -> Result<
    (
        crate::overseer::config::OverseerConfig,
        Ledger,
        std::path::PathBuf,
    ),
    (&'static str, String),
> {
    let config = Config::load()
        .map(|config| config.overseer)
        .map_err(|error| ("config", error.to_string()))?;
    let ledger = Ledger::load().map_err(|error| ("ledger", error.to_string()))?;
    let heartbeat = heartbeat_path().map_err(|error| ("heartbeat", error.to_string()))?;
    Ok((config, ledger, heartbeat))
}
