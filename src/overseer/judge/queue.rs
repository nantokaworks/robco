use super::{
    MergeCase, Request, audit, completion,
    keys::{dispatch_key, merge_identity, merge_identity_parts, merge_key},
    result::{DispatchAdvice, MergeAdvice, Parsed},
    revisions::RevisionCache,
    snapshot::QueueSnapshot,
    spawn_session,
};
use crate::{
    Result,
    config::Config,
    overseer::{daily::DailyCounter, logging::DecisionKind},
};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::mpsc::TryRecvError,
};

pub struct JudgmentQueue {
    pending: VecDeque<Request>,
    active: Option<(Request, crate::overseer::session::SessionHandle)>,
    completed: HashMap<String, Parsed>,
    terminal_merges: RevisionCache,
    root: PathBuf,
    log_path: PathBuf,
    counter_path: PathBuf,
    counter: DailyCounter,
    revisions_path: PathBuf,
    snapshot_path: PathBuf,
    snapshot: QueueSnapshot,
}

impl JudgmentQueue {
    pub fn load() -> Result<Self> {
        Self::new(
            super::super::judge_dir()?,
            super::super::decision_log_path()?,
        )
    }

    fn new(root: PathBuf, log_path: PathBuf) -> Result<Self> {
        let counter_path = root.join("queue.json");
        let counter = DailyCounter::load(&counter_path)?;
        let revisions_path = root.join("revisions.json");
        let terminal_merges = RevisionCache::load(&revisions_path)?;
        let snapshot_path = root.join("queue_state.json");
        let snapshot = QueueSnapshot::load(&snapshot_path)?;
        Ok(Self {
            pending: VecDeque::new(),
            active: None,
            completed: HashMap::new(),
            terminal_merges,
            root,
            log_path,
            counter_path,
            counter,
            revisions_path,
            snapshot_path,
            snapshot,
        })
    }

    /// Starts or polls at most one judgment and never waits for its model process.
    pub fn tick(&mut self, config: &Config) -> Result<()> {
        let advanced = self.advance(config);
        // Published after every pass, including the failing ones: a snapshot
        // left describing a judgment that is no longer running is exactly the
        // "stuck or queued?" ambiguity it exists to remove. A failed publish
        // never masks a failed pass — the pass is the more important error.
        advanced.and(self.publish_snapshot())
    }

    fn advance(&mut self, config: &Config) -> Result<()> {
        if self.active.is_none() {
            if let Some(request) = self.pending.pop_front() {
                if let Request::Dispatch { approved, .. } = &request
                    && self.counter.count_today() >= config.overseer.daily_llm_budget
                {
                    let parsed = Parsed::Dispatch(DispatchAdvice {
                        candidate_ids: approved.iter().map(|item| item.task_id.clone()).collect(),
                        reason: "daily_llm_budget".into(),
                        fail_safe: true,
                    });
                    audit::record(&self.log_path, &request, &parsed)?;
                    self.completed.insert(request.key().to_owned(), parsed);
                    return Ok(());
                }
                self.counter.increment(&self.counter_path)?;
                let handle = spawn_session(config, request.clone(), &self.root);
                self.active = Some((request, handle));
            }
            return Ok(());
        }
        let received = match self.active.as_ref().unwrap().1.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                crate::overseer::session::SessionResult::LaunchFailed(
                    "judgment session thread disconnected".into(),
                )
            }
        };
        let (request, _) = self.active.take().unwrap();
        let parsed = completion::normalize(received, &request);
        audit::record(&self.log_path, &request, &parsed)?;
        self.completed.insert(request.key().to_owned(), parsed);
        Ok(())
    }

    pub fn dispatch_advice(
        &mut self,
        approved: &[crate::overseer::dispatch::Candidate],
    ) -> Option<DispatchAdvice> {
        if approved.is_empty() {
            return Some(DispatchAdvice {
                candidate_ids: Vec::new(),
                reason: "no approved candidates".into(),
                fail_safe: false,
            });
        }
        let key = dispatch_key(approved);
        if let Some(Parsed::Dispatch(advice)) = self.completed.remove(&key) {
            return Some(advice);
        }
        self.enqueue_once(Request::Dispatch {
            key,
            approved: approved.to_vec(),
        });
        None
    }

    /// Drops dispatch judgments keyed to a candidate set that no longer exists,
    /// recording each discard.
    ///
    /// `dispatch_key` hashes the approved ids, so one task appearing or
    /// disappearing between a round being enqueued and its verdict arriving
    /// leaves that verdict stranded under a key nothing will ask for again.
    /// Dropping it silently is what made a whole round read as "the Overseer did
    /// nothing this pass". Every dispatch pass calls this, judged or not, so the
    /// reset is recorded even when the pass that supersedes the round never
    /// needed a judge.
    pub fn discard_stale_dispatch(
        &mut self,
        approved: &[crate::overseer::dispatch::Candidate],
    ) -> Result<()> {
        let current = dispatch_key(approved);
        let stale = self
            .completed
            .iter()
            .filter(|(key, parsed)| **key != current && matches!(parsed, Parsed::Dispatch(_)))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in stale {
            self.completed.remove(&key);
            audit::log(
                &self.log_path,
                DecisionKind::Skip,
                None,
                None,
                &format!("judgment_discarded:candidate_set_changed:{key}"),
            )?;
        }
        Ok(())
    }

    pub fn merge_advice(&mut self, case: MergeCase) -> Result<Option<MergeAdvice>> {
        let identity = merge_identity(&case);
        if self.terminal_merges.matches(&identity, &case.head_sha) {
            return Ok(None);
        }
        let key = merge_key(&case);
        if let Some(Parsed::Merge(advice)) = self.completed.remove(&key) {
            if advice.outcome == super::result::MergeJudgment::Allow {
                self.terminal_merges
                    .clear(&self.revisions_path, &identity)?;
            } else {
                self.terminal_merges.remember(
                    &self.revisions_path,
                    identity,
                    case.head_sha.clone(),
                )?;
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

    pub fn llm_calls_today(&self) -> u32 {
        self.counter.count_today()
    }

    /// Last published queue state, for readers outside the daemon process.
    pub fn snapshot(&self) -> &QueueSnapshot {
        &self.snapshot
    }

    pub fn has_terminal_merge(&self, task_id: &str, pr_url: Option<&str>) -> bool {
        pr_url.is_some_and(|url| {
            self.terminal_merges
                .contains(&merge_identity_parts(task_id, url))
        })
    }

    #[cfg(test)]
    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    #[cfg(test)]
    pub(super) fn set_llm_calls_today(&mut self, count: u32) {
        self.counter.set_today(count);
    }

    #[cfg(test)]
    pub(super) fn cache_merge(&mut self, case: &MergeCase, advice: MergeAdvice) {
        self.completed
            .insert(merge_key(case), Parsed::Merge(advice));
    }

    #[cfg(test)]
    pub(super) fn cache_dispatch(
        &mut self,
        approved: &[crate::overseer::dispatch::Candidate],
        advice: DispatchAdvice,
    ) {
        self.completed
            .insert(dispatch_key(approved), Parsed::Dispatch(advice));
    }

    #[cfg(test)]
    pub(super) fn completed_len(&self) -> usize {
        self.completed.len()
    }

    #[cfg(test)]
    pub(super) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Returns `true` when the request was newly queued rather than already
    /// waiting or already running.
    fn enqueue_once(&mut self, request: Request) -> bool {
        let key = request.key();
        let active = self.active.as_ref().is_some_and(|item| item.0.key() == key);
        if active || self.pending.iter().any(|item| item.key() == key) {
            return false;
        }
        self.pending.push_back(request);
        true
    }

    fn publish_snapshot(&mut self) -> Result<()> {
        let next = QueueSnapshot {
            active: self.active.as_ref().map(|item| item.0.label()),
            pending: self.pending.iter().map(Request::label).collect(),
        };
        if next != self.snapshot {
            next.save(&self.snapshot_path)?;
            self.snapshot = next;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn test_queue(root: &std::path::Path) -> JudgmentQueue {
    JudgmentQueue::new(root.join("cases"), root.join("decisions.jsonl")).unwrap()
}
