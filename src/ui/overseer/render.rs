use std::{collections::BTreeMap, time::Duration};

use chrono::{NaiveDate, Utc};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::model::ManagementMode;
use crate::overseer::{
    config::OverseerConfig,
    ledger::{Ledger, LedgerPhase},
    logging::{DecisionEntry, DecisionKind},
};

use super::{App, WorkerManagement};
use crate::ui::{inbox::InboxKind, theme::DEFAULT as THEME};

pub(super) fn append_health(
    lines: &mut Vec<Line<'static>>,
    config: &OverseerConfig,
    ledger: &Ledger,
    alive: bool,
    heartbeat_age: Option<Duration>,
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
            "circuit",
            if circuit_open { "OPEN" } else { "closed" }.into(),
            circuit_open,
        ),
    ]));
    if dispatch_without_daemon {
        lines.push(warning(crate::overseer::DISPATCH_WITHOUT_DAEMON_HINT));
    }
    if !config.dispatch_enabled && alive && !circuit_open {
        lines.push(warning(crate::overseer::DISPATCH_STOPPED_HINT));
    }
    if circuit_open {
        lines.push(warning(crate::overseer::CIRCUIT_OPEN_HINT));
    }
    lines.push(Line::default());
}

pub(super) fn append_ledger(
    lines: &mut Vec<Line<'static>>,
    config: &OverseerConfig,
    ledger: &Ledger,
    management: &[WorkerManagement],
) {
    let active = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .collect::<Vec<_>>();
    let mut repos = BTreeMap::new();
    let mut phases = BTreeMap::new();
    for entry in &active {
        *repos.entry(entry.repo.as_str()).or_insert(0usize) += 1;
    }
    for entry in &active {
        *phases.entry(phase_name(entry.phase)).or_insert(0usize) += 1;
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
    if !ledger.skip_list.is_empty() {
        lines.push(pair("skip list", &list_text(&ledger.skip_list), false));
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

pub(super) fn append_inbox(lines: &mut Vec<Line<'static>>, app: &App) {
    lines.push(Line::from(Span::styled(
        format!("inbox ({})", app.overseer_inbox.len()),
        THEME.accent_bold_style(),
    )));
    if app.overseer_inbox.is_empty() {
        lines.push(Line::from(Span::styled("  none", THEME.muted_style())));
    }
    for (index, item) in app.overseer_inbox.iter().enumerate() {
        let marker = if index == app.overseer_inbox_selected {
            ">"
        } else {
            " "
        };
        let kind = match item.kind {
            InboxKind::Escalation => "ESC",
            InboxKind::Question => "?",
        };
        let target = item
            .target_session
            .as_deref()
            .map_or("display-only", |session| session);
        lines.push(Line::from(format!(
            " {marker} [{kind}] {} => {target}",
            item.label
        )));
    }
    lines.push(Line::from(Span::styled(
        "  [/] select   a answer   y approve",
        THEME.hint_style(),
    )));
    lines.push(Line::default());
}

fn dispatches_on(ledger: &Ledger, today: NaiveDate) -> u32 {
    if ledger.counters.date == Some(today) {
        ledger.counters.dispatched_today
    } else {
        0
    }
}

pub(super) fn append_decisions(lines: &mut Vec<Line<'static>>, decisions: &[DecisionEntry]) {
    lines.push(Line::from(Span::styled(
        "recent decisions",
        THEME.accent_bold_style(),
    )));
    // `decisions` is oldest-first (see `logging::tail`); show the newest three first.
    let recent = decisions.iter().rev().take(3).collect::<Vec<_>>();
    if recent.is_empty() {
        lines.push(Line::from(Span::styled("  none", THEME.muted_style())));
        return;
    }
    for entry in recent {
        let task = entry.task.as_deref().unwrap_or("-");
        let label = match entry.kind {
            DecisionKind::Escalate | DecisionKind::CircuitOpen => "!",
            _ => "·",
        };
        lines.push(Line::from(format!(
            "  {label} {} {task} — {}",
            entry.at.format("%m-%d %H:%M"),
            entry.reason
        )));
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
fn phase_name(phase: LedgerPhase) -> &'static str {
    match phase {
        LedgerPhase::Dispatched => "dispatched",
        LedgerPhase::Claimed => "claimed",
        LedgerPhase::Working => "working",
        LedgerPhase::PrOpened => "pr_opened",
        LedgerPhase::Merged => "merged",
        LedgerPhase::Failed => "failed",
        LedgerPhase::Escalated => "escalated",
    }
}
