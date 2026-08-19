//! Opens a pull request without a worker session, for [`super::pr_precheck`]
//! to fall back to when the branch's agent session has already ended. The
//! agent, when it is still around, writes a better body than this template
//! can — this only runs once that option is gone.

use std::path::Path;

use crate::git;

/// The five sections every robco pull request body must carry, plus the
/// `Close Dropr: #N` line the merge webhook needs to close `display_id` on
/// merge. `display_id` is already in `#N` form (see
/// `super::lifecycle::App::task_display_id`).
pub(super) fn pr_body(display_id: &str) -> String {
    format!(
        "## Summary\n\
         Opened by robco because the agent session for this branch had already ended.\n\n\
         ## Changes\n\
         See the commits on this branch for the full diff.\n\n\
         ## Test Plan\n\
         Not run by robco. Check this branch's own commits and this pull request's CI checks.\n\n\
         ## Impact\n\
         See the commits on this branch for scope.\n\n\
         ## Checklist\n\
         - [ ] CI is green\n\
         - [ ] Reviewed by a human before merge\n\n\
         Close Dropr: {display_id}"
    )
}

/// The same precondition `merge_selected` checks before merging: robco is
/// about to act on the branch's own behalf, so an uncommitted change would be
/// silently left out of the pull request it opens.
fn require_clean_worktree(clean: bool) -> std::result::Result<(), &'static str> {
    clean
        .then_some(())
        .ok_or("commit or clean untracked changes before opening a pull request")
}

/// Opens the pull request. `title` is the agent's task title, `display_id` is
/// its dropr task id (`#N`) — both already resolved by the caller, since
/// neither is available to a background thread without a live `App`.
pub(super) fn create_fallback_pr(
    repo_path: &Path,
    branch: &str,
    worktree_path: &Path,
    title: &str,
    display_id: &str,
) -> std::result::Result<String, String> {
    let clean = git::worktree_is_clean(worktree_path).map_err(|error| error.to_string())?;
    require_clean_worktree(clean).map_err(str::to_string)?;
    let body = pr_body(display_id);
    git::create_pr(repo_path, branch, title, &body).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_carries_the_five_sections_and_the_close_directive() {
        let body = pr_body("#477");
        for heading in [
            "## Summary",
            "## Changes",
            "## Test Plan",
            "## Impact",
            "## Checklist",
        ] {
            assert!(body.contains(heading), "missing {heading}");
        }
        assert!(body.contains("Close Dropr: #477"));
    }

    #[test]
    fn a_dirty_worktree_is_refused() {
        assert_eq!(
            require_clean_worktree(false),
            Err("commit or clean untracked changes before opening a pull request")
        );
        assert!(require_clean_worktree(true).is_ok());
    }
}
