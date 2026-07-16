use super::{COMMAND_TIMEOUT, terminal};
use crate::{
    Result,
    chief::{
        CHIEF_AGENT_ID,
        exec::run_timeout,
        inbox::InboxReader,
        ledger::{Ledger, LedgerEntry, LedgerPhase},
        monitor::{Observations, PrObservation, SessionObservation, TaskObservation},
    },
    dropr::DroprOverlay,
    registry::Registry,
};
use chrono::{DateTime, Utc};
use std::{collections::HashSet, process::Command};

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
    for entry in &ledger.entries {
        let agent = registry
            .repos
            .iter()
            .flat_map(|repo| &repo.agents)
            .find(|agent| agent.id == entry.agent_id);
        if let Some(agent) = agent {
            if entry.phase == LedgerPhase::Merged {
                observations.registered_agents.push(entry.agent_id.clone());
            }
            let mut command = Command::new("tmux");
            command.args(["has-session", "-t", &format!("={}", agent.tmux_session)]);
            match run_timeout(command, COMMAND_TIMEOUT) {
                Ok(output) => observations.sessions.push(SessionObservation {
                    agent_id: entry.agent_id.clone(),
                    status: if output.status.success() {
                        "running"
                    } else {
                        "dead"
                    }
                    .into(),
                    last_activity_at: output
                        .status
                        .success()
                        .then(|| tmux_activity(&agent.tmux_session))
                        .flatten(),
                }),
                Err(error) => observations
                    .errors
                    .push(format!("tmux probe skipped: {error}")),
            }
        } else if !terminal(entry.phase) {
            observations.sessions.push(SessionObservation {
                agent_id: entry.agent_id.clone(),
                status: "dead".into(),
                last_activity_at: None,
            });
        }
    }
    gather_task_states(ledger, &mut observations);
    gather_pr_states(ledger, &mut observations);
    observations
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

fn gather_task_states(ledger: &Ledger, observations: &mut Observations) {
    let workspaces = DroprOverlay::load_best_effort();
    let repos: HashSet<_> = ledger
        .entries
        .iter()
        .filter(|entry| !terminal(entry.phase))
        .map(|entry| entry.repo.as_str())
        .collect();
    for repo in repos {
        gather_repo_task_states(repo, ledger, &workspaces, observations);
    }
}

fn gather_repo_task_states(
    repo: &str,
    ledger: &Ledger,
    workspaces: &DroprOverlay,
    observations: &mut Observations,
) {
    let mut command = Command::new("git");
    command.args(["-C", repo, "remote", "get-url", "origin"]);
    let output = match run_timeout(command, COMMAND_TIMEOUT) {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            observations.errors.push(format!(
                "git origin probe exited {} for {repo}",
                output.status
            ));
            return;
        }
        Err(error) => {
            observations
                .errors
                .push(format!("git origin probe skipped for {repo}: {error}"));
            return;
        }
    };
    let origin = String::from_utf8_lossy(&output.stdout);
    let Some(workspace) = workspaces.find_by_repo_url(origin.trim()) else {
        observations
            .errors
            .push(format!("dropr workspace not found for {repo}"));
        return;
    };
    let Some(tasks) = crate::dropr::fetch_repo_tasks(&workspace.id) else {
        observations
            .errors
            .push(format!("dropr task probe skipped for {repo}"));
        return;
    };
    for entry in ledger
        .entries
        .iter()
        .filter(|entry| entry.repo == repo && !terminal(entry.phase))
    {
        for task in tasks
            .iter()
            .filter(|task| task.display_id == entry.display_id || task.display_id == entry.task_id)
        {
            observations.tasks.push(TaskObservation {
                task_id: entry.task_id.clone(),
                state: task.status.clone(),
            });
        }
    }
}

fn gather_pr_states(ledger: &Ledger, observations: &mut Observations) {
    for entry in ledger.entries.iter().filter(|entry| !terminal(entry.phase)) {
        let mut command = Command::new("gh");
        let selector = entry.pr_url.as_deref().unwrap_or(&entry.branch);
        command.current_dir(&entry.repo).args([
            "pr",
            "view",
            selector,
            "--json",
            "state,statusCheckRollup,url",
        ]);
        match run_timeout(command, COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                match serde_json::from_slice::<PrObservation>(&output.stdout) {
                    Ok(mut pr) => {
                        pr.task_id = Some(entry.task_id.clone());
                        observations.prs.push(pr);
                    }
                    Err(error) => observations
                        .errors
                        .push(format!("gh PR JSON skipped: {error}")),
                }
            }
            Ok(output) => observations
                .errors
                .push(format!("gh PR probe exited {}", output.status)),
            Err(error) => observations
                .errors
                .push(format!("gh PR probe skipped: {error}")),
        }
    }
}

pub(super) fn adopt_registry_children(ledger: &mut Ledger) -> Result<()> {
    let registry = Registry::load()?;
    for repo in &registry.repos {
        for agent in repo
            .agents
            .iter()
            .filter(|agent| agent.parent_agent_id.as_deref() == Some(CHIEF_AGENT_ID))
        {
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
            });
        }
    }
    Ok(())
}
