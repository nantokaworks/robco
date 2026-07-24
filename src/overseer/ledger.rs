use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};

use chrono::{DateTime, NaiveDate, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerEntry {
    pub task_id: String,
    pub display_id: String,
    pub repo: String,
    pub agent_id: String,
    pub branch: String,
    pub phase: LedgerPhase,
    pub dispatched_at: DateTime<Utc>,
    pub retries: u32,
    pub pr_url: Option<String>,
    /// Times the auto-merge pass has updated this pull request's branch onto its base
    /// because it had fallen behind. Bounded by `overseer.max_branch_updates`, so a
    /// branch that keeps losing the race against other merges escalates instead of
    /// looping. Defaulted so ledgers written before the field existed still load.
    #[serde(default)]
    pub branch_updates: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerPhase {
    Dispatched,
    Claimed,
    Working,
    PrOpened,
    Merged,
    Failed,
    Escalated,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LedgerCounters {
    pub date: Option<NaiveDate>,
    pub dispatched_today: u32,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
    pub skip_list: Vec<String>,
    pub counters: LedgerCounters,
}

/// Live workers counted globally and per repository.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct ActiveWorkers {
    pub count: usize,
    pub repos: BTreeMap<String, usize>,
}

impl LedgerPhase {
    /// The phase's name as every reader spells it: the CLI status line, the
    /// OVERSEER frame, the review digest, and the serialized ledger. They must
    /// agree, or the same board reads differently depending on where you look.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dispatched => "dispatched",
            Self::Claimed => "claimed",
            Self::Working => "working",
            Self::PrOpened => "pr_opened",
            Self::Merged => "merged",
            Self::Failed => "failed",
            Self::Escalated => "escalated",
        }
    }
}

/// A phase no worker can leave: the entry no longer holds anything.
pub fn terminal(phase: LedgerPhase) -> bool {
    matches!(
        phase,
        LedgerPhase::Merged | LedgerPhase::Failed | LedgerPhase::Escalated
    )
}

impl Ledger {
    /// The workers occupying capacity right now. The dispatch gate and
    /// `robco overseer status` both read this one helper, so the count that
    /// enforces `max_workers` / `per_repo_limit` is the count the operator sees.
    ///
    /// Management mode is deliberately not a filter. Manual suppresses Overseer
    /// *intervention* — the worker belongs to a human, so it is never killed,
    /// restarted, or re-dispatched — but it still holds a worktree, a branch, a
    /// tmux session, and CPU in its repository. Exempting it from the caps would
    /// let a mode toggle free a slot the resources never released.
    pub fn active_workers(&self) -> ActiveWorkers {
        let mut repos: BTreeMap<String, usize> = BTreeMap::new();
        let mut count = 0;
        for entry in self.entries.iter().filter(|entry| !terminal(entry.phase)) {
            count += 1;
            *repos.entry(entry.repo.clone()).or_default() += 1;
        }
        ActiveWorkers { count, repos }
    }

    pub fn load() -> Result<Self> {
        let path = super::ledger_path()?;
        Self::load_from(&path)
    }

    pub fn save(&self) -> Result<()> {
        let path = super::ledger_path()?;
        Self::save_to(self, &path)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let raw = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_str(&raw) {
            Ok(ledger) => Ok(ledger),
            Err(error) => {
                let corrupt_path = path.with_extension("json.corrupt");
                fs::rename(path, &corrupt_path)?;
                eprintln!(
                    "warning: corrupt overseer ledger {} moved to {}; using defaults: {error}",
                    path.display(),
                    corrupt_path.display()
                );
                Ok(Self::default())
            }
        }
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension(format!("json.{}.tmp", nanoid!()));
        let written = fs::write(&temp_path, raw).and_then(|()| fs::rename(&temp_path, path));
        if let Err(error) = written {
            let _ = fs::remove_file(temp_path);
            return Err(error.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("overseer/ledger.json");
        let ledger = Ledger {
            entries: vec![LedgerEntry {
                task_id: "task-1".into(),
                display_id: "#128".into(),
                repo: "nantokaworks/robco".into(),
                agent_id: "worker-1".into(),
                branch: "task-128".into(),
                phase: LedgerPhase::PrOpened,
                dispatched_at: Utc::now(),
                retries: 1,
                pr_url: Some("https://example.test/pr/1".into()),
                branch_updates: 0,
            }],
            skip_list: vec!["task-2".into()],
            counters: LedgerCounters {
                date: Some(Utc::now().date_naive()),
                dispatched_today: 2,
                consecutive_failures: 1,
            },
        };

        ledger.save_to(&path).unwrap();
        let serialized = fs::read_to_string(&path).unwrap();
        assert!(serialized.contains("\"pr_opened\""));
        assert_eq!(Ledger::load_from(&path).unwrap(), ledger);
    }

    #[test]
    fn phases_serialize_to_required_strings() {
        let phases = [
            (LedgerPhase::Dispatched, "dispatched"),
            (LedgerPhase::Claimed, "claimed"),
            (LedgerPhase::Working, "working"),
            (LedgerPhase::PrOpened, "pr_opened"),
            (LedgerPhase::Merged, "merged"),
            (LedgerPhase::Failed, "failed"),
            (LedgerPhase::Escalated, "escalated"),
        ];

        for (phase, expected) in phases {
            assert_eq!(
                serde_json::to_string(&phase).unwrap(),
                format!("\"{expected}\"")
            );
        }
    }

    #[test]
    fn active_workers_counts_every_non_terminal_entry() {
        let entry = |repo: &str, phase| LedgerEntry {
            task_id: "task-1".into(),
            display_id: "#1".into(),
            repo: repo.into(),
            agent_id: "agent".into(),
            branch: "branch".into(),
            phase,
            dispatched_at: Utc::now(),
            retries: 0,
            pr_url: None,
            branch_updates: 0,
        };
        let ledger = Ledger {
            entries: vec![
                entry("/one", LedgerPhase::Working),
                entry("/one", LedgerPhase::PrOpened),
                entry("/two", LedgerPhase::Dispatched),
                entry("/one", LedgerPhase::Merged),
                entry("/two", LedgerPhase::Failed),
                entry("/three", LedgerPhase::Escalated),
            ],
            ..Ledger::default()
        };

        let active = ledger.active_workers();
        assert_eq!(active.count, 3);
        assert_eq!(
            active.repos,
            BTreeMap::from([("/one".to_string(), 2), ("/two".to_string(), 1)])
        );
        // The repository total is the sum of the per-repository counts, so the
        // global cap and the per-repository cap can never read different ledgers.
        assert_eq!(active.count, active.repos.values().sum::<usize>());
    }

    #[test]
    fn missing_ledger_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger.json");
        assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
    }

    #[test]
    fn corrupt_ledger_is_preserved_aside() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("ledger.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(Ledger::load_from(&path).unwrap(), Ledger::default());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(path.with_extension("json.corrupt")).unwrap(),
            "not json"
        );
    }
}
