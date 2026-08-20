use std::collections::BTreeMap;
use std::time::Duration;

use ratatui::text::Line;

use crate::locale::{Locale, fmt};
use crate::model::ManagementMode;
use crate::overseer::{
    config::{OverseerConfig, ProtectionMode},
    ledger::Ledger,
    logging::DecisionEntry,
};
use crate::registry::Registry;

use super::WorkerManagement;
pub(super) use super::render_format::{
    flags_line, list_text, management_name, map_text, merge_recovery_state, on_off, pair, terminal,
    warning,
};

pub(super) fn append_health(
    lines: &mut Vec<Line<'static>>,
    config: &OverseerConfig,
    alive: bool,
    heartbeat_age: Option<Duration>,
    daemon_version: Option<&str>,
    version_drift: Option<&str>,
    locale: Locale,
) {
    lines.push(flags_line(&[
        (
            "daemon",
            if alive { "alive" } else { "STALE / OFFLINE" }.into(),
            !alive,
        ),
        (
            "hb",
            heartbeat_age.map_or_else(
                || "missing".into(),
                |age| fmt(locale, "{}s ago", &[&age.as_secs().to_string()]),
            ),
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
    lines.push(flags_line(&[
        ("auto-merge", on_off(config.auto_merge).into(), false),
        (
            "protection",
            config.protection_mode.label().into(),
            config.auto_merge && config.protection_mode != ProtectionMode::Required,
        ),
        (
            "merge-recovery",
            merge_recovery_state(config),
            // Recovery without auto-merge is inert: nothing ever fails a merge
            // to hand back, so the setting reads as armed while doing nothing.
            config.merge_recovery_enabled && !config.auto_merge,
        ),
    ]));
    // Ahead of the other warnings for the same reason the CLI puts it first:
    // every flag above can read healthy while the daemon carries none of what
    // has been merged since it started.
    if let Some(drift) = version_drift {
        lines.push(warning(drift));
    }
    lines.push(Line::default());
}

pub(super) fn append_ledger(
    lines: &mut Vec<Line<'static>>,
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
    lines.push(flags_line(&[(
        "workers",
        format!("{} active", active.len()),
        false,
    )]));
    if !repos.is_empty() {
        lines.push(pair("workers by repo", &map_text(&repos), false));
    }
    let primary_holders = ledger.primary_holders();
    if !primary_holders.is_empty() {
        let holders = primary_holders
            .iter()
            .map(|(repo, display_id)| format!("{}={display_id}", registry.repo_label(repo)))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(pair("primary holder", &holders, false));
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
