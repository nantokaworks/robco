//! The merge half of the judgment queue: asking for a verdict on one pull
//! request, and the terminal verdicts it must not keep re-asking.
//!
//! A child module of `queue`, so it reads the queue's private state directly.
//! Kept out of `queue.rs` because spawning, polling, and persisting a judgment
//! session is a different machine from the bookkeeping that decides whether a
//! pull request needs a verdict at all — and `queue.rs` is opened for the first.

use super::{
    super::keys::{merge_identity, merge_identity_parts, merge_key},
    super::result::{MergeAdvice, MergeJudgment, MergeVerdict},
    JudgmentQueue, MergeCase, Parsed, Request, audit,
};
use crate::{Result, overseer::logging::DecisionKind};

impl JudgmentQueue {
    pub fn merge_advice(&mut self, case: MergeCase) -> Result<MergeVerdict> {
        let identity = merge_identity(&case);
        let key = merge_key(&case);
        if self.terminal_merges.matches(&identity, &key) {
            return Ok(MergeVerdict::Refused);
        }
        // Anything still held for this pull request under another key answered a
        // version of the change that no longer exists.
        self.completed
            .discard_superseded_merges(&self.log_path, &identity, &key)?;
        if let Some(Parsed::Merge(advice)) = self.completed.take(&key) {
            self.remember(identity, &key, &advice)?;
            return Ok(MergeVerdict::Advice(advice));
        }
        let (task_id, repo) = (case.task_id.clone(), case.repo.clone());
        // Recorded on the transition into the queue only. The waiting pull
        // request is otherwise the one auto-merge outcome that writes nothing,
        // and several ticks of silence read as a dead auto-merge.
        if self.enqueue_once(Request::Merge { key, case }) {
            audit::log(
                &self.log_path,
                DecisionKind::Hold,
                Some(&task_id),
                Some(&repo),
                audit::MERGE_PENDING,
            )?;
        }
        Ok(MergeVerdict::Queued)
    }

    /// Records the verdicts that must not be asked for again at this fingerprint.
    ///
    /// A refusal is remembered, so the gate stops re-asking until the worker
    /// changes something. An approval is not: it has been acted on. Neither is a
    /// fail-safe — that is the judge's own session failing, and it says nothing
    /// about the change under review. Remembering one cached an expired auth
    /// token as a refusal of the diff, and left a green, mergeable pull request
    /// with no exit but a human.
    fn remember(&mut self, identity: String, key: &str, advice: &MergeAdvice) -> Result<()> {
        if advice.outcome == MergeJudgment::Allow || advice.fail_safe {
            return self.terminal_merges.clear(&self.revisions_path, &identity);
        }
        self.terminal_merges
            .remember(&self.revisions_path, identity, key.to_owned())
    }

    pub fn has_terminal_merge(&self, task_id: &str, pr_url: Option<&str>) -> bool {
        pr_url.is_some_and(|url| {
            self.terminal_merges
                .contains(&merge_identity_parts(task_id, url))
        })
    }

    /// Drops the terminal verdict a pull request received.
    ///
    /// A veto or an escalation is remembered so the merge gate keeps reconsidering
    /// the pull request until the worker changes something. Once the pull request
    /// itself has settled there is nothing left to reconsider — the change it was
    /// given for can never be merged again — and the remembered verdict would only
    /// keep pulling the entry back into the gate on every pass.
    pub fn forget_terminal_merge(&mut self, task_id: &str, pr_url: &str) -> Result<()> {
        self.terminal_merges
            .clear(&self.revisions_path, &merge_identity_parts(task_id, pr_url))
    }
}
