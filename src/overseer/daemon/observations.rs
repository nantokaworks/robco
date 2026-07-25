use super::{COMMAND_TIMEOUT, terminal};
use crate::{
    Result,
    overseer::{
        exec::run_timeout,
        inbox::InboxReader,
        is_overseer_child,
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        monitor::{Observations, SessionObservation},
    },
    registry::Registry,
};
use chrono::{DateTime, Utc};
use external_state::{gather_pr_states, gather_task_states};
use std::process::Command;

#[path = "external_state.rs"]
mod external_state;
#[path = "liveness.rs"]
mod liveness;

pub(super) fn gather(ledger: &Ledger, inbox: &mut InboxReader) -> Observations {
    let mut observations = Observations::default();
    match inbox.read_new() {
        Ok(reports) => observations.inbox = reports.into_iter().map(Into::into).collect(),
        Err(error) => observations
            .errors
            .push(format!("inbox read failed: {error}")),
    }
    let registry = match Registry::load() {
        Ok(registry) => registry,
        Err(error) => {
            observations
                .errors
                .push(format!("registry read failed: {error}"));
            return observations;
        }
    };
    observations.manual_agents = registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .filter(|agent| agent.management == crate::model::ManagementMode::Manual)
        .map(|agent| agent.id.clone())
        .collect();
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
                    observations
                        .errors
                        .push(format!("registry recheck failed: {error}"));
                    continue;
                }
            }
        }
        // The registry sweep above already listed every registered Manual agent; the
        // recheck can surface one it missed, so fold that back in before reading it.
        if agent
            .as_ref()
            .is_some_and(|agent| agent.management == crate::model::ManagementMode::Manual)
            && !observations.manual_agents.contains(&entry.agent_id)
        {
            observations.manual_agents.push(entry.agent_id.clone());
        }
        let manual = observations.manual_agents.contains(&entry.agent_id);
        if let Some(agent) = agent {
            if entry.phase == LedgerPhase::Merged {
                observations.registered_agents.push(entry.agent_id.clone());
            }
            // Overseer never kills, restarts, or fails a Manual agent's session, so a
            // session probe could not drive any decision — but its PR state still has to
            // be collected below, or the entry never advances past the phase it was
            // frozen at. See `monitor::reconcile_manual_entry`.
            if manual {
                continue;
            }
            match liveness::probe_session_status(&agent.tmux_session) {
                Ok(dead) => observations.sessions.push(SessionObservation {
                    agent_id: entry.agent_id.clone(),
                    status: if dead { "dead" } else { "running" }.into(),
                    last_activity_at: (!dead)
                        .then(|| tmux_activity(&agent.tmux_session))
                        .flatten(),
                }),
                Err(error) => observations
                    .errors
                    .push(format!("tmux probe skipped: {error}")),
            }
        } else if !terminal(entry.phase) && !manual {
            observations.sessions.push(SessionObservation {
                agent_id: entry.agent_id.clone(),
                status: "dead".into(),
                last_activity_at: None,
            });
        }
    }
    let mut owned_ledger = ledger.clone();
    owned_ledger
        .entries
        .retain(|entry| !observations.detached_agents.contains(&entry.agent_id));
    // The dropr task state only feeds escalation, which never fires for a Manual
    // agent; PR state feeds phase advancement, which does.
    let mut auto_ledger = owned_ledger.clone();
    auto_ledger
        .entries
        .retain(|entry| !observations.manual_agents.contains(&entry.agent_id));
    gather_task_states(&auto_ledger, &mut observations);
    gather_pr_states(&owned_ledger, &mut observations);
    observations
}

/// Ledger entries whose worker is still registered but no longer an Overseer
/// child — the state `g` leaves behind when it detaches a Manual worker. An
/// entry whose agent has left the registry entirely is not detached; that is the
/// dead-session path, which [`gather`] still reports.
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
                        && !is_overseer_child(agent.parent_agent_id.as_deref())
                })
        })
        .map(|entry| entry.agent_id.clone())
        .collect()
}

fn tmux_activity(session: &str) -> Option<DateTime<Utc>> {
    let mut command = Command::new("tmux");
    command.args([
        "display-message",
        "-p",
        "-t",
        &format!("={session}"),
        "-F",
        "#{session_activity}",
    ]);
    let output = run_timeout(command, COMMAND_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let epoch = std::str::from_utf8(&output.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()?;
    DateTime::from_timestamp(epoch, 0)
}

pub(super) fn adopt_registry_children(ledger: &mut Ledger) -> Result<()> {
    let registry = Registry::load()?;
    adopt_registry_children_from(ledger, &registry);
    Ok(())
}

fn adopt_registry_children_from(ledger: &mut Ledger, registry: &Registry) {
    for repo in &registry.repos {
        for agent in repo.agents.iter().filter(|agent| {
            is_overseer_child(agent.parent_agent_id.as_deref())
                && agent.management == crate::model::ManagementMode::Auto
        }) {
            if ledger
                .entries
                .iter()
                .any(|entry| entry.agent_id == agent.id)
            {
                continue;
            }
            ledger.entries.push(LedgerEntry {
                task_id: agent.id.clone(),
                display_id: agent.title.clone(),
                repo: repo.path.to_string_lossy().into_owned(),
                agent_id: agent.id.clone(),
                branch: agent.branch.clone(),
                phase: LedgerPhase::Dispatched,
                dispatched_at: agent.created_at.with_timezone(&Utc),
                retries: 0,
                pr_url: None,
                branch_updates: 0,
                merge_recovery: Default::default(),
                manual_merge_skip: None,
            });
        }
    }
}

#[cfg(test)]
#[path = "observations_tests.rs"]
mod tests;
