use std::{path::Path, process::Command};

use crate::{
    Result,
    config::{ENV_AGENT_ID, ENV_PARENT_AGENT_ID},
};

use super::{TmuxServer, command_unit, env::color_env_mirror, launch::INHERITED_IDENTITY_KEYS};

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

/// Every live tmux session paired with the cwd of its first listed pane.
/// A missing tmux server is an empty list, not an error.
pub fn list_sessions_with_cwd(server: &TmuxServer) -> Result<Vec<(String, std::path::PathBuf)>> {
    let output = server
        .command()
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{pane_current_path}",
        ])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();
    for line in raw.lines() {
        let Some((name, cwd)) = line.split_once('\t') else {
            continue;
        };
        // Multi-pane sessions repeat the name; keep the first pane's cwd.
        if sessions.iter().any(|(existing, _)| existing == name) {
            continue;
        }
        sessions.push((name.to_string(), std::path::PathBuf::from(cwd)));
    }
    Ok(sessions)
}

/// A live AI session (prefix-matching, not a `-shell` companion) whose pane
/// cwd is `cwd`. Lets adoption rebind to a surviving session even when its
/// name does not match the derived one.
pub fn find_session_by_cwd(server: &TmuxServer, prefix: &str, cwd: &Path) -> Option<String> {
    let target = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    list_sessions_with_cwd(server)
        .ok()?
        .into_iter()
        .find(|(name, path)| {
            // Canonicalize the pane path too: either side may spell the same
            // directory through a symlink (macOS `/tmp` → `/private/tmp`), and
            // a missed match here spawns a duplicate session over a live chat.
            name.starts_with(prefix)
                && !name.ends_with("-shell")
                && path.canonicalize().unwrap_or_else(|_| path.clone()) == target
        })
        .map(|(name, _)| name)
}

pub fn has_session(server: &TmuxServer, session: &str) -> Result<bool> {
    let output = server
        .command()
        .args(["has-session", "-t", &exact(session)])
        .output()?;
    Ok(output.status.success())
}

pub(super) fn has_server(server: &TmuxServer) -> Result<bool> {
    let output = server.command().arg("list-sessions").output()?;
    Ok(output.status.success())
}

/// Whether a `tmux` binary can be run at all, for tests that need a real
/// session and would rather skip than fail on a runner that has none —
/// GitHub's hosted `macos-latest` image does not ship tmux, unlike its
/// `ubuntu-latest` image.
#[cfg(test)]
pub(crate) fn is_installed() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

/// Build a detached tmux session command with session-scoped environment.
///
/// Environment injection uses `new-session -e`, which requires tmux >= 3.2.
/// Missing robco identity keys are assigned empty values so the new session
/// cannot inherit stale identities from the tmux server's global environment.
/// Missing color keys are unset in the command shell rather than with `env -u`
/// so shell builtins and every command in a compound program see the change.
pub fn new_session_command(
    server: &TmuxServer,
    session: &str,
    cwd: &Path,
    program: &str,
    envs: &[(&str, String)],
) -> Command {
    new_session_command_with_lookup(server, session, cwd, program, envs, |key| {
        std::env::var_os(key).map(|value| value.to_string_lossy().into_owned())
    })
}

pub(super) fn new_session_command_with_lookup(
    server: &TmuxServer,
    session: &str,
    cwd: &Path,
    program: &str,
    envs: &[(&str, String)],
    lookup: impl Fn(&str) -> Option<String>,
) -> Command {
    let mut command = server.command();
    command
        .args(["new-session", "-d", "-s", session, "-c"])
        .arg(cwd);
    for key in [ENV_AGENT_ID, ENV_PARENT_AGENT_ID]
        .into_iter()
        .chain(INHERITED_IDENTITY_KEYS)
    {
        if !envs.iter().any(|(candidate, _)| *candidate == key) {
            command.arg("-e").arg(format!("{key}="));
        }
    }
    let (mirror, unset) = color_env_mirror(lookup);
    for (key, value) in mirror {
        if !envs.iter().any(|(candidate, _)| *candidate == key) {
            command.arg("-e").arg(format!("{key}={value}"));
        }
    }
    for (key, value) in envs {
        command.arg("-e").arg(format!("{key}={value}"));
    }
    let unset = unset
        .into_iter()
        .filter(|key| !envs.iter().any(|(candidate, _)| candidate == key))
        .collect::<Vec<_>>();
    if unset.is_empty() {
        command.arg(program);
    } else {
        let keys = unset.join(" ");
        command.arg(format!("unset {keys}; {program}"));
    }
    command
}

pub fn new_session(
    server: &TmuxServer,
    session: &str,
    cwd: &Path,
    program: &str,
    envs: &[(&str, String)],
) -> Result<()> {
    let output = new_session_command(server, session, cwd, program, envs).output()?;
    command_unit(output, "tmux new-session")?;
    apply_cosmetic_options(server, session);
    Ok(())
}

/// The preview-rendering window options every robco session gets, regardless
/// of how it was created. Best-effort throughout: none of these affect
/// whether the session itself works, only how its preview renders, so a
/// failure here costs a cosmetic — never the session.
pub(super) fn apply_cosmetic_options(server: &TmuxServer, session: &str) {
    let _ = server
        .command()
        .args([
            "set-window-option",
            "-t",
            &exact(session),
            "monitor-activity",
            "on",
        ])
        .output();
    // Alternate-screen apps (Claude Code's TUI among them) keep their output
    // in the alt buffer, which has no scrollback — the preview could never
    // scroll them back. Denying the alt screen routes their output through the
    // normal buffer, whose history `capture_scrollback` can walk.
    let _ = server
        .command()
        .args([
            "set-window-option",
            "-t",
            &exact(session),
            "alternate-screen",
            "off",
        ])
        .output();
    // A user-level `pane-border-status` (e.g. `top`) reserves a title row
    // inside the window even for a single pane, so the pane runs one row
    // shorter than the size the preview asks for and its mirror shows a blank
    // bottom line. robco sessions are always single-pane, so the border line
    // only costs a row — drop it.
    let _ = server
        .command()
        .args([
            "set-window-option",
            "-t",
            &exact(session),
            "pane-border-status",
            "off",
        ])
        .output();
}

pub fn kill_session(server: &TmuxServer, session: &str) -> Result<()> {
    let output = server
        .command()
        .args(["kill-session", "-t", &exact(session)])
        .output()?;
    command_unit(output, "tmux kill-session")
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
