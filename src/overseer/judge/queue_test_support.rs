//! Reach-ins the judgment queue's tests need.
//!
//! A child module of `queue`, so it can see the queue's private state without
//! that state having to be visible to the rest of the judge. Kept out of
//! `queue.rs` because scaffolding is not the state machine, and the state
//! machine is what a reader opens that file for.
//!
//! The two a caller outside the judge needs — a queue rooted in a temp
//! directory, and a verdict seeded into it — are `pub(crate)`: the auto-merge
//! gate's own tests play out pass sequences against a real queue, and a stub
//! would let the two drift on exactly the question those tests exist to pin
//! down. Everything here is behind `cfg(test)`.

use super::{
    super::keys::{dispatch_key, merge_key},
    super::result::{DispatchAdvice, MergeAdvice},
    JudgmentQueue, MergeCase, Parsed, Request,
};
use crate::overseer::dispatch::Candidate;

impl JudgmentQueue {
    pub(in crate::overseer::judge) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(in crate::overseer::judge) fn set_llm_calls_today(&mut self, count: u32) {
        self.counter.set_today(count);
    }

    pub(crate) fn cache_merge(&mut self, case: &MergeCase, advice: MergeAdvice) {
        let request = Request::Merge {
            key: merge_key(case),
            case: case.clone(),
        };
        self.completed.insert(request, Parsed::Merge(advice));
    }

    pub(in crate::overseer::judge) fn cache_dispatch(
        &mut self,
        approved: &[Candidate],
        advice: DispatchAdvice,
    ) {
        let request = Request::Dispatch {
            key: dispatch_key(approved),
            approved: approved.to_vec(),
        };
        self.completed.insert(request, Parsed::Dispatch(advice));
    }

    pub(in crate::overseer::judge) fn completed_len(&self) -> usize {
        self.completed.len()
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
pub(crate) fn test_queue(root: &std::path::Path) -> JudgmentQueue {
    JudgmentQueue::new(root.join("cases"), root.join("decisions.jsonl")).unwrap()
}
