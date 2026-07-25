//! Why GitHub refused a `gh pr merge`, when the exit status alone does not say.
//!
//! One refusal is worth naming: `--rebase` against a head branch that carries a
//! merge commit. GitHub still reports the pull request as mergeable and clean —
//! `--squash` and `--merge` both land it — and refuses the rebase alone, so the
//! raw `gh` output reads as "robco is broken" rather than "this branch cannot be
//! replayed onto the base". Relaying that failure verbatim leaves the operator
//! with an exit status and no next move.
//!
//! Naming the cause is all this does. Quietly retrying under another strategy
//! would hide the very divergence the single `merge_strategy` setting removes.

use std::process::Output;

use crate::config::MergeStrategy;

/// A merge failure whose cause robco can state in the operator's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeRefusal {
    /// Compact token for the daemon's hold reason and `decisions.jsonl`.
    pub reason: &'static str,
    /// The sentence an operator reads on the TUI banner.
    pub message: &'static str,
}

/// How GitHub words a head branch it cannot replay onto the base. Both spellings
/// are matched because the wording has varied between the REST message `gh`
/// relays and `gh`'s own phrasing of it.
const CANNOT_REBASE: [&str; 2] = ["can't be rebased", "cannot be rebased"];

/// Names the cause of a failed `gh pr merge`, or `None` when the output carries
/// no cause worth restating.
pub fn explain_merge_failure(strategy: MergeStrategy, output: &str) -> Option<MergeRefusal> {
    if strategy != MergeStrategy::Rebase {
        return None;
    }
    let output = output.to_ascii_lowercase();
    CANNOT_REBASE
        .iter()
        .any(|marker| output.contains(marker))
        .then_some(MergeRefusal {
            reason: "rebase_refused_merge_commit",
            message: "the head branch contains a merge commit, so GitHub cannot replay it onto \
                      the base; squash or merge this pull request instead, or rebuild the branch \
                      without the merge commit",
        })
}

/// What a failed command actually said. `gh` reports refusals on stderr, but
/// falls back to stdout for some failures, and an empty detail is the one
/// message that tells the operator nothing at all.
pub fn command_failure_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use std::process::ExitStatus;

    use super::*;

    fn output(stdout: &str, stderr: &str) -> Output {
        Output {
            status: ExitStatus::default(),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_rebase_refused_for_a_merge_commit_is_named() {
        let refusal = explain_merge_failure(
            MergeStrategy::Rebase,
            "X Pull request #193 is not mergeable: This branch can't be rebased.",
        )
        .expect("the refusal is recognised");
        assert_eq!(refusal.reason, "rebase_refused_merge_commit");
        assert!(refusal.message.contains("merge commit"));
    }

    #[test]
    fn the_alternate_wording_is_recognised_too() {
        assert!(
            explain_merge_failure(MergeStrategy::Rebase, "This branch cannot be rebased").is_some()
        );
    }

    /// Only the rebase path can hit this refusal, so a squash or merge failure
    /// is never dressed up as one.
    #[test]
    fn other_strategies_are_never_explained_as_a_rebase_refusal() {
        let stderr = "This branch can't be rebased";
        assert!(explain_merge_failure(MergeStrategy::Squash, stderr).is_none());
        assert!(explain_merge_failure(MergeStrategy::Merge, stderr).is_none());
    }

    /// An unrelated rebase failure keeps its own output rather than being
    /// mislabelled — the cause named here has to be the cause that happened.
    #[test]
    fn an_unrelated_failure_is_left_alone() {
        assert!(
            explain_merge_failure(MergeStrategy::Rebase, "GraphQL: Resource not accessible")
                .is_none()
        );
    }

    #[test]
    fn failure_text_prefers_stderr_and_falls_back_to_stdout() {
        assert_eq!(command_failure_text(&output("out", "err\n")), "err");
        assert_eq!(command_failure_text(&output(" out \n", "  ")), "out");
        assert_eq!(command_failure_text(&output("", "")), "");
    }
}
