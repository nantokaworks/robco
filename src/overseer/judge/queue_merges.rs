//! The merge half of the judgment queue: asking for a verdict on one pull
//! request, and the terminal verdicts it must not keep re-asking.
//!
//! A child module of `queue`, so it reads the queue's private state directly.
//! Kept out of `queue.rs` because spawning, polling, and persisting a judgment
//! session is a different machine from the bookkeeping that decides whether a
//! pull request needs a verdict at all — and `queue.rs` is opened for the first.

use super::{
    super::keys::{merge_identity, merge_identity_parts, merge_key},
    super::result::{MergeAdvice, MergeJudgment},
    JudgmentQueue, MergeCase, Parsed, Request, audit,
};
use crate::{Result, overseer::logging::DecisionKind};

impl JudgmentQueue {
    pub fn merge_advice(&mut self, case: MergeCase) -> Result<Option<MergeAdvice>> {
        let identity = merge_identity(&case);
        let key = merge_key(&case);
        if self.terminal_merges.matches(&identity, &key) {
            return Ok(None);
        }
        // Anything still held for this pull request under another key answered a
        // version of the change that no longer exists.
        self.completed
            .discard_superseded_merges(&self.log_path, &identity, &key)?;
        if let Some(Parsed::Merge(advice)) = self.completed.take(&key) {
            match (advice.fail_safe, advice.outcome) {
                // A fail-safe verdict is the judge session itself failing, not
                // a judgment about the change — caching it as terminal would
                // park the pull request on an answer that was never about the
                // diff. Leaving nothing remembered is what lets the next pass
                // re-ask the judge instead; `merge_judge_fail_safe` bounds how
                // many times it may.
                (true, _) => {}
                (false, MergeJudgment::Allow) => {
                    self.terminal_merges
                        .clear(&self.revisions_path, &identity)?;
                }
                (false, _) => {
                    self.terminal_merges
                        .remember(&self.revisions_path, identity, key.clone())?;
                }
            }
            return Ok(Some(advice));
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
        Ok(None)
    }

    /// Queues a judgment for a change the merge gate has not cleared yet, so the
    /// session runs while the pull request is still waiting on its checks.
    ///
    /// Enqueue-only on purpose. Unlike [`Self::merge_advice`] this never takes a
    /// completed verdict and never writes `terminal_merges`, because nothing here
    /// is asking what the judge said: the gate is still the authority on whether
    /// this pull request may merge, and it reads the answer through `merge_advice`
    /// on the pass it is ready to act on one. Consuming the verdict here would
    /// drop it before that pass ever saw it.
    ///
    /// The change is fingerprinted by [`merge_key`], which is deliberately blind
    /// to the head sha — so priming a pull request, updating its branch, and
    /// priming it again all name the same question, and the second call finds the
    /// first still queued rather than paying for the judgment twice.
    ///
    /// Returns whether this call actually started a new judgment, so the caller
    /// can charge its own budget for judgments bought rather than for calls made.
    pub fn prime_merge(&mut self, case: MergeCase) -> Result<bool> {
        let identity = merge_identity(&case);
        let key = merge_key(&case);
        // A verdict this change already has — terminal, or waiting to be read —
        // is the one `merge_advice` will hand back. Asking again buys nothing.
        if self.terminal_merges.matches(&identity, &key) || self.completed.holds(&key) {
            return Ok(false);
        }
        let (task_id, repo) = (case.task_id.clone(), case.repo.clone());
        if !self.enqueue_once(Request::Merge { key, case }) {
            return Ok(false);
        }
        audit::log(
            &self.log_path,
            DecisionKind::Hold,
            Some(&task_id),
            Some(&repo),
            audit::MERGE_PENDING,
        )?;
        Ok(true)
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
