//! Whether the repository's own checkout is safe to run the release
//! pipeline against.

use std::{path::Path, process::Command};

use crate::{exec::run_timeout, overseer::exec::COMMAND_TIMEOUT};

/// Confirms the repository's own checkout is safe to run the pipeline
/// against. Returns the skip reason on the first guard that fails.
///
/// Fetches `origin/main` to learn the merge's commit — the same read-only
/// step `git::post_merge` takes — but never writes to the checkout itself:
/// an operator's own working tree, dirty or not, is never this module's to
/// change. A checkout behind or diverged from that commit is left exactly
/// as it is; the pipeline waits for whatever already keeps it current.
pub(super) fn ready(repo: &Path) -> std::result::Result<(), &'static str> {
    match crate::git::worktree_is_clean(repo) {
        Ok(true) => {}
        Ok(false) => return Err("working_tree_dirty"),
        Err(_) => return Err("working_tree_check_failed"),
    }
    // Checked ahead of the commit comparison below so a checkout left
    // detached or on another branch gets its own named reason — dropr:429 —
    // rather than falling into the generic "not on the merged commit" one,
    // which reads as "just behind" and hides the real cause.
    match crate::git::current_branch(repo) {
        Ok(Some(branch)) if branch == "main" => {}
        Ok(Some(_)) => return Err("checkout_not_on_main"),
        Ok(None) => return Err("checkout_detached"),
        Err(_) => return Err("checkout_branch_check_failed"),
    }
    let Ok(merged_commit) = crate::git::remote_branch_commit(repo, "main") else {
        return Err("checkout_not_on_merged_commit");
    };
    match head_commit(repo) {
        Some(head) if head == merged_commit => Ok(()),
        _ => Err("checkout_not_on_merged_commit"),
    }
}

fn head_commit(repo: &Path) -> Option<String> {
    let mut command = Command::new("git");
    command.args(["-C"]).arg(repo).args(["rev-parse", "HEAD"]);
    let output = run_timeout(command, COMMAND_TIMEOUT).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
