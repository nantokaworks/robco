//! `robco status`: the one read an operator gets of what the daemon is
//! doing, and the line builders that read is assembled from.
//!
//! The default output answers exactly three questions — is anything waiting on
//! me, is anything stuck, and what is running right now — because those are the
//! only three an operator actually asked of this command (dropr:357). Everything
//! else this module used to print unconditionally (the raw ledger phase tally,
//! the skip list, the decision-log tail, `workers by repo` keyed by absolute
//! path) is still here, but only under `--debug`: it informs a decision only
//! when something above pointed at it first.

use std::{
    collections::BTreeMap,
    fs,
    time::{Duration, SystemTime},
};

use super::{read_pid, settings::protection_warning};
use crate::{
    Result,
    config::Config,
    overseer::{
        config::OverseerConfig,
        daemon::merge_pass_telemetry,
        exec::process_alive,
        heartbeat, heartbeat_path,
        ledger::{ActiveWorkers, Ledger},
        logging, merge_pass_path,
        session::health::SessionHealth,
    },
    registry::Registry,
    ui::inbox::{self, InboxItem},
};

#[path = "status_debug.rs"]
mod status_debug;
use status_debug::print_debug_section;

pub(crate) fn status(config: &Config, debug: bool) -> Result<()> {
    let ledger = Ledger::load()?;
    let registry = Registry::load()?;
    let pid = read_pid();
    let heartbeat = heartbeat_path()?;
    let heartbeat_age = fs::metadata(&heartbeat)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    let daemon_version = heartbeat::recorded_version(&heartbeat);
    let healthy = daemon_healthy(config.overseer.poll_interval_secs);
    let session_health = SessionHealth::load()?;

    let inbox = inbox::current(&registry)?;
    let waiting = waiting_reasons(&inbox.items);
    print_section(&waiting_summary(&waiting), &waiting);

    let stuck = stuck_reasons(
        &config.overseer,
        healthy,
        daemon_version.as_deref(),
        session_health.as_ref(),
        logging::corrupt_line_count()?,
    );
    print_section(&stuck_summary(&stuck), &stuck);

    println!(
        "{}",
        running_line(
            &ledger.active_workers(),
            &ledger.primary_holders(),
            &registry
        )
    );

    if debug {
        let merge_pass = merge_pass_telemetry::load(&merge_pass_path()?);
        print_debug_section(
            config,
            &ledger,
            session_health.as_ref(),
            healthy,
            pid,
            heartbeat_age,
            daemon_version.as_deref(),
            merge_pass.as_ref(),
        )?;
    }
    Ok(())
}

/// Prints a summary line, then one indented, single-line reason per row — a
/// count that reads as an empty list prints no rows at all.
fn print_section(summary: &str, reasons: &[String]) {
    println!("{summary}");
    for reason in reasons {
        println!("  - {}", summarize_reason(reason));
    }
}

/// Longest a summarized reason is allowed to run, in characters. An
/// escalation reason can run to a multi-line explanation (dropr:357) —
/// readable in the TUI's Inbox preview or under `--debug`'s decision-log
/// tail, but the opposite of "answerable at a glance" printed whole into a
/// terminal.
const REASON_LINE_LIMIT: usize = 160;

/// Reduces a reason to the one line an operator needs to tell this row apart
/// from the others: its first line, trimmed, capped at [`REASON_LINE_LIMIT`]
/// characters. Cut in either direction — cut, an ellipsis says so; the full
/// text is never dropped, only not printed twice.
fn summarize_reason(reason: &str) -> String {
    let first_line = reason.lines().next().unwrap_or("").trim();
    let truncated: String = first_line.chars().take(REASON_LINE_LIMIT).collect();
    let cut = truncated.chars().count() < first_line.chars().count() || reason.lines().count() > 1;
    if cut {
        format!("{truncated}…")
    } else {
        truncated
    }
}

/// "Is anything waiting on me?" — every actionable Inbox item, the same
/// dismissal-aware, reconciled set the TUI's `N/M actionable` count reads.
///
/// Reusing the Inbox aggregation rather than the raw ledger phase tally is
/// what fixes `escalated: 2` reading as two things needing attention when an
/// operator already resolved both elsewhere (one task correctly marked
/// `blocked`, one already `closed`): the Inbox is dismissal-aware, the raw
/// tally never was.
fn waiting_reasons(items: &[InboxItem]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.actionable())
        .map(|item| format!("[{}] {}", item.remedy().tag(), item.label))
        .collect()
}

fn waiting_summary(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "waiting on you: none".to_string()
    } else {
        format!("waiting on you: {}", reasons.len())
    }
}

/// "Is anything stuck?" — every condition that stops the daemon from doing its
/// job on its own: offline, its build has drifted from what merged, dispatch
/// is offline, a credential the daemon needs has failed, a loosened merge
/// gate, or corrupted audit data. These are exactly the lines this command
/// used to print as scattered `warning:` lines — gathered here so a quiet
/// daemon reads as `stuck: none` instead of an operator having to confirm the
/// absence of several separate warnings by eye.
fn stuck_reasons(
    config: &OverseerConfig,
    healthy: bool,
    daemon_version: Option<&str>,
    session_health: Option<&SessionHealth>,
    corrupt_lines: usize,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if !healthy {
        reasons.push("daemon is offline or its heartbeat has gone stale".to_string());
    }
    // Gated on the daemon being up, like the version-drift check always was: a
    // dead daemon is already reported above, and naming the build it is not
    // running says nothing new.
    if healthy && let Some(warning) = heartbeat::drift(daemon_version) {
        reasons.push(warning);
    }
    if let Some(warning) = session_health.and_then(SessionHealth::warning) {
        reasons.push(warning.to_string());
    }
    if config.auto_merge
        && let Some(warning) = protection_warning(config.protection_mode)
    {
        reasons.push(warning.to_string());
    }
    if let Some(warning) = corrupt_lines_warning(corrupt_lines) {
        reasons.push(warning);
    }
    reasons
}

fn stuck_summary(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "stuck: none".to_string()
    } else {
        format!("stuck: {}", reasons.len())
    }
}

/// "What is running right now?" — active workers by repository (named, never
/// by the absolute path the ledger records them under) and which task holds
/// each repository's primary slot. A completed or terminal entry is not
/// "running" in the sense an operator means it.
fn running_line(
    active: &ActiveWorkers,
    primary_holders: &BTreeMap<String, String>,
    registry: &Registry,
) -> String {
    let workers = if active.count == 0 {
        "no active workers".to_string()
    } else {
        let by_repo = active
            .repos
            .iter()
            .map(|(path, count)| {
                let label = registry.repo_label(path);
                match primary_holders.get(path) {
                    Some(primary) => format!("{label}={count} (primary {primary})"),
                    None => format!("{label}={count}"),
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} worker(s) ({by_repo})", active.count)
    };
    format!("running now: {workers}")
}

/// A decision-log line that exists but does not parse is silently skipped by
/// every reader — there is nothing else to do with it — which otherwise makes
/// corruption (e.g. two records shredded together by a non-atomic append)
/// invisible forever. Surfacing the count here is what makes it visible
/// (dropr:VYj8In1jqvunWtAy3OtCo). Omitted while the count is zero, so the line
/// names an exception rather than a permanent zero.
fn corrupt_lines_warning(count: usize) -> Option<String> {
    if count == 0 {
        return None;
    }
    Some(format!(
        "{count} unparseable decision-log line(s) — corrupted entries are silently skipped; inspect ~/.robco/overseer/decisions.jsonl"
    ))
}

pub(super) fn daemon_healthy(poll_interval_secs: u64) -> bool {
    read_pid().is_some_and(process_alive)
        && heartbeat_path()
            .ok()
            .and_then(|path| fs::metadata(path).ok())
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| {
                age <= Duration::from_secs(poll_interval_secs.saturating_mul(2).max(5))
            })
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
