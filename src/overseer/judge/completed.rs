//! Verdicts that have come back and have not been consumed yet.
//!
//! A verdict is filed under the exact question it answered, so it is only ever
//! handed to a caller asking that same question again. The other half of that
//! rule is this module's real job: when the question changes, the verdict left
//! behind has to be dropped *and recorded*. Dropping it silently is what made a
//! whole round read as "the Overseer did nothing this pass" — the model ran, the
//! answer arrived, and nothing in `decisions.jsonl` said where it went.
//!
//! The request is kept beside its verdict rather than the key alone, because
//! deciding whether a verdict has been superseded means asking what it was
//! about: which candidate set, or which pull request.

use std::{collections::HashMap, path::Path};

use super::{
    Request, audit,
    keys::{dispatch_key, merge_identity},
    result::Parsed,
};
use crate::{Result, overseer::dispatch::Candidate, overseer::logging::DecisionKind};

#[derive(Default)]
pub(super) struct CompletedJudgments(HashMap<String, (Request, Parsed)>);

impl CompletedJudgments {
    pub(super) fn insert(&mut self, request: Request, parsed: Parsed) {
        self.0.insert(request.key().to_owned(), (request, parsed));
    }

    pub(super) fn take(&mut self, key: &str) -> Option<Parsed> {
        self.0.remove(key).map(|(_, parsed)| parsed)
    }

    /// Drops dispatch verdicts keyed to a candidate set that no longer exists.
    ///
    /// `dispatch_key` hashes the approved ids, so one task appearing or
    /// disappearing between a round being enqueued and its verdict arriving
    /// leaves that verdict under a key nobody will ask for again.
    pub(super) fn discard_stale_dispatch(
        &mut self,
        log_path: &Path,
        approved: &[Candidate],
    ) -> Result<()> {
        let current = dispatch_key(approved);
        let stale = self
            .0
            .iter()
            .filter(|(key, (request, _))| {
                **key != current && matches!(request, Request::Dispatch { .. })
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in stale {
            self.0.remove(&key);
            audit::log(
                log_path,
                DecisionKind::Skip,
                None,
                None,
                &format!("judgment_discarded:candidate_set_changed:{key}"),
            )?;
        }
        Ok(())
    }

    /// Drops merge verdicts for `identity` that answered an earlier version of
    /// the change.
    ///
    /// The dispatch path has had this sweep since a stranded verdict was first
    /// mistaken for an idle Overseer; the merge path never did, so a pull
    /// request whose diff changed left its old verdict in memory for the life of
    /// the daemon. A mechanical base update no longer lands here — the
    /// fingerprint is unchanged, so the key is the same one being asked for —
    /// which is what makes the remaining discards genuinely superseded work.
    pub(super) fn discard_superseded_merges(
        &mut self,
        log_path: &Path,
        identity: &str,
        current: &str,
    ) -> Result<()> {
        let stale = self
            .0
            .iter()
            .filter_map(|(key, (request, _))| match request {
                Request::Merge { case, .. }
                    if key != current && merge_identity(case) == identity =>
                {
                    Some((key.clone(), case.task_id.clone(), case.repo.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (key, task_id, repo) in stale {
            self.0.remove(&key);
            audit::log(
                log_path,
                DecisionKind::Skip,
                Some(&task_id),
                Some(&repo),
                &format!("judgment_discarded:change_superseded:{key}"),
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.0.len()
    }
}
