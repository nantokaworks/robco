use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use super::{command::escalate_workers, ledger::Ledger, runtime_requests_dir};
use crate::{Result, config::Config};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RuntimeRequest {
    ResetCircuit {
        source: String,
        at: DateTime<Utc>,
    },
    PanicEscalate {
        source: String,
        agent_ids: Vec<String>,
        at: DateTime<Utc>,
    },
}

pub(crate) fn enqueue(request: RuntimeRequest) -> Result<()> {
    enqueue_in(&runtime_requests_dir()?, request)
}

pub(crate) fn enqueue_in(dir: &Path, request: RuntimeRequest) -> Result<()> {
    fs::create_dir_all(dir)?;
    let request_id = nanoid!();
    let path = dir.join(format!("{request_id}.json"));
    let temp_path = dir.join(format!("{request_id}.json.{}.tmp", nanoid!()));
    let raw = serde_json::to_string_pretty(&request)?;
    let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, &path));
    if let Err(error) = written {
        let _ = fs::remove_file(temp_path);
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn drain(ledger: &mut Ledger, config: &mut Config) -> Result<bool> {
    drain_in(&runtime_requests_dir()?, ledger, config)
}

pub(crate) fn drain_in(dir: &Path, ledger: &mut Ledger, config: &mut Config) -> Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    // Requests apply in filename (nanoid) order, which is NOT chronological.
    // Every RuntimeRequest variant must therefore be commutative and idempotent
    // (ResetCircuit zeroes the streak + reaffirms dispatch; PanicEscalate escalates
    // named workers) so drain order never changes the outcome. Preserve that
    // invariant when adding new variants.
    paths.sort();

    let mut config_changed = false;
    for path in paths {
        // A single unreadable/unremovable request must not abort the whole drain
        // (that would discard the accumulated config change and, worse, leave an
        // applied ResetCircuit file to replay every tick — silently re-zeroing the
        // failure streak and defeating the circuit). Handle each file resiliently.
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!(
                    "warning: overseer runtime request {} unreadable, skipped this tick: {error}",
                    path.display()
                );
                continue;
            }
        };
        let request = match serde_json::from_str(&raw) {
            Ok(request) => request,
            Err(error) => {
                quarantine_corrupt(&path, &error);
                continue;
            }
        };
        config_changed |= apply(ledger, config, request);
        // Ack by removing the file. If removal fails, move it aside so an already
        // applied request cannot be replayed on the next tick.
        if let Err(error) = fs::remove_file(&path) {
            quarantine_applied(&path, &error);
        }
    }
    Ok(config_changed)
}

pub(crate) fn apply(ledger: &mut Ledger, config: &mut Config, request: RuntimeRequest) -> bool {
    match request {
        RuntimeRequest::ResetCircuit { .. } => {
            ledger.counters.consecutive_failures = 0;
            if config.overseer.dispatch_enabled {
                false
            } else {
                config.overseer.dispatch_enabled = true;
                true
            }
        }
        RuntimeRequest::PanicEscalate { agent_ids, .. } => {
            escalate_workers(ledger, &agent_ids.into_iter().collect::<HashSet<_>>());
            false
        }
    }
}

fn quarantine_applied(path: &Path, error: &std::io::Error) {
    let aside = path.with_extension("json.applied");
    if fs::rename(path, &aside).is_err() {
        let _ = fs::remove_file(path);
    }
    eprintln!(
        "warning: overseer runtime request {} applied but could not be removed ({error}); moved aside to avoid replay",
        path.display()
    );
}

fn quarantine_corrupt(path: &Path, error: &serde_json::Error) {
    let corrupt_path = corrupt_path(path);
    if fs::rename(path, &corrupt_path).is_err() {
        let _ = fs::remove_file(path);
    }
    eprintln!(
        "warning: corrupt overseer runtime request {} skipped and moved aside: {error}",
        path.display()
    );
}

fn corrupt_path(path: &Path) -> PathBuf {
    path.with_extension("json.corrupt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::ledger::{LedgerEntry, LedgerPhase};

    #[test]
    fn enqueue_then_drain_applies_and_acks() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("requests");
        let mut ledger = Ledger::default();
        ledger.counters.consecutive_failures = 99;
        let mut config = Config::default();
        config.overseer.dispatch_enabled = false;
        enqueue_in(
            &dir,
            RuntimeRequest::ResetCircuit {
                source: "test".into(),
                at: Utc::now(),
            },
        )
        .unwrap();

        assert!(drain_in(&dir, &mut ledger, &mut config).unwrap());
        assert_eq!(ledger.counters.consecutive_failures, 0);
        assert!(config.overseer.dispatch_enabled);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
        assert_eq!(ledger.counters.consecutive_failures, 0);
    }

    #[test]
    fn panic_escalate_marks_workers_including_pr_opened() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("requests");
        let mut ledger = Ledger {
            entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
            ..Ledger::default()
        };
        let mut config = Config::default();
        enqueue_in(
            &dir,
            RuntimeRequest::PanicEscalate {
                source: "test".into(),
                agent_ids: vec!["worker-1".into()],
                at: Utc::now(),
            },
        )
        .unwrap();

        assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
        assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
    }

    #[test]
    fn drain_missing_dir_is_noop() {
        let temp = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        let mut config = Config::default();

        assert!(!drain_in(&temp.path().join("missing"), &mut ledger, &mut config).unwrap());
    }

    #[test]
    fn corrupt_file_is_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("requests");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bad.json"), "not json").unwrap();
        let mut ledger = Ledger::default();
        let mut config = Config::default();

        assert!(!drain_in(&dir, &mut ledger, &mut config).unwrap());
        assert!(!dir.join("bad.json").exists());
        assert!(dir.join("bad.json.corrupt").exists());
    }

    #[test]
    fn drain_applies_and_acks_all_pending_requests() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("requests");
        let mut ledger = Ledger {
            entries: vec![ledger_entry("worker-1", LedgerPhase::PrOpened)],
            ..Ledger::default()
        };
        ledger.counters.consecutive_failures = 7;
        let mut config = Config::default();
        config.overseer.dispatch_enabled = false;
        enqueue_in(
            &dir,
            RuntimeRequest::PanicEscalate {
                source: "test".into(),
                agent_ids: vec!["worker-1".into()],
                at: Utc::now(),
            },
        )
        .unwrap();
        enqueue_in(
            &dir,
            RuntimeRequest::ResetCircuit {
                source: "test".into(),
                at: Utc::now(),
            },
        )
        .unwrap();

        assert!(drain_in(&dir, &mut ledger, &mut config).unwrap());
        assert_eq!(ledger.counters.consecutive_failures, 0);
        assert!(config.overseer.dispatch_enabled);
        assert_eq!(ledger.entries[0].phase, LedgerPhase::Escalated);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    fn ledger_entry(agent_id: &str, phase: LedgerPhase) -> LedgerEntry {
        LedgerEntry {
            task_id: format!("task-{agent_id}"),
            display_id: "#202".into(),
            repo: "nantokaworks/robco".into(),
            agent_id: agent_id.into(),
            branch: "task-202".into(),
            phase,
            dispatched_at: Utc::now(),
            retries: 0,
            pr_url: Some("https://example.test/pr/202".into()),
            branch_updates: 0,
        }
    }
}
