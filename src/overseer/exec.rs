use super::{
    logging::{self, DecisionEntry, DecisionKind, log_message},
    monitor::Action,
    release_pipeline,
};
pub(crate) use crate::exec::run_timeout;
use crate::{
    Result,
    git::{
        merge_lock::with_merge_lock_if_free,
        post_merge::{Cleanup, OnFailure},
    },
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

/// Runs the pass's actions and reports which repositories the post-merge
/// fast-forward brought up to date.
///
/// The return value is what lifts the merge gate's per-repository barrier: the
/// pull runs here, on a later pass than the merge that needs it, so without it
/// the gate would have to assume the base caught up rather than know it. See
/// `crate::overseer::daemon::merge_settle`.
///
/// `release_pipeline_enabled` is `config.overseer.release_pipeline_enabled`,
/// threaded down to `Action::CheckReleasePipeline` rather than read from a
/// `Config` this function loads itself: the operator's opt-in for that
/// unattended-shell-execution privilege is decided once, by the same pass
/// that already holds the config, not re-read per action.
pub(crate) fn execute_actions(
    actions: &[Action],
    release_pipeline_enabled: bool,
) -> Result<HashSet<String>> {
    let mut cleanup_blocked = HashSet::new();
    let mut pulled = HashSet::new();
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
                } else {
                    match clean_up_agent(agent_id) {
                        Ok(Some(repo)) => {
                            pulled.insert(repo);
                        }
                        Ok(None) => {}
                        Err(error) => log_cleanup_failure(agent_id, "worktree", &error)?,
                    }
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
            Action::CheckReleasePipeline {
                task_id,
                repo,
                pr_url,
            } => release_pipeline::consider(
                task_id,
                repo,
                pr_url.as_deref(),
                release_pipeline_enabled,
            )?,
        }
    }
    Ok(pulled)
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
/// The cleanup runs the same sequence an interactive or MCP merge does, so it
/// runs under the repository's merge lock — the daemon is the third surface
/// that has to take it for the other two to mean anything.
///
/// It does not *wait* for the lock. A daemon pass covers every repository it
/// watches, and blocking one of them on however long a merge takes would stall
/// the whole pass. Losing the race is left for the next pass instead, which the
/// surviving registry row is what arranges — see [`clean_up_locked`].
///
/// Returns the repository path when the base fast-forward succeeded, which is
/// the signal the merge gate's per-repository barrier waits on. Contention
/// returns `None`: the pull did not run, so the barrier must not lift.
fn clean_up_agent(agent_id: &str) -> Result<Option<String>> {
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
        return Ok(None);
    };
    let ran = with_merge_lock_if_free(&repo, || {
        clean_up_locked(agent_id, &repo, &worktree, &branch)
    })?;
    let Some(pulled) = ran else {
        // Logged as its own thing rather than through `log_cleanup_failure`:
        // nothing failed, and calling it a failure would have an operator
        // reading the log for a cleanup that needs looking at find a merge
        // that was working as intended.
        log_message(
            None,
            &format!(
                "cleanup for {agent_id} deferred: another robco process is merging in {}",
                repo.display()
            ),
        )?;
        return Ok(None);
    };
    Ok(pulled)
}

/// The cleanup itself, with the repository's merge lock held.
///
/// The sequence lives in [`crate::git::post_merge`] so this path and the
/// interactive merge cannot drift apart. It runs under [`OnFailure::Continue`]:
/// nobody is watching the daemon, and a base branch that refuses to fast-forward
/// must not leave the worktree and the branch behind forever.
///
/// The registry row survives a failed worktree removal on purpose. It is what
/// makes the next reconcile pass re-emit the cleanup, so the retry keeps
/// happening until the worktree is actually gone. A pass that never got the lock
/// does not reach this function at all, so its row survives for the same reason.
fn clean_up_locked(
    agent_id: &str,
    repo: &Path,
    worktree: &Path,
    branch: &str,
) -> Result<Option<String>> {
    let outcome = Cleanup {
        repo,
        worktree,
        branch,
        on_failure: OnFailure::Continue,
    }
    .run(|_| ())?;
    for note in &outcome.notes {
        log_message(None, &format!("cleanup for {agent_id}: {note}"))?;
    }
    let pulled = outcome
        .base_pulled
        .then(|| repo.to_string_lossy().into_owned());
    if !outcome.worktree_removed {
        return Ok(pulled);
    }
    Registry::locked_update(|registry| {
        for repo in &mut registry.repos {
            repo.agents.retain(|agent| agent.id != agent_id);
        }
    })?;
    Ok(pulled)
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
