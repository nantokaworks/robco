use std::collections::HashMap;

use crate::Result;

use super::TmuxServer;

/// Active-pane pid and current foreground command for every live tmux
/// session, keyed by session name.
///
/// A session key missing from the snapshot means the session does not
/// exist — the same signal a per-session `tmux::has_session` would give as
/// `Ok(false)`. That is a different situation from [`capture_panes`] itself
/// returning `Err`: callers that need to tell "definitely gone" apart from
/// "could not check this tick" must look at the `Result`, not just at
/// whether a key is present.
#[derive(Debug, Clone, Default)]
pub struct PaneSnapshot {
    panes: HashMap<String, (Option<u32>, Option<String>)>,
}

impl PaneSnapshot {
    pub fn contains(&self, session: &str) -> bool {
        self.panes.contains_key(session)
    }

    pub fn pane_pid(&self, session: &str) -> Option<u32> {
        self.panes.get(session).and_then(|(pid, _)| *pid)
    }

    pub fn pane_current_command(&self, session: &str) -> Option<&str> {
        self.panes.get(session).and_then(|(_, cmd)| cmd.as_deref())
    }
}

/// One batched probe for every live tmux session's active-pane pid and
/// current command, replacing the per-session `has-session` /
/// `display-message #{pane_pid}` / `display-message #{pane_current_command}`
/// spawns on the status hot path with a single `list-panes -a` call.
///
/// The `#{&&:#{window_active},#{pane_active}}` filter keeps only each
/// session's currently active window's active pane — `#{pane_active}` alone
/// is scoped per window, so a session with several windows would otherwise
/// yield one row per window. robco sessions are always single-window,
/// single-pane, so this is exactly one row per session.
///
/// A missing tmux server (or any non-zero exit from `list-panes`) yields an
/// empty snapshot, mirroring [`super::list_sessions_with_cwd`]. A
/// process-level failure to run `tmux` at all propagates as `Err`, which
/// callers treat the same way as a per-session probe `Err(_)`: keep the
/// previous status and retry next tick, rather than reading every session as
/// gone.
pub fn capture_panes(server: &TmuxServer) -> Result<PaneSnapshot> {
    let output = server
        .command()
        .args([
            "list-panes",
            "-a",
            "-f",
            "#{&&:#{window_active},#{pane_active}}",
            "-F",
            "#{session_name}|#{pane_pid}|#{pane_current_command}",
        ])
        .output()?;
    Ok(parse_panes(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
    ))
}

fn parse_panes(success: bool, raw: &str) -> PaneSnapshot {
    if !success {
        return PaneSnapshot::default();
    }
    let mut panes = HashMap::new();
    for line in raw.lines() {
        let mut fields = line.splitn(3, '|');
        let (Some(session), Some(pid), Some(command)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // A multi-window session would repeat the name if the active-pane
        // filter above ever failed to collapse it to one row; keep the first
        // row seen, mirroring `list_sessions_with_cwd`.
        panes.entry(session.to_string()).or_insert_with(|| {
            let pane_pid = pid.trim().parse().ok();
            let pane_current_command =
                (!command.trim().is_empty()).then(|| command.trim().to_string());
            (pane_pid, pane_current_command)
        });
    }
    PaneSnapshot { panes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_panes_reads_one_row_per_session() {
        let snapshot = parse_panes(true, "sess-a|111|zsh\nsess-b|222|cargo\n");

        assert!(snapshot.contains("sess-a"));
        assert_eq!(snapshot.pane_pid("sess-a"), Some(111));
        assert_eq!(snapshot.pane_current_command("sess-a"), Some("zsh"));
        assert_eq!(snapshot.pane_pid("sess-b"), Some(222));
        assert_eq!(snapshot.pane_current_command("sess-b"), Some("cargo"));
        assert!(!snapshot.contains("sess-c"));
    }

    #[test]
    fn parse_panes_keeps_first_row_for_a_repeated_session_name() {
        // In practice the `window_active && pane_active` filter already
        // collapses a multi-window session to one row; this guards the
        // parser itself against ever seeing a duplicate.
        let snapshot = parse_panes(true, "sess-a|111|zsh\nsess-a|999|vim\n");

        assert_eq!(snapshot.pane_pid("sess-a"), Some(111));
        assert_eq!(snapshot.pane_current_command("sess-a"), Some("zsh"));
    }

    #[test]
    fn parse_panes_empty_server_yields_empty_snapshot() {
        let snapshot = parse_panes(true, "");

        assert!(!snapshot.contains("sess-a"));
        assert_eq!(snapshot.pane_pid("sess-a"), None);
    }

    #[test]
    fn parse_panes_failed_call_yields_empty_snapshot() {
        let snapshot = parse_panes(false, "sess-a|111|zsh\n");

        assert!(!snapshot.contains("sess-a"));
    }
}
