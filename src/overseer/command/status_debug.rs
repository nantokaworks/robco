//! The internal bookkeeping `robco overseer status --debug` prints: the raw
//! ledger phase tally, `workers by repo` keyed by absolute path, the skip
//! list, the decision-log tail, and the toggle/budget lines the three-question
//! default output moved out from under (see `status.rs`'s module doc). Kept
//! in its own file — split from `status.rs` per the project's file-size
//! limit — because it is a distinct concern: rendering the daemon's raw
//! records verbatim, not answering an operator's question about them.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::{
    Result,
    config::Config,
    model::ManagementMode,
    overseer::{
        command::on_off,
        config::OverseerConfig,
        daemon::merge_pass_telemetry::MergePassTelemetry,
        ledger::{Ledger, waiting_on_prerequisite},
        logging,
        review::ReviewPass,
        session::health::SessionHealth,
    },
    registry::Registry,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn print_debug_section(
    config: &Config,
    ledger: &Ledger,
    registry: &Registry,
    session_health: Option<&SessionHealth>,
    healthy: bool,
    pid: Option<u32>,
    heartbeat_age: Option<Duration>,
    daemon_version: Option<&str>,
    merge_pass: Option<&MergePassTelemetry>,
) -> Result<()> {
    println!();
    println!("--- debug ---");
    println!(
        "{}",
        daemon_line(healthy, pid, heartbeat_age, daemon_version)
    );
    println!(
        "{}",
        toggle_line(&config.overseer, ledger.merge_recovery_drops())
    );
    println!("workers: {}", ledger.active_workers().count);
    println!("{}", llm_line(config)?);
    println!("{}", merge_pass_line(merge_pass));
    println!("{}", discord_line(&config.overseer.discord));
    println!("{}", session_auth_line(session_health));
    println!("workers by repo: {:?}", ledger.active_workers().repos);
    println!("primary holder by repo: {:?}", ledger.primary_holders());
    println!("{}", repos_line(registry));
    let mut phases = BTreeMap::new();
    for entry in &ledger.entries {
        *phases.entry(entry.phase.label()).or_insert(0usize) += 1;
    }
    println!("phases: {phases:?}");
    // Not counted anywhere in `phases` above: a prerequisite wait leaves the
    // entry's phase untouched (`Working` or `PrOpened`), and is deliberately
    // absent from the three-question default output — it needs no operator,
    // so it is not "waiting on you" and it holds no capacity, so it is not
    // "running now" either. This is its one visible trace. See dropr:375.
    let prerequisite_waits = ledger
        .entries
        .iter()
        .filter(|entry| waiting_on_prerequisite(entry))
        .count();
    if prerequisite_waits != 0 {
        println!("waiting on prerequisite: {prerequisite_waits}");
    }
    let manual_merges = ledger.manual_merge_skips();
    if manual_merges != 0 {
        println!("merge-eligible, manual: {manual_merges}");
    }
    let queued_approvals = ledger.queued_merge_approvals();
    if queued_approvals != 0 {
        println!("merges queued (operator approved): {queued_approvals}");
    }
    println!("skip list: {:?}", ledger.skip_list);
    println!("recent decisions:");
    for entry in logging::tail(10)? {
        println!(
            "  {} {:?} task={} repo={} {}",
            entry.at.to_rfc3339(),
            entry.kind,
            entry.task.as_deref().unwrap_or("-"),
            entry.repo.as_deref().unwrap_or("-"),
            entry.reason
        );
    }
    Ok(())
}

/// Whether a session the daemon spawns can authenticate.
///
/// Reported unconditionally, including when no probe has ever run: an absent
/// record is itself the answer an operator needs after installing the service,
/// and a line that appears only on failure cannot be checked before the failure.
fn session_auth_line(health: Option<&SessionHealth>) -> String {
    health.map_or_else(
        || "session auth: unknown (no preflight recorded)".to_string(),
        SessionHealth::summary,
    )
}

/// How long the last auto-merge pass took, and which repository was slowest —
/// see `daemon::merge_pass_telemetry`. Repositories are now evaluated
/// concurrently, so no other line in this section says whether a pass is
/// stretching out; this is the one place that would show.
fn merge_pass_line(merge_pass: Option<&MergePassTelemetry>) -> String {
    let Some(pass) = merge_pass else {
        return "merge pass: none recorded yet".to_string();
    };
    let slowest = pass.slowest_repo.as_deref().map_or_else(
        || "-".to_string(),
        |repo| format!("{repo} ({}ms)", pass.slowest_repo_ms),
    );
    format!(
        "merge pass: {}ms  {} repos  slowest: {slowest}  at {}",
        pass.duration_ms,
        pass.repos_evaluated,
        pass.at.to_rfc3339()
    )
}

/// Today's board-review LLM spend.
fn llm_line(config: &Config) -> Result<String> {
    let review = ReviewPass::load()?.calls_today();
    Ok(format!(
        "llm today: review {review}/{} ({})",
        config.overseer.daily_review_budget,
        review_state(&config.overseer)
    ))
}

/// How the board review is running.
///
/// The pass itself always runs on its interval, and its deterministic findings
/// with it; `review_profile` decides only whether a reviewer model reads the same
/// digest afterwards. The line used to print `disabled` for a missing profile,
/// which read as "the review is off" — and was, which is the bug this wording
/// outlived. The two states are now named separately, because an operator seeing a
/// quiet board needs to know whether nothing was found or nothing looked.
fn review_state(config: &OverseerConfig) -> String {
    let interval = config.review_interval_mins;
    config.review_profile.as_deref().map_or_else(
        || format!("findings every {interval}m, no reviewer model"),
        |profile| format!("every {interval}m via {profile}"),
    )
}

/// Where the Overseer's reports — decision notifications and digests — go.
///
/// The report channel is set once and then invisible: nothing else prints it,
/// so a report landing in the wrong channel could only be diagnosed by opening
/// `config.json`. Naming the fallback explicitly is the point — `reports ->
/// <id> (chat channel)` tells the operator the dedicated channel is unset, not
/// that it happens to equal the chat one.
fn discord_line(discord: &crate::overseer::config::DiscordConfig) -> String {
    if !discord.enabled {
        return "discord: off".to_string();
    }
    let reports = match (&discord.notify_channel_id, &discord.channel_id) {
        (Some(notify), _) => format!("{notify} (notify-channel)"),
        (None, Some(chat)) => format!("{chat} (chat channel)"),
        (None, None) => "unset".to_string(),
    };
    format!("discord: on  reports -> {reports}")
}

/// The daemon's identity line: whether it is up, which process it is, when it
/// last reported, and — the one thing a running daemon cannot otherwise be
/// asked — which build it started from. A daemon keeps that build until the
/// service restarts, so `healthy` alone never says whether a merged fix is live.
fn daemon_line(
    healthy: bool,
    pid: Option<u32>,
    heartbeat_age: Option<Duration>,
    daemon_version: Option<&str>,
) -> String {
    format!(
        "daemon: {} pid={} heartbeat={} version={}",
        if healthy { "healthy" } else { "down/stale" },
        pid.map_or_else(|| "-".into(), |pid| pid.to_string()),
        heartbeat_age.map_or_else(|| "missing".into(), |age| format!("{}s", age.as_secs())),
        daemon_version.unwrap_or("unknown")
    )
}

/// Which registered repos the Overseer actually looks at (`#306`). Dispatch and
/// auto-merge already gate on `RepoNode::management` in the passes themselves;
/// this line is the one place that state is visible without opening the TUI or
/// digging through `decisions.jsonl` for a `overseer_unmanaged` skip.
fn repos_line(registry: &Registry) -> String {
    let opted_out: Vec<&str> = registry
        .repos
        .iter()
        .filter(|repo| repo.management == ManagementMode::Manual)
        .map(|repo| repo.name.as_str())
        .collect();
    if opted_out.is_empty() {
        return format!("repos: {} watched, 0 opted out", registry.repos.len());
    }
    format!(
        "repos: {} watched, {} opted out: {}",
        registry.repos.len() - opted_out.len(),
        opted_out.len(),
        opted_out.join(", ")
    )
}

/// Render the toggle summary line of `robco overseer status --debug`.
///
/// Every toggle reported here must be one the daemon actually honours; a switch
/// that is only displayed invites the reader to blame it for an outage it has no
/// part in.
fn toggle_line(config: &OverseerConfig, recovery_drops: u32) -> String {
    format!(
        "auto-merge: {} (protection: {})  merge-recovery: {}",
        on_off(config.auto_merge),
        config.protection_mode.label(),
        merge_recovery_state(config, recovery_drops),
    )
}

/// Merge recovery reads as one setting, so its cap travels with its switch: an
/// operator who sees only `on` cannot tell how many handbacks a stuck pull
/// request still has before it reaches them.
///
/// Switched off, the number that matters is the opposite one — how many failures
/// a worker could have fixed went to nobody. Without it `off` is a flag; with it
/// the flag has a consequence attached, which is what an operator needs to decide
/// whether to switch it on. It is omitted while nothing has been dropped, so the
/// line names an exception rather than a permanent zero.
fn merge_recovery_state(config: &OverseerConfig, drops: u32) -> String {
    if config.merge_recovery_enabled {
        return format!("on (max {})", config.max_merge_recoveries);
    }
    if drops == 0 {
        return "off".into();
    }
    format!("off ({drops} dropped)")
}

#[cfg(test)]
#[path = "status_debug_tests.rs"]
mod tests;
