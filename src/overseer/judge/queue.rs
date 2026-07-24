use super::{
    MergeCase, Request, completion,
    keys::{dispatch_key, merge_identity, merge_identity_parts, merge_key},
    result::{DispatchAdvice, MergeAdvice, Parsed},
    revisions::RevisionCache,
    spawn_session,
};
use crate::{
    Result,
    config::Config,
    overseer::{
        daily::DailyCounter,
        logging::{self, DecisionEntry, DecisionKind},
    },
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
        })
    }

    /// Starts or polls at most one judgment and never waits for its model process.
    pub fn tick(&mut self, config: &Config) -> Result<()> {
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
                    self.audit(&request, &parsed)?;
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
        self.audit(&request, &parsed)?;
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
            self.log(
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
        self.enqueue_once(Request::Merge { key, case });
        Ok(None)
    }

    pub fn llm_calls_today(&self) -> u32 {
        self.counter.count_today()
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

    fn enqueue_once(&mut self, request: Request) {
        let key = request.key();
        let active = self.active.as_ref().is_some_and(|item| item.0.key() == key);
        if !active && !self.pending.iter().any(|item| item.key() == key) {
            self.pending.push_back(request);
        }
    }

    fn audit(&self, request: &Request, parsed: &Parsed) -> Result<()> {
        match (request, parsed) {
            (Request::Dispatch { approved, .. }, Parsed::Dispatch(advice)) => {
                for candidate in approved {
                    let selected = advice.candidate_ids.contains(&candidate.task_id);
                    let kind = if advice.fail_safe {
                        DecisionKind::Hold
                    } else if selected {
                        DecisionKind::Dispatch
                    } else {
                        DecisionKind::Skip
                    };
                    self.log(
                        kind,
                        Some(&candidate.task_id),
                        Some(&candidate.repo),
                        &advice.reason,
                    )?;
                }
            }
            (Request::Merge { case, .. }, Parsed::Merge(advice)) => {
                let kind = match advice.outcome {
                    super::result::MergeJudgment::Allow => DecisionKind::Merge,
                    super::result::MergeJudgment::Veto => DecisionKind::Escalate,
                    super::result::MergeJudgment::Escalate => DecisionKind::Escalate,
                };
                self.log(kind, Some(&case.task_id), Some(&case.repo), &advice.reason)?;
            }
            _ => unreachable!("request and judgment type must match"),
        }
        Ok(())
    }

    fn log(
        &self,
        kind: DecisionKind,
        task: Option<&str>,
        repo: Option<&str>,
        reason: &str,
    ) -> Result<()> {
        let mut entry = DecisionEntry::new(kind, reason);
        entry.task = task.map(str::to_owned);
        entry.repo = repo.map(str::to_owned);
        entry.source = Some("judge".into());
        logging::append_to(&self.log_path, &entry)
    }
}

#[cfg(test)]
pub(super) fn test_queue(root: &std::path::Path) -> JudgmentQueue {
    JudgmentQueue::new(root.join("cases"), root.join("decisions.jsonl")).unwrap()
}
