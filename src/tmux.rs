use std::{path::Path, process::Command};

use crate::{Error, Result};

pub fn session_name(prefix: &str, repo: &str, agent: &str) -> String {
    format!(
        "{prefix}{}_{}",
        sanitize_target_part(repo),
        sanitize_target_part(agent)
    )
}

pub fn sanitize_target_part(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

pub fn has_session(session: &str) -> Result<bool> {
    let status = Command::new("tmux")
        .args(["has-session", "-t", session])
        .status()?;
    Ok(status.success())
}

pub fn new_session_command(session: &str, cwd: &Path, program: &str) -> Command {
    let mut command = Command::new("tmux");
    command
        .args(["new-session", "-d", "-s", session, "-c"])
        .arg(cwd)
        .arg(program);
    command
}

pub fn new_session(session: &str, cwd: &Path, program: &str) -> Result<()> {
    let output = new_session_command(session, cwd, program).output()?;
    command_unit(output, "tmux new-session")?;
    let _ = Command::new("tmux")
        .args(["set-window-option", "-t", session, "monitor-activity", "on"])
        .output();
    Ok(())
}

pub fn kill_session(session: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["kill-session", "-t", session])
        .output()?;
    command_unit(output, "tmux kill-session")
}

pub fn capture_plain(session: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-e", "-p", "-t", session])
        .output()?;
    command_output(output, "tmux capture-pane")
}

pub fn attach(session: &str) -> Result<()> {
    let in_tmux = std::env::var_os("TMUX").is_some();
    let mut command = Command::new("tmux");
    if in_tmux {
        command.args(["switch-client", "-t", session]);
    } else {
        command.args(["attach", "-t", session]);
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Command {
            context: "tmux attach",
            stderr: format!("tmux exited with {status}"),
        })
    }
}

fn command_unit(output: std::process::Output, context: &'static str) -> Result<()> {
    command_output(output, context).map(|_| ())
}

fn command_output(output: std::process::Output, context: &'static str) -> Result<String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    Err(Error::Command {
        context,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_tmux_target_parts() {
        assert_eq!(sanitize_target_part("foo.bar:baz"), "foo-bar-baz");
        assert_eq!(
            session_name("robco_", "my.repo", "fix/thing"),
            "robco_my-repo_fix-thing"
        );
    }
}
