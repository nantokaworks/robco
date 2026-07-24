use super::{
    logging::{self, DecisionEntry, DecisionKind, log_message},
    monitor::Action,
};
pub(crate) use crate::exec::run_timeout;
use crate::{
    Result,
    git::post_merge::{Cleanup, OnFailure},
    registry::Registry,
};
use fd_lock::RwLock;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn process_alive(pid: u32) -> bool {
    let mut command = Command::new("kill");
    command.args(["-0", &pid.to_string()]);
    run_timeout(command, Duration::from_secs(2)).is_ok_and(|output| output.status.success())
}

pub(crate) fn execute_actions(actions: &[Action]) -> Result<()> {
    let mut cleanup_blocked = HashSet::new();
    for action in actions {
        match action {
            Action::KillSession { agent_id } => {
                if let Err(error) = kill_agent_session(agent_id) {
                    log_cleanup_failure(agent_id, "session", &error)?;
                    cleanup_blocked.insert(agent_id.as_str());
                }
            }
            Action::RemoveWorktree { agent_id } => {
                if cleanup_blocked.contains(agent_id.as_str()) {
                    log_message(
                        None,
                        &format!(
                            "worktree cleanup deferred for {agent_id}: session cleanup failed"
                        ),
                    )?;
                } else if let Err(error) = clean_up_agent(agent_id) {
                    log_cleanup_failure(agent_id, "worktree", &error)?;
                }
            }
            Action::Notify { message } => eprintln!("overseer: {message}"),
            Action::MarkFailed {
                task_id, reason, ..
            } => log_message(Some(task_id), reason)?,
            Action::Escalate { task_id, reason } => {
                let mut entry = DecisionEntry::new(DecisionKind::Escalate, reason);
                entry.task = Some(task_id.clone());
                entry.source = Some("reconcile".into());
                logging::append(&entry)?;
            }
            Action::LogDecision { task_id, message } => log_message(task_id.as_deref(), message)?,
        }
    }
    Ok(())
}

fn log_cleanup_failure(agent_id: &str, target: &str, error: &dyn std::fmt::Display) -> Result<()> {
    log_message(
        None,
        &format!("{target} cleanup failed for {agent_id}: {error}"),
    )
}

fn kill_agent_session(agent_id: &str) -> Result<()> {
    let registry = Registry::load()?;
    let Some(agent) = registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .find(|agent| agent.id == agent_id)
    else {
        return Ok(());
    };
    let target = format!("={}", agent.tmux_session);
    let mut probe = Command::new("tmux");
    probe.args(["has-session", "-t", &target]);
    if run_timeout(probe, COMMAND_TIMEOUT).is_ok_and(|output| output.status.success()) {
        let mut kill = Command::new("tmux");
        kill.args(["kill-session", "-t", &target]);
        let output = run_timeout(kill, COMMAND_TIMEOUT)?;
        if !output.status.success() {
            return Err(command_error("tmux kill-session", &output).into());
        }
    }
    Ok(())
}

/// Runs the post-merge cleanup for one merged agent and drops its registry row.
///
/// The sequence itself lives in [`crate::git::post_merge`] so this path and the
/// interactive merge cannot drift apart. It runs under [`OnFailure::Continue`]:
/// nobody is watching the daemon, and a base branch that refuses to fast-forward
/// must not leave the worktree and the branch behind forever.
///
/// The registry row survives a failed worktree removal on purpose. It is what
/// makes the next reconcile pass re-emit the cleanup, so the retry keeps
/// happening until the worktree is actually gone.
fn clean_up_agent(agent_id: &str) -> Result<()> {
    let registry = Registry::load()?;
    let target = registry.repos.iter().find_map(|repo| {
        repo.agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .map(|agent| {
                (
                    repo.path.clone(),
                    agent.worktree_path.clone(),
                    agent.branch.clone(),
                )
            })
    });
    let Some((repo, worktree, branch)) = target else {
        return Ok(());
    };
    let outcome = Cleanup {
        repo: &repo,
        worktree: &worktree,
        branch: &branch,
        on_failure: OnFailure::Continue,
    }
    .run(|_| ())?;
    for note in &outcome.notes {
        log_message(None, &format!("cleanup for {agent_id}: {note}"))?;
    }
    if !outcome.worktree_removed {
        return Ok(());
    }
    Registry::locked_update(|registry| {
        for repo in &mut registry.repos {
            repo.agents.retain(|agent| agent.id != agent_id);
        }
    })?;
    Ok(())
}

fn command_error(command: &str, output: &Output) -> std::io::Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    std::io::Error::other(format!(
        "{command} exited {}: {}",
        output.status,
        stderr.trim()
    ))
}

pub(crate) fn append_jsonl(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(crate) struct PidGuard {
    path: PathBuf,
}

impl PidGuard {
    pub(crate) fn acquire(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("pid.lock"))?;
        let mut lock = RwLock::new(lock_file);
        let _guard = lock.write()?;
        if let Ok(raw) = fs::read_to_string(&path)
            && raw.trim().parse::<u32>().ok().is_some_and(process_alive)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "overseer daemon is already running",
            )
            .into());
        }
        let _ = fs::remove_file(&path);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(std::process::id().to_string().as_bytes())?;
        Ok(Self { path })
    }
}

impl Drop for PidGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
