use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use crossterm::terminal;

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
    let output = Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()?;
    Ok(output.status.success())
}

fn has_server() -> Result<bool> {
    let output = Command::new("tmux").arg("list-sessions").output()?;
    Ok(output.status.success())
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

pub fn capture_text(session: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", session])
        .output()?;
    command_output(output, "tmux capture-pane")
}

pub fn resize_session(session: &str, width: u16, height: u16) -> Result<()> {
    let target = format!("{width}x{height}");
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            session,
            "#{window_width}x#{window_height}",
        ])
        .output()?;
    let current = command_output(output, "tmux display-message")?;
    if current.trim() == target {
        return Ok(());
    }

    let output = Command::new("tmux")
        .args(["set-option", "-t", session, "window-size", "manual"])
        .output()?;
    command_unit(output, "tmux set-option window-size")?;

    let width = width.to_string();
    let height = height.to_string();
    let output = Command::new("tmux")
        .args(["resize-window", "-t", session, "-x", &width, "-y", &height])
        .output()?;
    command_unit(output, "tmux resize-window")
}

pub fn attach(session: &str) -> Result<()> {
    let in_tmux = std::env::var_os("TMUX").is_some();
    if !in_tmux {
        let (width, height) = terminal::size()?;
        if width != 0 && height != 0 {
            resize_session(session, width, height)?;
        }
    }
    // Hand window sizing back to the attaching client. The preview path pins
    // `window-size` to `manual`; if we leave it pinned, the window stays at our
    // pre-attach guess (or the small preview size) and does not track the real
    // client. tmux then cannot reserve the row consumed by the status bar that
    // `install()` turns on, so the "C-q to return" line is drawn over the inner
    // program's bottom row (e.g. Claude's mode indicator). Letting the client
    // drive the size (as ClaudeSquad does) makes tmux lay out `pane = client - 1`
    // status row on attach, so nothing overlaps and no filler rows appear.
    let _ = set_session_option(session, "window-size", "latest");

    let binding = ReturnKeyBinding::install(in_tmux, session)?;
    let mut command = Command::new("tmux");
    if in_tmux {
        command.args(["switch-client", "-t", session]);
    } else {
        command.args(["attach", "-t", session]);
    }
    let status = command.status()?;
    let attach_result = if status.success() {
        if in_tmux {
            wait_for_return_key(session)
        } else {
            Ok(())
        }
    } else {
        Err(Error::Command {
            context: "tmux attach",
            stderr: format!("tmux exited with {status}"),
        })
    };
    let restore_result = binding.restore();
    if !has_session(session)? {
        return restore_result.map(|_| ());
    }
    attach_result.and(restore_result)
}

pub fn send_keys(session: &str, keys: &[&str]) -> Result<()> {
    let output = Command::new("tmux")
        .args(["send-keys", "-t", session])
        .args(keys)
        .output()?;
    command_unit(output, "tmux send-keys")
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

struct ReturnKeyBinding {
    session: String,
    previous: Option<String>,
    previous_status: Option<String>,
    previous_status_right: Option<String>,
}

impl ReturnKeyBinding {
    fn install(in_tmux: bool, session: &str) -> Result<Self> {
        let previous = capture_key_binding("C-q")?;
        let mut command = Command::new("tmux");
        if in_tmux {
            // One idempotent root binding that works at any nesting depth. The
            // return signal is derived from the session the key is pressed in
            // (`#{session_name}`, expanded by tmux at the press), so every nested
            // robco waits on its own session's signal. Because the binding text
            // is identical at every level, a deeper instance installing over a
            // shallower one cannot corrupt it, and `switch-client -l` returns to
            // the parent session the current client came from.
            command.args([
                "bind-key",
                "-T",
                "root",
                "C-q",
                "run-shell",
                "tmux switch-client -l ; tmux wait-for -S robco-return-#{session_name}",
            ]);
        } else {
            // Outside tmux the client returns when the blocking `tmux attach`
            // exits, so detaching is enough; no wait-for signal is needed.
            command.args(["bind-key", "-T", "root", "C-q", "detach-client"]);
        }
        let output = command.output()?;
        command_unit(output, "tmux bind-key")?;
        let (previous_status, previous_status_right) = match (|| {
            let previous_status = capture_session_option(session, "status")?;
            let previous_status_right = capture_session_option(session, "status-right")?;
            set_session_option(session, "status", "on")?;
            set_session_option(session, "status-right", "C-q to return")?;
            Ok((previous_status, previous_status_right))
        })() {
            Ok(previous) => previous,
            Err(err) => {
                let _ = restore_key_binding(previous.as_deref());
                return Err(err);
            }
        };
        Ok(Self {
            session: session.to_string(),
            previous,
            previous_status,
            previous_status_right,
        })
    }

    fn restore(self) -> Result<()> {
        let key_result = if has_server()? {
            restore_key_binding(self.previous.as_deref())
        } else {
            Ok(())
        };
        let status_result = restore_session_option_if_present(
            &self.session,
            "status",
            self.previous_status.as_deref(),
        );
        let status_right_result = restore_session_option_if_present(
            &self.session,
            "status-right",
            self.previous_status_right.as_deref(),
        );
        key_result.and(status_result.and(status_right_result))
    }
}

fn capture_key_binding(key: &str) -> Result<Option<String>> {
    let output = Command::new("tmux")
        .args(["list-keys", "-T", "root", key])
        .output()?;
    let binding = command_output(output, "tmux list-keys")?;
    let binding = binding.trim().to_string();
    Ok((!binding.is_empty()).then_some(binding))
}

fn restore_key_binding(previous: Option<&str>) -> Result<()> {
    match previous {
        Some(previous) => {
            let mut child = Command::new("tmux")
                .args(["source-file", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(previous.as_bytes())?;
                stdin.write_all(b"\n")?;
            }
            let output = child.wait_with_output()?;
            command_unit(output, "tmux restore key binding")
        }
        None => {
            let output = Command::new("tmux")
                .args(["unbind-key", "-T", "root", "C-q"])
                .output()?;
            command_unit(output, "tmux unbind-key")
        }
    }
}

fn capture_session_option(session: &str, option: &str) -> Result<Option<String>> {
    let output = Command::new("tmux")
        .args(["show-options", "-t", session, "-q", option])
        .output()?;
    let presence = command_output(output, "tmux show-options")?;
    if presence.is_empty() {
        return Ok(None);
    }

    let output = Command::new("tmux")
        .args(["show-options", "-t", session, "-q", "-v", option])
        .output()?;
    let value = command_output(output, "tmux show-options")?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    Ok(Some(value))
}

fn set_session_option(session: &str, option: &str, value: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["set-option", "-t", session, option, value])
        .output()?;
    command_unit(output, "tmux set-option")
}

fn unset_session_option(session: &str, option: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["set-option", "-u", "-t", session, option])
        .output()?;
    command_unit(output, "tmux set-option -u")
}

fn restore_session_option(session: &str, option: &str, previous: Option<&str>) -> Result<()> {
    match previous {
        Some(value) => set_session_option(session, option, value),
        None => unset_session_option(session, option),
    }
}

fn restore_session_option_if_present(
    session: &str,
    option: &str,
    previous: Option<&str>,
) -> Result<()> {
    if !has_session(session)? {
        return Ok(());
    }
    restore_session_option(session, option, previous)
}

fn wait_for_return_key(session: &str) -> Result<()> {
    let signal = return_signal_name(session);
    let mut child = Command::new("tmux").args(["wait-for", &signal]).spawn()?;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(Error::Command {
                context: "tmux wait-for",
                stderr: format!("tmux exited with {status}"),
            });
        }
        if !has_session(session)? {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn return_signal_name(session: &str) -> String {
    // Must match the signal emitted by the C-q root binding installed in
    // `ReturnKeyBinding::install` (`robco-return-#{session_name}`). The session
    // name robco creates is already sanitized, so this round-trips to the same
    // string tmux expands `#{session_name}` to.
    format!("robco-return-{}", sanitize_target_part(session))
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

    #[test]
    fn return_signal_name_is_tmux_safe() {
        let signal = return_signal_name("repo/foo.bar:baz");
        assert!(signal.starts_with("robco-return-"));
        assert!(signal.ends_with("-repo-foo-bar-baz"));
    }
}
