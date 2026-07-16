use super::{
    CHIEF_AGENT_ID,
    exec::{PidGuard, append_jsonl, execute_actions, log_message, run_timeout},
    heartbeat_path,
    inbox::InboxReader,
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    monitor::{
        ObservationSnapshot, Observations, PrObservation, SessionObservation, TaskObservation,
        reconcile,
    },
    pidfile_path, snapshots_path,
};
use crate::{Result, config::Config, dropr::DroprOverlay, registry::Registry};
use chrono::{DateTime, Utc};
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
pub async fn run_daemon() -> Result<()> {
    let _pid_guard = PidGuard::acquire(pidfile_path()?)?;
    let mut config = Config::load()?;
    let mut ledger = Ledger::load()?;
    adopt_registry_children(&mut ledger)?;
    ledger.save()?;
    let mut inbox = InboxReader::new()?;
    loop {
        let started = Instant::now();
        let now = Utc::now();
        let mut observations = gather_observations(&ledger, &mut inbox);
        if let Err(error) = append_jsonl(
            &snapshots_path()?,
            &ObservationSnapshot {
                at: now,
                observations: observations.clone(),
            },
        ) {
            observations
                .errors
                .push(format!("snapshot write failed: {error}"));
        }
        let (next, actions) = reconcile(&ledger, &observations, now, config.chief.stuck_after_mins);
        execute_actions(&actions)?;
        next.save()?;
        inbox.commit()?;
        ledger = next;
        fs::write(heartbeat_path()?, now.to_rfc3339())?;
        if let Ok(reloaded) = Config::load() {
            config = reloaded;
        } else {
            log_message(None, "config reload failed; retaining previous config")?;
        }
        let interval = Duration::from_secs(config.chief.poll_interval_secs);
        let remaining = interval.saturating_sub(started.elapsed());
        if wait_for_shutdown(remaining).await? {
            return Ok(());
        }
    }
}
fn gather_observations(ledger: &Ledger, inbox: &mut InboxReader) -> Observations {
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
        } else if !matches!(
            entry.phase,
            LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
        ) {
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
    let mut repos = std::collections::HashSet::new();
    for entry in ledger.entries.iter().filter(|entry| !terminal(entry.phase)) {
        repos.insert(entry.repo.as_str());
    }
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
fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}
fn gather_pr_states(ledger: &Ledger, observations: &mut Observations) {
    for entry in ledger.entries.iter().filter(|entry| {
        !matches!(
            entry.phase,
            LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
        )
    }) {
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
fn adopt_registry_children(ledger: &mut Ledger) -> Result<()> {
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
async fn wait_for_shutdown(duration: Duration) -> Result<bool> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::time::sleep(duration) => Ok(false),
            result = tokio::signal::ctrl_c() => { result?; Ok(true) },
            _ = terminate.recv() => Ok(true),
        }
    }
    #[cfg(not(unix))]
    tokio::select! {
        _ = tokio::time::sleep(duration) => Ok(false),
        result = tokio::signal::ctrl_c() => { result?; Ok(true) },
    }
}
