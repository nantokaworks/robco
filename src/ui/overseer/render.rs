use std::{collections::BTreeMap, time::Duration};

use chrono::{NaiveDate, Utc};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::model::ManagementMode;
use crate::overseer::{
    config::{OverseerConfig, ProtectionMode},
    ledger::{Ledger, LedgerPhase},
    logging::DecisionEntry,
};
use crate::registry::Registry;

use super::WorkerManagement;
use crate::ui::theme::DEFAULT as THEME;

pub(super) fn append_health(
    lines: &mut Vec<Line<'static>>,
    config: &OverseerConfig,
    ledger: &Ledger,
    alive: bool,
    heartbeat_age: Option<Duration>,
    daemon_version: Option<&str>,
    version_drift: Option<&str>,
) {
    lines.push(flags_line(&[
        (
            "daemon",
            if alive { "alive" } else { "STALE / OFFLINE" }.into(),
            !alive,
        ),
        (
            "hb",
            heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s ago", age.as_secs())),
            !alive,
        ),
        // Reads beside the liveness it qualifies: an alive daemon still runs
        // whatever build it started from, and this is the only place that says
        // which one that is.
        (
            "version",
            daemon_version.unwrap_or("unknown").to_string(),
            version_drift.is_some(),
        ),
    ]));
    let dispatch_without_daemon = config.dispatch_enabled && !alive;
    let circuit_open = ledger.counters.consecutive_failures >= config.failure_circuit_threshold;
    lines.push(flags_line(&[
        (
            "dispatch",
            on_off(config.dispatch_enabled).into(),
            dispatch_without_daemon,
        ),
        ("auto-merge", on_off(config.auto_merge).into(), false),
        (
            "protection",
            config.protection_mode.label().into(),
            config.auto_merge && config.protection_mode != ProtectionMode::Required,
        ),
        (
            "autonomy",
            config.autonomy_level.label().into(),
            config.auto_merge && config.autonomy_level.envelope_warning().is_some(),
        ),
        (
            "merge-recovery",
            merge_recovery_state(config),
            // Recovery without auto-merge is inert: nothing ever fails a merge
            // to hand back, so the setting reads as armed while doing nothing.
            config.merge_recovery_enabled && !config.auto_merge,
        ),
        (
            "circuit",
            if circuit_open { "OPEN" } else { "closed" }.into(),
            circuit_open,
        ),
    ]));
    // Ahead of the other warnings for the same reason the CLI puts it first:
    // every flag above can read healthy while the daemon carries none of what
    // has been merged since it started.
    if let Some(drift) = version_drift {
        lines.push(warning(drift));
    }
    if dispatch_without_daemon {
        lines.push(warning(crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT));
    }
    if !config.dispatch_enabled && alive && !circuit_open {
        lines.push(warning(crate::overseer::DISPATCH_STOPPED_HINT));
    }
    if circuit_open {
        lines.push(warning(crate::overseer::CIRCUIT_OPEN_HINT));
    }
    // Gated on auto-merge, like the red `autonomy` flag above: the envelope only
    // decides anything while the merge pass runs, so warning about it otherwise
    // would name a gate that is not currently in the path.
    if config.auto_merge
        && let Some(hint) = config.autonomy_level.envelope_warning()
    {
        lines.push(warning(hint));
    }
    lines.push(Line::default());
}

pub(super) fn append_ledger(
    lines: &mut Vec<Line<'static>>,
    config: &OverseerConfig,
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    management: &[WorkerManagement],
    registry: &Registry,
) {
    let active = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .collect::<Vec<_>>();
    let mut repos = BTreeMap::new();
    let mut phases = BTreeMap::new();
    for entry in &active {
        *repos
            .entry(registry.repo_label(&entry.repo))
            .or_insert(0usize) += 1;
    }
    for entry in &active {
        *phases.entry(entry.phase.label()).or_insert(0usize) += 1;
    }
    lines.push(flags_line(&[
        (
            "dispatches",
            format!(
                "{} / {}",
                dispatches_on(ledger, Utc::now().date_naive()),
                crate::overseer::dispatch::format_dispatch_limit(config.daily_dispatch_limit)
            ),
            false,
        ),
        (
            "workers",
            format!(
                "{} / {} global, {}/repo",
                active.len(),
                config.max_workers,
                config.per_repo_limit
            ),
            false,
        ),
    ]));
    if !repos.is_empty() {
        lines.push(pair("workers by repo", &map_text(&repos), false));
    }
    if !phases.is_empty() {
        lines.push(pair("active phases", &map_text(&phases), false));
    }
    if !management.is_empty() {
        let auto = management
            .iter()
            .filter(|(_, mode)| *mode == ManagementMode::Auto)
            .count();
        let manual = management
            .iter()
            .filter(|(_, mode)| *mode == ManagementMode::Manual)
            .count();
        lines.push(pair(
            "management",
            &format!("auto={auto}, manual={manual}"),
            false,
        ));
    }
    // The management pair above counts live workers; this one counts the pull
    // requests that management is actually withholding from the merge gate, which
    // is the state an operator mistakes for a merge that failed.
    let manual_merges = ledger.manual_merge_skips();
    if manual_merges != 0 {
        lines.push(pair(
            "merge-eligible, manual",
            &manual_merges.to_string(),
            false,
        ));
    }
    if !ledger.skip_list.is_empty() {
        lines.push(pair("skip list", &list_text(&ledger.skip_list), false));
    }
    let standoffs = super::decisions::standoffs(decisions);
    if !standoffs.is_empty() {
        // Without this the operator cannot tell an overseer standing off a task
        // another agent holds from an overseer that simply has nothing to do.
        lines.push(pair("standing off", &standoffs.join(", "), false));
    }
    lines.push(Line::default());
}

pub(super) fn append_worker_management(
    lines: &mut Vec<Line<'static>>,
    management: &[WorkerManagement],
) {
    for (label, mode) in management {
        lines.push(pair(
            &format!("worker {label}"),
            management_name(*mode),
            false,
        ));
    }
}

/// Merge recovery as one reading: the switch and, when it is on, the number of
/// handbacks a stuck pull request has left before it escalates to the operator.
fn merge_recovery_state(config: &OverseerConfig) -> String {
    if config.merge_recovery_enabled {
        format!("on (max {})", config.max_merge_recoveries)
    } else {
        "off".into()
    }
}

fn dispatches_on(ledger: &Ledger, today: NaiveDate) -> u32 {
    if ledger.counters.date == Some(today) {
        ledger.counters.dispatched_today
    } else {
        0
    }
}

fn pair(label: &str, value: &str, warn: bool) -> Line<'static> {
    let value_style = if warn {
        Style::default().fg(Color::Red)
    } else {
        THEME.accent_style()
    };
    Line::from(vec![
        Span::styled(format!("{label}: "), THEME.muted_style()),
        Span::styled(value.to_string(), value_style),
    ])
}

pub(super) fn flags_line(segments: &[(&str, String, bool)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (label, value, warn)) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", THEME.muted_style()));
        }
        spans.push(Span::styled(format!("{label}: "), THEME.muted_style()));
        let value_style = if *warn {
            Style::default().fg(Color::Red)
        } else {
            THEME.accent_style()
        };
        spans.push(Span::styled(value.clone(), value_style));
    }
    Line::from(spans)
}

pub(super) fn warning(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::Red),
    ))
}
fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
fn list_text(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.join(", ")
    }
}
fn map_text<K: std::fmt::Display>(map: &BTreeMap<K, usize>) -> String {
    if map.is_empty() {
        "none".into()
    } else {
        map.iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
pub(super) fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

fn management_name(mode: ManagementMode) -> &'static str {
    match mode {
        ManagementMode::Auto => "Auto",
        ManagementMode::Manual => "Manual",
    }
}
