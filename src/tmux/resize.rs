use crate::Result;

use super::{
    TmuxServer, command_output, command_unit,
    session::{exact, has_session},
};

pub fn resize_session(server: &TmuxServer, session: &str, width: u16, height: u16) -> Result<()> {
    // A missing target must be a no-op, not an error. `display-message` below
    // exits 0 and prints an empty `x` for a nonexistent session (observed on
    // tmux 3.7), so the `current == target` short-circuit never fires and the
    // `set-option window-size` call fails with `no such window`. That Err used
    // to bubble out of `attach` and terminate robco (dropping the ssh session)
    // whenever the user attached to a worktree whose AI session had exited.
    if !has_session(server, session)? {
        return Ok(());
    }
    let session = exact(session);
    let target = format!("{width}x{height}");
    let output = server
        .command()
        .args([
            "display-message",
            "-p",
            "-t",
            &session,
            "#{window_width}x#{window_height}:#{pane_height}",
        ])
        .output()?;
    let probe = command_output(output, "tmux display-message")?;
    let (window, pane_fills_window) = parse_size_probe(probe.trim()).unwrap_or(("", true));
    if !pane_fills_window {
        // A user-level `pane-border-status` (e.g. `top`) reserves a title row
        // inside the window even for a single pane, so the pane — and with it
        // every `capture-pane` — comes up one row short of the size set here,
        // leaving a blank line at the bottom of the preview. robco sessions
        // are always single-pane, so the border line only costs a row; drop
        // it and the pane expands to fill the window. Best-effort: sessions
        // robco created already have it off, this heals adopted, orphan, and
        // pre-fix sessions.
        let _ = server
            .command()
            .args([
                "set-window-option",
                "-t",
                &session,
                "pane-border-status",
                "off",
            ])
            .output();
    }
    if window == target {
        return Ok(());
    }

    let output = server
        .command()
        .args(["set-option", "-t", &session, "window-size", "manual"])
        .output()?;
    command_unit(output, "tmux set-option window-size")?;

    let width = width.to_string();
    let height = height.to_string();
    let output = server
        .command()
        .args(["resize-window", "-t", &session, "-x", &width, "-y", &height])
        .output()?;
    command_unit(output, "tmux resize-window")
}

/// Split the `WxH:P` output of the size probe into the window size (`WxH`)
/// and whether the pane fills the window's full height (`P == H`). `None`
/// for anything else, e.g. the empty string a vanished session prints.
fn parse_size_probe(probe: &str) -> Option<(&str, bool)> {
    let (window, pane_height) = probe.split_once(':')?;
    let (_, window_height) = window.split_once('x')?;
    Some((window, window_height == pane_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_probe_reports_full_height_pane() {
        assert_eq!(parse_size_probe("201x59:59"), Some(("201x59", true)));
    }

    #[test]
    fn size_probe_flags_pane_border_status_row() {
        // `pane-border-status` steals a row: the pane is one short of the window.
        assert_eq!(parse_size_probe("149x55:54"), Some(("149x55", false)));
    }

    #[test]
    fn size_probe_rejects_missing_session_output() {
        assert_eq!(parse_size_probe(""), None);
        assert_eq!(parse_size_probe("201x59"), None);
    }
}
