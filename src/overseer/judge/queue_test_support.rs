//! Reach-ins the judgment queue's own tests need.
//!
//! A child module of `queue`, so it can see the queue's private state without
//! that state having to be visible to the rest of the judge. Kept out of
//! `queue.rs` because scaffolding is not the state machine, and the state
//! machine is what a reader opens that file for.

use super::{
    super::keys::{dispatch_key, merge_key},
    super::result::{DispatchAdvice, MergeAdvice},
    JudgmentQueue, MergeCase, Parsed, Request,
};
use crate::overseer::dispatch::Candidate;

impl JudgmentQueue {
    pub(in crate::overseer::judge) fn is_active(&self) -> bool {
        !self.active.is_empty()
    }

    pub(in crate::overseer::judge) fn active_len(&self) -> usize {
        self.active.len()
    }

    pub(in crate::overseer::judge) fn set_llm_calls_today(&mut self, count: u32) {
        self.counter.set_today(count);
    }

    pub(in crate::overseer::judge) fn cache_merge(
        &mut self,
        case: &MergeCase,
        advice: MergeAdvice,
    ) {
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

    pub(in crate::overseer::judge) fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
pub(in crate::overseer::judge) fn test_queue(root: &std::path::Path) -> JudgmentQueue {
    JudgmentQueue::new(root.join("cases"), root.join("decisions.jsonl")).unwrap()
}
