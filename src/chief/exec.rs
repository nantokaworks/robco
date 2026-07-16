use super::{decision_log_path, monitor::Action};
use crate::{Result, registry::Registry};
use chrono::Utc;
use fd_lock::RwLock;
use serde_json::json;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn run_timeout(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "command timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

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
            Action::RemoveWorktree { agent_id, .. } => {
                if cleanup_blocked.contains(agent_id.as_str()) {
                    log_message(
                        None,
                        &format!(
                            "worktree cleanup deferred for {agent_id}: session cleanup failed"
                        ),
                    )?;
                } else if let Err(error) = remove_agent_worktree(agent_id) {
                    log_cleanup_failure(agent_id, "worktree", &error)?;
                }
            }
            Action::Notify { message } => eprintln!("chief: {message}"),
            Action::MarkFailed { task_id, reason } | Action::Escalate { task_id, reason } => {
                log_message(Some(task_id), reason)?
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

fn remove_agent_worktree(agent_id: &str) -> Result<()> {
    let registry = Registry::load()?;
    let target = registry.repos.iter().find_map(|repo| {
        repo.agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .map(|agent| (repo.path.clone(), agent.worktree_path.clone()))
    });
    let Some((repo, worktree)) = target else {
        return Ok(());
    };
    if worktree.exists() {
        let mut command = Command::new("git");
        command
            .args(["-C"])
            .arg(&repo)
            .args(["worktree", "remove"])
            .arg(&worktree);
        let output = run_timeout(command, COMMAND_TIMEOUT)?;
        if !output.status.success() {
            return Err(command_error("git worktree remove", &output).into());
        }
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

pub(crate) fn log_message(task_id: Option<&str>, message: &str) -> Result<()> {
    append_jsonl(
        &decision_log_path()?,
        &json!({ "at": Utc::now(), "task_id": task_id, "message": message }),
    )
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
                "chief daemon is already running",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_out_hung_command_promptly() {
        let started = Instant::now();
        let mut command = Command::new("sleep");
        command.arg("5");
        let error = run_timeout(command, Duration::from_millis(100)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
