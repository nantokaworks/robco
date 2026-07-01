use std::{path::Path, process::Command};

use crate::Result;

use super::{command_output, command_unit};

pub fn session_name(prefix: &str, repo: &str, agent: &str) -> String {
    format!(
        "{prefix}{}_{}",
        sanitize_target_part(repo),
        sanitize_target_part(agent)
    )
}

/// Anchor a session name so tmux matches it exactly, for any target kind.
///
/// tmux resolves a `-t <name>` target by exact match first, then falls back to
/// an `fnmatch(3)` pattern or a name prefix. Our shell session is named
/// `<ai>-shell`, which has the AI session `<ai>` as a prefix, so an un-anchored
/// target for `<ai>` bleeds into `<ai>-shell` whenever `<ai>` itself does not
/// exist (e.g. the main worktree AI was never launched but the user started a
/// program in TERM). The `=` prefix forces an exact-name match and prevents the
/// AI tab from mirroring the TERM session.
///
/// The trailing `:` is load-bearing. `=<session>` alone resolves only for pure
/// *session*-target commands (`has-session`, `kill-session`, session
/// `set-option` / `show-options`). For *pane*- and *window*-target commands the
/// bare `=<session>` is broken on tmux 3.7: `capture-pane` and `send-keys` fail
/// with `can't find pane`, `set-option window-size` fails with `no such window`,
/// and `display-message '#{window_width}...'` exits 0 but prints an empty string
/// (this is what actually made the #50 resize path fail — it misfires for live
/// sessions, not just missing ones). Appending `:` selects the session's default
/// window/pane, which resolves for every target kind while still matching the
/// session name exactly, so the prefix-bleed guard above is preserved.
pub(super) fn exact(session: &str) -> String {
    format!("={session}:")
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
        .args(["has-session", "-t", &exact(session)])
        .output()?;
    Ok(output.status.success())
}

pub(super) fn has_server() -> Result<bool> {
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
        .args([
            "set-window-option",
            "-t",
            &exact(session),
            "monitor-activity",
            "on",
        ])
        .output();
    Ok(())
}

pub fn kill_session(session: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["kill-session", "-t", &exact(session)])
        .output()?;
    command_unit(output, "tmux kill-session")
}

pub fn resize_session(session: &str, width: u16, height: u16) -> Result<()> {
    // A missing target must be a no-op, not an error. `display-message` below
    // exits 0 and prints an empty `x` for a nonexistent session (observed on
    // tmux 3.7), so the `current == target` short-circuit never fires and the
    // `set-option window-size` call fails with `no such window`. That Err used
    // to bubble out of `attach` and terminate robco (dropping the ssh session)
    // whenever the user attached to a worktree whose AI session had exited.
    if !has_session(session)? {
        return Ok(());
    }
    let session = exact(session);
    let target = format!("{width}x{height}");
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            &session,
            "#{window_width}x#{window_height}",
        ])
        .output()?;
    let current = command_output(output, "tmux display-message")?;
    if current.trim() == target {
        return Ok(());
    }

    let output = Command::new("tmux")
        .args(["set-option", "-t", &session, "window-size", "manual"])
        .output()?;
    command_unit(output, "tmux set-option window-size")?;

    let width = width.to_string();
    let height = height.to_string();
    let output = Command::new("tmux")
        .args(["resize-window", "-t", &session, "-x", &width, "-y", &height])
        .output()?;
    command_unit(output, "tmux resize-window")
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
    fn exact_target_anchors_session_and_default_pane() {
        // `=` forces an exact session match (no prefix bleed into `<name>-shell`)
        // and the trailing `:` selects the default window/pane so the target
        // resolves for pane/window commands too (capture-pane, send-keys,
        // set-option window-size) — not just session-only commands.
        assert_eq!(exact("robco_repo_agent"), "=robco_repo_agent:");
    }
}
