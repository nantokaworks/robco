use super::{
    MergeCase, Request, audit,
    completed::CompletedJudgments,
    completion,
    keys::dispatch_key,
    pending::PendingQueue,
    result::{self, DispatchAdvice, Parsed},
    revisions::RevisionCache,
    snapshot::QueueSnapshot,
    spawn_session,
};
use crate::{Result, config::Config, overseer::daily::DailyCounter};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::mpsc::TryRecvError,
};

pub struct JudgmentQueue {
    pending: VecDeque<Request>,
    /// Judgments currently running, keyed by [`Request::slot`]. A `BTreeMap`
    /// rather than a `HashMap` so the order `persist_pending` and
    /// `publish_snapshot` see is deterministic across ticks instead of
    /// shuffling with the hasher's mood.
    active: BTreeMap<String, (Request, crate::overseer::session::SessionHandle)>,
    completed: CompletedJudgments,
    terminal_merges: RevisionCache,
    root: PathBuf,
    log_path: PathBuf,
    counter_path: PathBuf,
    counter: DailyCounter,
    revisions_path: PathBuf,
    snapshot_path: PathBuf,
    snapshot: QueueSnapshot,
    pending_path: PathBuf,
    persisted: PendingQueue,
    /// Keys restored from the durable queue at start-up, and so the only ones
    /// whose case directory may already hold an answer this process did not
    /// write. Each is consumed the first time its request is served — a round
    /// this daemon asks itself is always answered by a session, never by a file
    /// left over from an earlier one.
    recoverable: HashSet<String>,
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
        let pending_path = root.join("pending.json");
        // Everything durable comes back pending, including whatever was active:
        // its session process died with the daemon that started it.
        let persisted = PendingQueue::load(&pending_path)?;
        let recoverable = persisted
            .requests
            .iter()
            .map(|request| request.key().to_owned())
            .collect();
        Ok(Self {
            pending: persisted.requests.iter().cloned().collect(),
            active: BTreeMap::new(),
            completed: CompletedJudgments::default(),
            terminal_merges,
            root,
            log_path,
            counter_path,
            counter,
            revisions_path,
            snapshot_path,
            snapshot,
            pending_path,
            persisted,
            recoverable,
        })
    }

    /// Polls every running judgment and starts new ones for open slots, up to
    /// the configured concurrency ceiling, and never waits for a model process.
    pub fn tick(&mut self, config: &Config) -> Result<()> {
        let advanced = self.advance(config);
        // Published after every pass, including the failing ones: a snapshot
        // left describing a judgment that is no longer running is exactly the
        // "stuck or queued?" ambiguity it exists to remove. A failed publish
        // never masks a failed pass — the pass is the more important error.
        advanced
            .and(self.persist_pending())
            .and(self.publish_snapshot())
    }

    fn advance(&mut self, config: &Config) -> Result<()> {
        for (request, received) in self.poll_active() {
            let parsed = completion::normalize(received, &request);
            audit::record(&self.log_path, &request, &parsed)?;
            self.completed.insert(request, parsed);
        }
        self.fill_slots(config)
    }

    /// Drains every active judgment that has finished, without waiting on any
    /// that has not.
    fn poll_active(&mut self) -> Vec<(Request, crate::overseer::session::SessionResult)> {
        let mut finished = Vec::new();
        self.active.retain(|_, (request, handle)| match handle.try_recv() {
            Ok(result) => {
                finished.push((request.clone(), result));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                finished.push((
                    request.clone(),
                    crate::overseer::session::SessionResult::LaunchFailed(
                        "judgment session thread disconnected".into(),
                    ),
                ));
                false
            }
        });
        finished
    }

    /// Starts judgments for as many open slots as the pending queue and the
    /// concurrency ceiling allow, in service order.
    ///
    /// A slot is open when nothing with the same [`Request::slot`] is already
    /// active; the ceiling bounds the total regardless of how many slots are
    /// open, since each judgment is a spawned agent CLI process. Every
    /// iteration either starts a session or resolves a request without one
    /// (daily budget exhausted, or a stored verdict recovered after a
    /// restart), and both remove the request from `pending` — so the loop
    /// always terminates.
    fn fill_slots(&mut self, config: &Config) -> Result<()> {
        let ceiling = config.overseer.max_concurrent_judges.max(1);
        while self.active.len() < ceiling {
            let Some(index) = self
                .pending
                .iter()
                .position(|request| !self.active.contains_key(&request.slot()))
            else {
                break;
            };
            let request = self.pending.remove(index).expect("index from position");
            if let Request::Dispatch { approved, .. } = &request
                && self.counter.count_today() >= config.overseer.daily_llm_budget
            {
                let parsed = Parsed::Dispatch(DispatchAdvice {
                    candidate_ids: approved.iter().map(|item| item.task_id.clone()).collect(),
                    reason: "daily_llm_budget".into(),
                    fail_safe: true,
                    ignored_fields: Vec::new(),
                });
                audit::record(&self.log_path, &request, &parsed)?;
                self.completed.insert(request, parsed);
                continue;
            }
            if self.recoverable.remove(request.key())
                && let Some(parsed) = self.stored_verdict(&request)
            {
                audit::record(&self.log_path, &request, &parsed)?;
                self.completed.insert(request, parsed);
                continue;
            }
            self.counter.increment(&self.counter_path)?;
            let slot = request.slot();
            let handle = spawn_session(config, request.clone(), &self.root);
            self.active.insert(slot, (request, handle));
        }
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
                ignored_fields: Vec::new(),
            });
        }
        let key = dispatch_key(approved);
        if let Some(Parsed::Dispatch(advice)) = self.completed.take(&key) {
            return Some(advice);
        }
        self.enqueue_once(Request::Dispatch {
            key,
            approved: approved.to_vec(),
        });
        None
    }

    /// Drops dispatch judgments keyed to a candidate set that no longer exists,
    /// recording each discard. Every dispatch pass calls this, judged or not, so
    /// the reset is recorded even when the pass that supersedes the round never
    /// needed a judge.
    pub fn discard_stale_dispatch(
        &mut self,
        approved: &[crate::overseer::dispatch::Candidate],
    ) -> Result<()> {
        self.completed
            .discard_stale_dispatch(&self.log_path, approved)
    }

    pub fn llm_calls_today(&self) -> u32 {
        self.counter.count_today()
    }

    /// Last published queue state, for readers outside the daemon process.
    pub fn snapshot(&self) -> &QueueSnapshot {
        &self.snapshot
    }

    /// Returns `true` when the request was newly queued rather than already
    /// waiting or already running.
    fn enqueue_once(&mut self, request: Request) -> bool {
        let key = request.key();
        let active = self.active.values().any(|item| item.0.key() == key);
        if active || self.pending.iter().any(|item| item.key() == key) {
            return false;
        }
        self.pending.push_back(request);
        true
    }

    /// A verdict the case directory already holds, if any.
    ///
    /// A restart orphans the session that was running but not the answer it may
    /// already have written, and a key names the exact question that was asked —
    /// so a `result.json` sitting in the case directory is an answer to *this*
    /// request. Re-running the session would spend model budget on a verdict
    /// already bought.
    ///
    /// Only ever consulted for a request restored from the durable queue. A
    /// round this process asked itself must reach a session: reusing the file it
    /// wrote a moment ago would freeze the judge's answer for as long as the
    /// question kept recurring.
    ///
    /// A half-written file is not a verdict: `is_complete` is the same
    /// whole-JSON check the session loop polls with, and anything it rejects
    /// falls through to a fresh run. Anything it accepts goes through
    /// `completion::normalize`, so a stored result that is complete but
    /// unusable fails exactly as a fresh one would.
    fn stored_verdict(&self, request: &Request) -> Option<Parsed> {
        let raw = fs::read(self.root.join(request.key()).join("result.json")).ok()?;
        result::is_complete(&raw).then(|| {
            completion::normalize(
                crate::overseer::session::SessionResult::Result(raw),
                request,
            )
        })
    }

    fn persist_pending(&mut self) -> Result<()> {
        let next = PendingQueue::capture(self.active.values().map(|item| &item.0), &self.pending);
        if next != self.persisted {
            next.save(&self.pending_path)?;
            self.persisted = next;
        }
        Ok(())
    }

    fn publish_snapshot(&mut self) -> Result<()> {
        let next = QueueSnapshot {
            active: self.active.values().map(|item| item.0.label()).collect(),
            pending: self.pending.iter().map(Request::label).collect(),
        };
        if next != self.snapshot {
            next.save(&self.snapshot_path)?;
            self.snapshot = next;
        }
        Ok(())
    }
}

#[path = "queue_merges.rs"]
mod merges;

#[cfg(test)]
#[path = "queue_test_support.rs"]
mod test_support;
#[cfg(test)]
pub(super) use test_support::test_queue;
