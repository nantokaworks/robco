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
    /// When the entry reached a terminal phase — merged, failed, or escalated.
    /// `dispatched_at` says when the work started; a history view needs when it
    /// ended, and reconciliation is the only pass that can tell. Stamped once,
    /// on the transition, so a later pass cannot rewrite it. `None` while the
    /// entry is still live, and for entries that settled before the field
    /// existed. Defaulted so ledgers written before then still load.
    #[serde(default)]
    pub settled_at: Option<DateTime<Utc>>,
    pub retries: u32,
    pub pr_url: Option<String>,
    /// Times the auto-merge pass has updated this pull request's branch onto its base
    /// because it had fallen behind. Bounded by `overseer.max_branch_updates`, so a
    /// branch that keeps losing the race against other merges escalates instead of
    /// looping. Defaulted so ledgers written before the field existed still load.
    #[serde(default)]
    pub branch_updates: u32,
    /// Handbacks of a failed merge to the worker that owns this branch.
    /// Defaulted so ledgers written before the field existed still load.
    #[serde(default)]
    pub merge_recovery: MergeRecovery,
    /// Pull request the merge pass has already recorded a manual-management skip
    /// for. The skip is a standing state — it lasts as long as the operator
    /// leaves the worker manual — so recording it once per pull request keeps it
    /// out of `decisions.jsonl` on every later poll pass. Cleared as soon as the
    /// entry is Overseer's to merge again. Defaulted so ledgers written before
    /// the field existed still load.
    #[serde(default)]
    pub manual_merge_skip: Option<String>,
}

/// What the merge gate remembers about handing this pull request's failures back
/// to its worker.
///
/// The counter is the budget — bounded by `overseer.max_merge_recoveries`, so a
/// worker that cannot fix the failure escalates instead of being re-prompted
/// forever. The head sha is the deduplication key: it stops the same failure on
/// the same revision from being handed back once per poll interval, and a worker
/// that pushed a fix presents a new head, which is a genuinely new failure.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MergeRecovery {
    /// Handbacks charged so far. A new head resets the deduplication, never this.
    pub charged: u32,
    /// Head sha the last handback was charged against.
    pub head: Option<String>,
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

/// A repository whose merge has landed but whose primary worktree has not been
/// confirmed to hold it yet.
///
/// The merge gate reads this to keep a second merge out of the same repository
/// until the post-merge `git pull --ff-only` has actually run — see
/// [`crate::overseer::daemon::merge_settle`]. It lives in the ledger rather than
/// in the pass, because the pull it waits on runs on a *later* pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MergeSettling {
    /// Auto-merge passes this repository has been held for. Bounded by
    /// `overseer.max_merge_settle_passes`, so a pull that never succeeds does
    /// not park the repository forever.
    pub passes_held: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
    pub skip_list: Vec<String>,
    pub counters: LedgerCounters,
    /// Repositories waiting on a post-merge fast-forward, keyed by repository
    /// path. Defaulted so ledgers written before the field existed still load.
    pub merge_settling: BTreeMap<String, MergeSettling>,
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

    /// Live merge candidates the merge pass is declining because their worker is
    /// manual-managed.
    ///
    /// Read off the marker the merge pass itself writes rather than re-derived
    /// from the registry, so every surface reports the gate's own verdict instead
    /// of a second opinion that can disagree with it. Terminal entries are
    /// excluded: a pull request a human merged themselves is no longer something
    /// Overseer is holding back.
    pub fn manual_merge_skips(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.manual_merge_skip.is_some() && !terminal(entry.phase))
            .count()
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
#[path = "ledger_tests.rs"]
mod tests;
