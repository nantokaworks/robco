//! The merge sequence robco runs for one worker: merge the pull request, run
//! the post-merge cleanup, kill the worker's sessions, and wake the Overseer.
//!
//! It lives here rather than in the TUI because two callers run it — the merge
//! key and the `robco_merge` MCP tool — and a merge issued from either has to
//! leave the repository in the same state. The only thing the callers supply
//! differently is what they do with the progress steps: the TUI renders them on
//! a banner, the MCP tool collects them into its result.
//!
//! The sequence still races across concurrent robco processes merging the
//! same repository at once, so it runs under [`with_merge_lock`], which
//! serialises it across processes. It never touches the repository's own
//! working tree — see [`crate::git::post_merge`] — so an operator's checkout
//! is not part of what it races on.

use std::path::Path;

use crate::{
    Result,
    config::MergeStrategy,
    git::{
        self,
        merge_lock::with_merge_lock,
        post_merge::{Cleanup, CleanupStep, OnFailure},
    },
    overseer::runtime_request::{self, RuntimeRequest},
};

pub const MERGING_PR: &str = "merging PR";
pub const PULLING_MAIN: &str = "pulling main";
pub const CLEANING_UP: &str = "cleaning up";

/// Whether the merge itself is still outstanding. A pull request that already
/// merged needs everything the sequence does *after* `gh pr merge` and must
/// never be handed to `gh pr merge` again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    MergeThenClean,
    CleanOnly,
}

impl MergeMode {
    /// The step the run starts on, so a progress banner does not open on
    /// "merging PR" for a run that never merges.
    pub fn first_step(self) -> &'static str {
        match self {
            Self::MergeThenClean => MERGING_PR,
            Self::CleanOnly => PULLING_MAIN,
        }
    }
}

pub struct MergeFlow<'a> {
    pub repo: &'a Path,
    pub branch: &'a str,
    pub worktree: &'a Path,
    pub tmux_session: &'a str,
    pub shell_session: &'a str,
    pub mode: MergeMode,
    pub strategy: MergeStrategy,
    /// Names the caller in the `MergeCompleted` runtime request, so the daemon
    /// log says which surface issued the merge it is reconciling.
    pub source: &'a str,
}

impl MergeFlow<'_> {
    /// Runs the sequence, naming each step to `step` as it starts. The last step
    /// named before an error is the step that failed, which is how a caller
    /// reports *where* a merge stopped without inspecting the error text.
    pub fn run(&self, step: impl FnMut(&'static str)) -> Result<()> {
        with_merge_lock(self.repo, || self.run_locked(step))
    }

    fn run_locked(&self, mut step: impl FnMut(&'static str)) -> Result<()> {
        if self.mode == MergeMode::MergeThenClean {
            step(MERGING_PR);
            git::merge_pr(self.repo, self.branch, self.strategy)?;
        }
        // A watched merge stops at the first failure so the caller sees it; the
        // Overseer daemon runs the same steps under `Continue`.
        Cleanup {
            repo: self.repo,
            worktree: self.worktree,
            branch: self.branch,
            on_failure: OnFailure::Abort,
        }
        .run(|cleanup_step| step(step_label(cleanup_step)))?;
        let server = crate::tmux::TmuxServer::default_server();
        let _ = crate::tmux::kill_session(&server, self.tmux_session);
        let _ = crate::tmux::kill_session(&server, self.shell_session);
        self.announce_merge();
        Ok(())
    }

    /// Tell the Overseer daemon this repository just merged, so it reconciles
    /// the merge on a pass that starts now instead of rediscovering it up to a
    /// poll interval later. Announced after cleanup rather than straight after
    /// the merge: the daemon cleans merged workers up too, and waking it
    /// mid-cleanup would have both processes removing the same worktree and
    /// killing the same sessions. Best effort — failing to announce only costs
    /// the delay this removes, and a caller cannot act on the failure. A cleanup
    /// that fails aborts before this point, so that merge is left for the daemon
    /// to find by polling.
    fn announce_merge(&self) {
        let _ = runtime_request::enqueue(RuntimeRequest::MergeCompleted {
            source: self.source.to_string(),
            repo: self.repo.display().to_string(),
            at: chrono::Utc::now(),
        });
    }
}

fn step_label(step: CleanupStep) -> &'static str {
    match step {
        CleanupStep::PullingMain => PULLING_MAIN,
        CleanupStep::CleaningUp => CLEANING_UP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_only_run_opens_on_the_first_post_merge_step() {
        assert_eq!(MergeMode::MergeThenClean.first_step(), MERGING_PR);
        assert_eq!(MergeMode::CleanOnly.first_step(), PULLING_MAIN);
    }

    #[test]
    fn cleanup_steps_keep_their_progress_labels() {
        assert_eq!(step_label(CleanupStep::PullingMain), PULLING_MAIN);
        assert_eq!(step_label(CleanupStep::CleaningUp), CLEANING_UP);
    }

    /// The whole point of naming steps through the callback: a caller that only
    /// records them still knows which one was running when the sequence failed.
    /// A clean-only run over a directory that is not a repository fails on its
    /// first step, and must report exactly that one — and no later one, because
    /// `OnFailure::Abort` stops there.
    #[test]
    fn the_sequence_stops_at_the_failing_step_and_names_it() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let mut steps = Vec::new();
        let flow = MergeFlow {
            repo: &repo,
            branch: "feature",
            worktree: &temp.path().join("worktree"),
            tmux_session: "robco-feature",
            shell_session: "robco-feature-shell",
            mode: MergeMode::CleanOnly,
            strategy: MergeStrategy::default(),
            source: "test",
        };

        let result = flow.run_locked(|step| steps.push(step));

        assert!(result.is_err());
        assert_eq!(steps, [PULLING_MAIN]);
    }
}
