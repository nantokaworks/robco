//! The pre-flight checks a PR request runs, shared by the TUI's confirm dialog
//! and the `robco_pr_request` MCP tool.
//!
//! PR authoring belongs to the worker agent: robco never runs `gh pr create`
//! itself, it only sends the configured prompt into the worker's session. These
//! two checks are what stop it sending that prompt into a session that is gone,
//! or asking for a second pull request on a branch that already has one open.
//!
//! Both checks shell out, so both callers run them away from their event loop —
//! the TUI on a worker thread, the MCP server on the request's own thread.

use std::path::Path;

/// Refusals are plain strings because both callers show them to a reader
/// verbatim: the TUI on its message line, the MCP tool as the error body.
pub fn precheck(repo: &Path, branch: &str, tmux_session: &str) -> std::result::Result<(), String> {
    let running = crate::tmux::has_session(tmux_session).map_err(|error| error.to_string())?;
    require_running(running)?;
    let exists = crate::git::pr_exists(repo, branch).map_err(|error| error.to_string())?;
    require_no_open_pr(branch, exists)
}

/// A prompt sent to a session that is gone goes nowhere, and the caller would
/// otherwise be told the request succeeded.
fn require_running(running: bool) -> std::result::Result<(), String> {
    running
        .then_some(())
        .ok_or_else(|| "agent session is not running".to_string())
}

/// Asking again for a pull request the branch already has does not produce a
/// second one — it produces a worker redoing work that is already up for review.
fn require_no_open_pr(branch: &str, exists: bool) -> std::result::Result<(), String> {
    if exists {
        return Err(format!("PR already open for {branch}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_session_is_refused_and_a_running_one_passes() {
        assert_eq!(
            require_running(false).unwrap_err(),
            "agent session is not running"
        );
        assert!(require_running(true).is_ok());
    }

    /// The branch is named in the refusal because a controller driving several
    /// workers has to know which one it just asked about.
    #[test]
    fn an_existing_pull_request_is_refused_by_name() {
        assert_eq!(
            require_no_open_pr("feature/one", true).unwrap_err(),
            "PR already open for feature/one"
        );
        assert!(require_no_open_pr("feature/one", false).is_ok());
    }
}
