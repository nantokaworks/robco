use super::{ExceptionCase, case_from, completion::complete_session_result, spawn_session};
use crate::{
    Result,
    chief::{
        ledger::Ledger,
        logging::{self, DecisionEntry, DecisionKind},
        monitor::{Action, Observations},
        session::{SessionHandle, SessionResult},
    },
    config::Config,
};
use nanoid::nanoid;
use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::mpsc::TryRecvError,
};

pub struct ExceptionQueue {
    pending: VecDeque<ExceptionCase>,
    seen: HashSet<String>,
    active: Option<SessionHandle>,
    completion_ready: bool,
    queue_path: PathBuf,
    root: PathBuf,
    log_path: PathBuf,
}

impl ExceptionQueue {
    pub fn load() -> Result<Self> {
        let root = super::super::triage_dir()?;
        let log_path = super::super::decision_log_path()?;
        Self::load_from(root.join("queue.json"), root, log_path)
    }

    fn load_from(queue_path: PathBuf, root: PathBuf, log_path: PathBuf) -> Result<Self> {
        let pending: VecDeque<ExceptionCase> = match fs::read(&queue_path) {
            Ok(raw) => match serde_json::from_slice(&raw) {
                Ok(pending) => pending,
                Err(error) => {
                    recover_corrupt_queue(&queue_path, &log_path, &error.to_string());
                    VecDeque::new()
                }
            },
            Err(error) if error.kind() == ErrorKind::NotFound => VecDeque::new(),
            Err(error) => {
                log_queue_recovery(&log_path, &error.to_string());
                VecDeque::new()
            }
        };
        let seen = pending.iter().map(case_key).collect();
        Ok(Self {
            pending,
            seen,
            active: None,
            completion_ready: false,
            queue_path,
            root,
            log_path,
        })
    }

    pub fn enqueue(
        &mut self,
        actions: &[Action],
        ledger: &Ledger,
        observed: &Observations,
    ) -> Result<()> {
        let mut changed = false;
        for action in actions {
            let (task_id, kind, reason) = match action {
                Action::MarkFailed { task_id, reason } => (task_id, "worker_failed", reason),
                Action::Escalate { task_id, reason } => (task_id, "worker_escalated", reason),
                _ => continue,
            };
            let Some(entry) = ledger
                .entries
                .iter()
                .find(|entry| &entry.task_id == task_id)
            else {
                continue;
            };
            let case = case_from(entry, kind, reason, observed);
            if self.seen.insert(case_key(&case)) {
                self.pending.push_back(case);
                changed = true;
            }
        }
        if changed { self.save() } else { Ok(()) }
    }

    /// Starts or polls at most one case and never waits for its LLM process.
    pub fn tick(&mut self, config: &Config, ledger: &mut Ledger) -> Result<()> {
        if self.completion_ready {
            return Ok(());
        }
        if self.active.is_none() {
            if let Some(case) = self.pending.front() {
                let case_dir = self.root.join(&case.id);
                if case_dir.join("outcome.json").exists() {
                    complete_session_result(
                        SessionResult::Missing,
                        ledger,
                        case,
                        &case_dir,
                        &self.log_path,
                    )?;
                    self.completion_ready = true;
                } else {
                    self.active = Some(spawn_session(config, case, &self.root));
                }
            }
            return Ok(());
        }
        let result = match self.active.as_ref().unwrap().try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                SessionResult::LaunchFailed("triage session thread disconnected".into())
            }
        };
        self.active = None;
        let case = self.pending.front().expect("active triage case").clone();
        complete_session_result(
            result,
            ledger,
            &case,
            &self.root.join(&case.id),
            &self.log_path,
        )?;
        self.completion_ready = true;
        Ok(())
    }

    pub fn acknowledge_completion(&mut self) -> Result<()> {
        if !self.completion_ready {
            return Ok(());
        }
        self.pending.pop_front();
        self.completion_ready = false;
        self.save()
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.queue_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = self
            .queue_path
            .with_extension(format!("json.{}.tmp", nanoid!()));
        let raw = serde_json::to_vec_pretty(&self.pending)?;
        if let Err(error) = fs::write(&temp, raw).and_then(|()| fs::rename(&temp, &self.queue_path))
        {
            let _ = fs::remove_file(temp);
            return Err(error.into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

fn recover_corrupt_queue(path: &std::path::Path, log_path: &std::path::Path, reason: &str) {
    let corrupt = path.with_extension("json.corrupt");
    let _ = fs::remove_file(&corrupt);
    let _ = fs::rename(path, corrupt);
    log_queue_recovery(log_path, reason);
}

fn log_queue_recovery(path: &std::path::Path, reason: &str) {
    let mut entry = DecisionEntry::new(
        DecisionKind::Hold,
        format!("triage queue unreadable; starting empty: {reason}"),
    );
    entry.source = Some("triage".into());
    let _ = logging::append_to(path, &entry);
}

fn case_key(case: &ExceptionCase) -> String {
    format!("{}:{}", case.task_id, case.kind)
}

#[cfg(test)]
pub(super) fn test_queue(root: &std::path::Path) -> Result<ExceptionQueue> {
    ExceptionQueue::load_from(
        root.join("queue.json"),
        root.join("cases"),
        root.join("decisions.jsonl"),
    )
}
