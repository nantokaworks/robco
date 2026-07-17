use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use chrono::{NaiveDate, Utc};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};

use crate::{
    chief::{
        config::ChiefConfig,
        heartbeat_path,
        ledger::{Ledger, LedgerPhase},
        logging::{self, DecisionKind},
    },
    config::Config,
    model::Status,
};

use super::theme::DEFAULT as THEME;

pub(super) fn summary() -> (String, Text<'static>) {
    let config = Config::load().map(|config| config.chief);
    let ledger = Ledger::load();
    let heartbeat = heartbeat_path();
    let mut lines = Vec::new();

    match (config, ledger, heartbeat) {
        (Ok(config), Ok(ledger), Ok(heartbeat)) => {
            append_health(&mut lines, &config, &ledger, &heartbeat);
            append_ledger(&mut lines, &config, &ledger);
            append_decisions(&mut lines);
        }
        (config, ledger, heartbeat) => {
            lines.push(warning("Chief state could not be read."));
            if let Err(error) = config {
                lines.push(detail("config", error));
            }
            if let Err(error) = ledger {
                lines.push(detail("ledger", error));
            }
            if let Err(error) = heartbeat {
                lines.push(detail("heartbeat", error));
            }
        }
    }
    ("CHIEF / local control".into(), lines.into())
}

pub(super) fn status() -> Status {
    let config = Config::load()
        .map(|config| config.chief)
        .unwrap_or_default();
    if crate::chief::daemon_pid_alive()
        && heartbeat_path()
            .ok()
            .is_some_and(|path| heartbeat_is_fresh(&path, config.poll_interval_secs))
    {
        Status::Running
    } else {
        Status::Dead
    }
}

pub(super) fn heartbeat_is_fresh(path: &Path, poll_secs: u64) -> bool {
    heartbeat_is_fresh_at(path, poll_secs, SystemTime::now())
}

fn heartbeat_is_fresh_at(path: &Path, poll_secs: u64, now: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age <= heartbeat_limit(poll_secs))
}

fn heartbeat_limit(poll_secs: u64) -> Duration {
    Duration::from_secs(poll_secs.saturating_mul(2).max(5))
}

fn append_health(
    lines: &mut Vec<Line<'static>>,
    config: &ChiefConfig,
    ledger: &Ledger,
    path: &Path,
) {
    let age = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let alive =
        crate::chief::daemon_pid_alive() && heartbeat_is_fresh(path, config.poll_interval_secs);
    lines.push(pair(
        "daemon",
        if alive { "alive" } else { "STALE / OFFLINE" },
        !alive,
    ));
    lines.push(pair(
        "heartbeat",
        &age.map_or_else(|| "missing".into(), |age| format!("{}s ago", age.as_secs())),
        !alive,
    ));
    let dispatch_without_daemon = config.dispatch_enabled && !alive;
    lines.push(pair(
        "dispatch",
        on_off(config.dispatch_enabled),
        dispatch_without_daemon,
    ));
    if dispatch_without_daemon {
        lines.push(warning(crate::chief::DISPATCH_WITHOUT_DAEMON_HINT));
    }
    lines.push(pair("auto-merge", on_off(config.auto_merge), false));
    let circuit_open = ledger.counters.consecutive_failures >= config.failure_circuit_threshold;
    lines.push(pair(
        "circuit",
        if circuit_open { "OPEN" } else { "closed" },
        circuit_open,
    ));
    lines.push(Line::default());
}

fn append_ledger(lines: &mut Vec<Line<'static>>, config: &ChiefConfig, ledger: &Ledger) {
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
    for entry in &ledger.entries {
        *phases.entry(phase_name(entry.phase)).or_insert(0usize) += 1;
    }
    lines.push(pair(
        "dispatches today",
        &format!(
            "{} / {}",
            dispatches_on(ledger, Utc::now().date_naive()),
            config.daily_dispatch_limit
        ),
        false,
    ));
    lines.push(pair(
        "workers",
        &format!(
            "{} / {} global; {} per repo",
            active.len(),
            config.max_workers,
            config.per_repo_limit
        ),
        false,
    ));
    lines.push(pair("workers by repo", &map_text(&repos), false));
    lines.push(pair("ledger phases", &map_text(&phases), false));
    lines.push(pair("skip list", &list_text(&ledger.skip_list), false));
    let escalations = ledger
        .entries
        .iter()
        .filter(|entry| entry.phase == LedgerPhase::Escalated)
        .map(|entry| format!("{} {}", entry.display_id, entry.repo))
        .collect::<Vec<_>>();
    lines.push(pair(
        "escalations",
        &list_text(&escalations),
        !escalations.is_empty(),
    ));
    lines.push(Line::default());
}

fn dispatches_on(ledger: &Ledger, today: NaiveDate) -> u32 {
    if ledger.counters.date == Some(today) {
        ledger.counters.dispatched_today
    } else {
        0
    }
}

fn append_decisions(lines: &mut Vec<Line<'static>>) {
    lines.push(Line::from(Span::styled(
        "recent decisions",
        THEME.accent_bold_style(),
    )));
    match logging::tail(8) {
        Ok(entries) if entries.is_empty() => {
            lines.push(Line::from(Span::styled("  none", THEME.muted_style())))
        }
        Ok(entries) => {
            for entry in entries.into_iter().rev() {
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
        Err(error) => lines.push(detail("decision log", error)),
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

fn warning(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(Color::Red),
    ))
}
fn detail(label: &str, error: impl std::fmt::Display) -> Line<'static> {
    warning(&format!("{label}: {error}"))
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
fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_heartbeat_is_not_fresh() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("heartbeat");
        fs::write(&path, "tick").unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(heartbeat_is_fresh_at(
            &path,
            10,
            modified + Duration::from_secs(20)
        ));
        assert!(!heartbeat_is_fresh_at(
            &path,
            10,
            modified + Duration::from_secs(21)
        ));
        assert!(!heartbeat_is_fresh_at(
            &temp.path().join("missing"),
            10,
            modified
        ));
    }

    #[test]
    fn stale_dispatch_counter_renders_zero() {
        let today = Utc::now().date_naive();
        let mut ledger = Ledger::default();
        ledger.counters.date = today.pred_opt();
        ledger.counters.dispatched_today = 7;
        let mut lines = Vec::new();
        append_ledger(&mut lines, &ChiefConfig::default(), &ledger);
        let rendered = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.starts_with("dispatches today: 0 / "));
    }
}
