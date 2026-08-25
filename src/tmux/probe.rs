//! Reading a just-launched pane's liveness and location back from tmux
//! (dropr:554), for [`super::launch::verify_launch`] to act on.

use std::path::PathBuf;

use super::{TmuxServer, session::exact};

/// What a just-launched pane looked like on one probe.
pub(super) enum PaneProbe {
    /// Still running, in the given `pane_current_path`.
    Alive(PathBuf),
    /// The launched program already exited; `remain-on-exit` (set atomically
    /// as part of creation — see `launch::verified_new_session_command`)
    /// kept the pane around so the tap it started can still be read.
    Dead,
    /// No session answered at all. `exit-empty off` and `remain-on-exit on`
    /// are both set in the very same tmux command that created the session
    /// (see `launch::verified_new_session_command`), so this should not
    /// happen in practice — it is kept as a fallback for a probe that could
    /// not reach the server at all, not a case this code expects to hit
    /// routinely.
    Gone,
}

/// One `display-message -p` query for a single format variable.
///
/// Two separate calls rather than one `"#{pane_dead}\t#{pane_current_path}"`
/// query: a literal control character (the tab meant to separate the two
/// fields) embedded in a `display-message` format string is not portable —
/// some tmux builds pass it through as-is, others sanitize it to `_` before
/// printing, which silently corrupted the split this used to do on exactly
/// the builds this whole check exists to support. A plain query has no
/// separator to mangle.
fn display_message(server: &TmuxServer, session: &str, format: &str) -> Option<String> {
    let output = server
        .command()
        .args(["display-message", "-p", "-t", &exact(session), format])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_string(),
    )
}

pub(super) fn probe_pane(server: &TmuxServer, session: &str) -> PaneProbe {
    let Some(dead) = display_message(server, session, "#{pane_dead}") else {
        return PaneProbe::Gone;
    };
    if dead == "1" {
        return PaneProbe::Dead;
    }
    match display_message(server, session, "#{pane_current_path}") {
        Some(path) if !path.is_empty() => PaneProbe::Alive(PathBuf::from(path)),
        // Either this second query failed outright, or came back empty: the
        // OS-level cwd lookup needs a live process to query, so a pane whose
        // process exits between the two probes above can read back empty (or
        // fail to resolve at all) for one round before `pane_dead` itself
        // would have caught it. Both read the same as a death observed a
        // beat late, not as a session that vanished outright — `remain-on-exit`
        // (already active by the time this runs) keeps the pane object
        // around for exactly this case.
        _ => PaneProbe::Dead,
    }
}
