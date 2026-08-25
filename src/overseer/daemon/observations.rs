use super::{COMMAND_TIMEOUT, terminal};
use crate::{
    overseer::{
        exec::run_timeout,
        inbox::InboxReader,
        is_worker_subagent,
        ledger::Ledger,
        monitor::{ObservationError, Observations, SessionObservation},
    },
    registry::Registry,
};
use branch_activity::gather_branch_activity;
use chrono::{DateTime, Utc};
use external_state::{gather_pr_states, gather_task_states};
use std::process::Command;

#[path = "branch_activity.rs"]
mod branch_activity;
#[path = "external_state.rs"]
pub(super) mod external_state;
#[path = "liveness.rs"]
mod liveness;

pub(super) fn gather(
    ledger: &mut Ledger,
    inbox: &mut InboxReader,
    now: DateTime<Utc>,
) -> Observations {
    let mut observations = Observations::default();
    match inbox.read_new() {
        Ok(reports) => observations.inbox = reports.into_iter().map(Into::into).collect(),
        Err(error) => observations
            .errors
            .push(ObservationError::new(format!("inbox read failed: {error}"))),
    }
    let registry = match Registry::load() {
        Ok(registry) => registry,
        Err(error) => {
            observations.errors.push(ObservationError::new(format!(
                "registry read failed: {error}"
            )));
            return observations;
        }
    };
    // Every pass, not just the daemon's first: a worker started while the
    // daemon is already running never shows up otherwise (dropr:489). This
    // reuses the registry load just above instead of reading it twice, and
    // runs before the loop below so this same pass's session/PR probes and
    // `registered_agents` list already cover the newly adopted entry.
    adopt_registry_children_from(ledger, &registry, now);
    observations.detached_agents = detached_agents(ledger, &registry);
    for entry in &ledger.entries {
        // A detached worker is not ours to probe: `monitor::reconcile` drops its
        // entry this same pass, so no session, task, or PR state is collected.
        if observations.detached_agents.contains(&entry.agent_id) {
            continue;
        }
        let mut agent = registry
            .repos
            .iter()
            .flat_map(|repo| &repo.agents)
            .find(|agent| agent.id == entry.agent_id)
            .cloned();
        if agent.is_none() && !terminal(entry.phase) {
            match Registry::load() {
                Ok(refreshed) => {
                    agent = refreshed
                        .repos
                        .iter()
                        .flat_map(|repo| &repo.agents)
                        .find(|agent| agent.id == entry.agent_id)
                        .cloned();
                }
                Err(error) => {
                    observations.errors.push(
                        ObservationError::new(format!("registry recheck failed: {error}"))
                            .about(&entry.task_id, &entry.repo),
                    );
                    continue;
                }
            }
        }
        if let Some(agent) = agent {
            // Every phase, not just `merged`. `monitor::reconcile` only asks
            // about a merged entry — whose cleanup is re-pushed for as long as
            // the row survives — but retention asks about a settled one of any
            // phase, and a list that answered "no" for a `failed` worker whose
            // session is still standing would let its entry be forgotten while
            // the worktree it names is still there.
            observations.registered_agents.push(entry.agent_id.clone());
            match liveness::probe_session_status(&agent.tmux_session) {
                Ok(dead) => {
                    let last_activity_at = if dead {
                        None
                    } else {
                        match tmux_activity(&agent.tmux_session) {
                            Ok(at) => Some(at),
                            // A probe fault is not the same signal as "no
                            // activity" — silently treating it as one is
                            // what let a hung worker's stuck check go
                            // completely blind. Log it and leave this tick's
                            // reading absent instead of guessing.
                            Err(error) => {
                                observations.errors.push(
                                    ObservationError::new(format!(
                                        "tmux activity probe faulted: {error}"
                                    ))
                                    .about(&entry.task_id, &entry.repo),
                                );
                                None
                            }
                        }
                    };
                    observations.sessions.push(SessionObservation {
                        agent_id: entry.agent_id.clone(),
                        status: if dead { "dead" } else { "running" }.into(),
                        last_activity_at,
                    });
                }
                Err(error) => observations.errors.push(
                    ObservationError::new(format!("tmux probe skipped: {error}"))
                        .about(&entry.task_id, &entry.repo),
                ),
            }
        } else if !terminal(entry.phase) {
            observations.sessions.push(SessionObservation {
                agent_id: entry.agent_id.clone(),
                status: "dead".into(),
                last_activity_at: None,
            });
        }
    }
    let mut owned_ledger = ledger.clone();
    owned_ledger.entries.retain(|entry| {
        !observations.detached_agents.contains(&entry.agent_id)
            && registry.contains_repo_path(&entry.repo)
    });
    gather_task_states(&owned_ledger, &mut observations, now);
    gather_pr_states(&owned_ledger, &mut observations, now);
    gather_branch_activity(&owned_ledger, &mut observations, now);
    observations
}

/// Ledger entries whose worker is still registered but has since become a
/// subagent of another worker — its `parent_agent_id` now names a worker the
/// registry still lists, so it belongs to that worker's own worktree, not to
/// a ledger entry of its own (dropr:521). An entry whose agent has left the
/// registry entirely is not detached; that is the dead-session path, which
/// [`gather`] still reports.
fn detached_agents(ledger: &Ledger, registry: &Registry) -> Vec<String> {
    ledger
        .entries
        .iter()
        .filter(|entry| {
            registry
                .repos
                .iter()
                .flat_map(|repo| &repo.agents)
                .any(|agent| {
                    agent.id == entry.agent_id
                        && is_worker_subagent(agent.parent_agent_id.as_deref(), registry)
                })
        })
        .map(|entry| entry.agent_id.clone())
        .collect()
}

/// Reads a live session's start-or-most-recent-activity time (tmux's
/// `#{session_activity}`). `Err` means the probe itself faulted — command
/// failure, non-zero exit, or output that would not parse — and callers
/// must not read that the same way as "no activity"; see [`gather`].
///
/// The target is `={session}:`, not the bare `={session}`: on tmux 3.7,
/// `display-message` against a bare session target exits 0 and prints an
/// empty string for pane/window-scoped format variables (the same failure
/// mode documented on `tmux::session::exact`, which this daemon-side probe
/// predates and did not go through). The trailing `:` selects the session's
/// default window/pane, which resolves `#{session_activity}` correctly.
fn tmux_activity(session: &str) -> std::result::Result<DateTime<Utc>, String> {
    let mut command = Command::new("tmux");
    command.args([
        "display-message",
        "-p",
        "-t",
        &format!("={session}:"),
        "-F",
        "#{session_activity}",
    ]);
    let output = run_timeout(command, COMMAND_TIMEOUT).map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "tmux display-message exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let raw = std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("tmux display-message output was not utf8: {error}"))?
        .trim();
    let epoch: i64 = raw
        .parse()
        .map_err(|error| format!("tmux display-message returned {raw:?}: {error}"))?;
    DateTime::from_timestamp(epoch, 0)
        .ok_or_else(|| format!("session_activity epoch {epoch} out of range"))
}

/// Adopts every registry agent that is not another worker's subagent
/// (`is_worker_subagent`) and has no ledger entry yet.
///
/// dropr:521: there is no such thing as a worktree the Overseer owns any
/// more. Every worker the registry lists is one the daemon can act on —
/// pressing `m` is what decides it acts, not which binary or path created
/// the worker. The one exclusion left is a worker's own child: a subagent
/// `robco new` spawned from inside a running worker session belongs to that
/// worker's own worktree, not to a ledger entry of its own.
///
/// `dispatched_at` is stamped from `now` — this pass's own clock, not
/// `agent.created_at` (dropr:523). Adoption can run long after an agent was
/// actually created (the daemon was down, or the agent came from a stale
/// binary that only just started reaching the ledger); stamping the older
/// timestamp would make the entry look stuck against `stuck_after_mins`
/// before the daemon ever watched it for a single minute. See
/// `monitor::apply::apply_session`'s `dispatched_at` floor.
fn adopt_registry_children_from(ledger: &mut Ledger, registry: &Registry, now: DateTime<Utc>) {
    for repo in &registry.repos {
        for agent in repo
            .agents
            .iter()
            .filter(|agent| !is_worker_subagent(agent.parent_agent_id.as_deref(), registry))
        {
            if ledger
                .entries
                .iter()
                .any(|entry| entry.agent_id == agent.id)
            {
                continue;
            }
            ledger.entries.push(crate::overseer::ledger::new_entry(
                agent,
                &repo.path.to_string_lossy(),
                now,
            ));
        }
    }
}

#[cfg(test)]
#[path = "observations_tests.rs"]
mod tests;
