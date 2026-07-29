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
    /// Passes the auto-merge gate has held this pull request under one reason.
    /// Bounded by `overseer.max_merge_holds`, so a gate reason that never clears
    /// escalates instead of being re-recorded once per poll forever. Defaulted so
    /// ledgers written before the field existed still load.
    #[serde(default)]
    pub merge_hold: MergeHold,
    /// Pull request the merge pass has already recorded a manual-management skip
    /// for. The skip is a standing state — it lasts as long as the operator
    /// leaves the worker manual — so recording it once per pull request keeps it
    /// out of `decisions.jsonl` on every later poll pass. Cleared as soon as the
    /// entry is Overseer's to merge again. Defaulted so ledgers written before
    /// the field existed still load.
    #[serde(default)]
    pub manual_merge_skip: Option<String>,
    /// Fail-safe merge verdicts this pull request has received — the judge
    /// session itself failed rather than saying anything about the change.
    /// Bounded by `overseer.max_merge_judge_fail_safes`, so a judge that never
    /// recovers still escalates instead of re-asking forever. Kept apart from
    /// `merge_hold`: that budget resets on the ordinary `Pending` pass between
    /// one fail-safe verdict and the next, which would otherwise never let this
    /// one accumulate. Reset once the judge answers with a real verdict.
    /// Defaulted so ledgers written before the field existed still load.
    #[serde(default)]
    pub merge_judge_fail_safes: u32,
    /// Whether this entry sits in `Escalated` because the merge-hold budget ran
    /// out. Kept apart from `merge_hold`, which resets on the reconsideration
    /// pass this flag grants. Cleared on merge.
    #[serde(default)]
    pub merge_hold_cap_escalated: bool,
    /// Reconsiderations given to an entry the hold cap escalated. Bounded by
    /// `overseer.max_merge_hold_rechecks`, but only ever charged for a pass
    /// that actually learned something — see `merge_hold_recheck_reason` /
    /// `merge_hold_recheck_head`. A condition that never changes is
    /// reconsidered for free instead of polling this budget away.
    #[serde(default)]
    pub merge_hold_rechecks: u32,
    /// Gate reason the last charged (or escalating) reconsideration pass saw.
    /// Kept apart from `merge_hold.reason`, which `merge_hold::cleared` can
    /// wipe on a pass this module never charges, for the same reason
    /// `merge_hold_cap_escalated` is kept apart from `merge_hold.escalated`.
    #[serde(default)]
    pub merge_hold_recheck_reason: Option<String>,
    /// Head sha the last charged (or escalating) reconsideration pass saw.
    #[serde(default)]
    pub merge_hold_recheck_head: Option<String>,
}

/// What the merge gate remembers about handing this pull request's failures back
/// to its worker.
///
/// The counter is the budget — bounded by `overseer.max_merge_recoveries`, so a
/// worker that cannot fix the failure escalates instead of being re-prompted
/// forever. The (head, base) pair is the deduplication key: it stops the same
/// failure on the same revision from being handed back once per poll interval,
/// while a worker that pushed a fix presents a new head, and a base that moved
/// under a stationary head — a later merge to the base branch conflicting with
/// this pull request — is just as genuinely new a failure. Either alone resets
/// the deduplication; neither resets `charged`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MergeRecovery {
    /// Handbacks charged so far. A new head or a moved base resets the
    /// deduplication, never this.
    pub charged: u32,
    /// Head sha the last handback was charged against.
    pub head: Option<String>,
    /// Base sha the last handback was charged against.
    pub base: Option<String>,
    /// Failures the owning worker could have fixed that were left alone because
    /// `overseer.merge_recovery_enabled` is off. Counted rather than acted on:
    /// the setting is the operator's call, and this is the evidence they need to
    /// make it.
    pub dropped: u32,
    /// Head sha the last drop was recorded against, so a standing failure costs
    /// one decision per revision rather than one per poll.
    pub dropped_head: Option<String>,
    /// Base sha the last drop was recorded against.
    pub dropped_base: Option<String>,
}

/// What the merge gate remembers about holding this pull request back.
///
/// Every gate exit that is not a merge records a reason, and most of those
/// reasons describe a condition that can last hours — a check still running, a
/// head that conflicts with its base, a base branch nobody has protected. The
/// counter is the budget — bounded by `overseer.max_merge_holds`, so a condition
/// that never clears reaches an operator instead of being written to
/// `decisions.jsonl` once per poll for as long as it lasts.
///
/// The (reason, head sha) pair is the key: a new head is the worker's answer and
/// a changed reason is a changed problem, so either restarts the count, while a
/// frozen pair keeps spending it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MergeHold {
    /// Gate reason the passes below were counted against.
    pub reason: Option<String>,
    /// Head sha that reason was recorded against. Empty for a hold decided
    /// before the pull request itself could be read.
    pub head: Option<String>,
    /// Passes charged so far against this pair.
    pub passes: u32,
    /// Whether the budget has already escalated this pair, so the cap is
    /// recorded once rather than on every later pass.
    pub escalated: bool,
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
/// `crate::overseer::daemon::merge_settle`. It lives in the ledger rather than
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
    /// Consecutive spawn failures per dispatch candidate, keyed by
    /// `Candidate::task_id`. Its own budget, separate from
    /// `counters.consecutive_failures`: that counter is shared with the
    /// worker-monitor and merge subsystems and resets on any unrelated merge
    /// landing anywhere in the registry, which is exactly why a single
    /// repeatedly-failing dispatch candidate never tripped it while other
    /// repositories kept dispatching successfully (dropr:_ord_VtFSIiLgWpgmDAGm).
    /// Bounded by `overseer.failure_circuit_threshold`, the same threshold the
    /// global circuit uses, but reaching it moves only the offending candidate
    /// onto `skip_list` — dispatch stays enabled for every other repository and
    /// `dispatch_enabled` is never touched. Cleared on a successful spawn and
    /// once a candidate trips its budget (a candidate later removed from
    /// `skip_list` by an operator starts its budget over).
    pub dispatch_failure_streaks: BTreeMap<String, u32>,
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

    /// Merge failures a worker could have fixed that were left alone because
    /// merge recovery is switched off.
    ///
    /// Counted across every entry the ledger still holds, terminal ones included:
    /// an entry that escalated *because* nobody was handed its failure is the
    /// clearest evidence the setting costs something, and dropping it from the
    /// count would hide exactly the cases worth reading. The retention window is
    /// what bounds how far back this reaches.
    pub fn merge_recovery_drops(&self) -> u32 {
        self.entries.iter().fold(0, |total, entry| {
            total.saturating_add(entry.merge_recovery.dropped)
        })
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
