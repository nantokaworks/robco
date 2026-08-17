//! Periodic per-repository health watch: security-advisory drift
//! (`repo_watch_advisory`) and stale/conflicted Dependabot pull requests
//! (`repo_watch_dependabot`). Detection only — filing the coordination task
//! is as far as this goes; a human or a dispatched worker still does the
//! actual fix (dropr task #430).
//!
//! Kept apart from `external_prs`: that pass lists every open pull request on
//! every due repository every five minutes so the TUI has something to show.
//! This one runs `bun audit` / `govulncheck` / a Dependabot-scoped `gh pr
//! list` on a much longer, config-driven cadence and only acts — by filing a
//! dropr task — when a condition is actually found.

use chrono::{DateTime, Utc};

use super::COMMAND_TIMEOUT;
use crate::{
    Result,
    config::Config,
    dropr,
    overseer::{logging, repo_watch::RepoWatchState},
    registry::Registry,
};

/// Runs the watch for every due, Overseer-managed repository.
///
/// Best-effort and self-contained, the same tolerance `external_prs`'s pass
/// gives every other GitHub-touching daemon pass: one repository's `bun` /
/// `govulncheck` / `gh` failure is logged and skipped rather than aborting
/// the whole tick or the rest of the repository list.
pub(super) fn watch_pass(config: &Config, now: DateTime<Utc>) -> Result<()> {
    if !config.overseer.repo_watch_enabled {
        return Ok(());
    }
    let registry = Registry::load()?;
    let (workspaces, overlay_ok) = dropr::DroprOverlay::load_with_status_timeout(COMMAND_TIMEOUT);
    if !overlay_ok {
        // `dispatch::gather` already records `dropr_overlay_unavailable` for
        // this same outage every tick; this pass just sits out until the
        // overlay answers again rather than logging the same fact twice.
        return Ok(());
    }
    let mut state = RepoWatchState::load()?;
    let interval = chrono::Duration::hours(config.overseer.repo_watch_interval_hours as i64);
    for repo in &registry.repos {
        if repo.management == crate::model::ManagementMode::Manual {
            continue;
        }
        let Some(remote) = &repo.remote_url else {
            continue;
        };
        let Some(workspace) = workspaces.find_by_repo_url(remote) else {
            continue;
        };
        if !workspace.is_materialised() {
            continue;
        }
        let key = repo.path.to_string_lossy().into_owned();
        if !due(state.repos.get(&key), now, interval) {
            continue;
        }
        if workspace.id.trim().is_empty() {
            // A materialised workspace with no usable id would otherwise reach
            // `task_create` and be refused there instead of here — caught
            // early and named so the cause (a bad overlay parse, most likely)
            // is visible in the log rather than read as a dropr-side refusal
            // (dropr task #445). Gated on `due` and recorded like a real
            // check below, so a broken workspace does not re-log every tick.
            logging::log_message(
                None,
                &format!(
                    "repo watch skipped for {}: workspace {:?} is materialised but has no usable id",
                    repo.name, workspace.name
                ),
            )?;
            state.repos.insert(key, now);
            continue;
        }
        if let Err(error) = super::repo_watch_advisory::run(
            &repo.path,
            &workspace.id,
            &repo.name,
            &mut state.reported_missing_tools,
        ) {
            logging::log_message(
                None,
                &format!("advisory watch failed for {}: {error}", repo.name),
            )?;
        }
        if let Err(error) = super::repo_watch_dependabot::run(
            &repo.path,
            &workspace.id,
            &repo.name,
            config.overseer.dependabot_stale_after_days,
        ) {
            logging::log_message(
                None,
                &format!("dependabot watch skipped for {}: {error}", repo.name),
            )?;
        }
        state.repos.insert(key, now);
    }
    state.save()
}

/// Whether a repository's watch is due — absent entirely counts as due, the
/// same convention `external_prs::is_due` uses.
fn due(checked_at: Option<&DateTime<Utc>>, now: DateTime<Utc>, interval: chrono::Duration) -> bool {
    checked_at.is_none_or(|at| now.signed_duration_since(*at) >= interval)
}

#[cfg(test)]
#[path = "repo_watch_tests.rs"]
mod tests;
