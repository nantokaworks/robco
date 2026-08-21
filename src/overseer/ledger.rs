use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};

use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

use crate::Result;

mod budgets;
pub use budgets::{
    LedgerCounters, MergeApproval, MergeHold, MergeRecovery, MergeSettling, OperatorOverride,
};
mod landable;
pub(crate) use landable::{ensure_landable, new_entry};
mod phase;
pub use phase::{LedgerPhase, holds_capacity, terminal, waiting_on_prerequisite};
mod pr_facts;
pub use pr_facts::PrFacts;
mod slots;
pub use slots::ActiveWorkers;

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
    /// When this entry began waiting on a dropr `blocks` dependency edge to
    /// resolve — a worker discovered mid-implementation that its task is
    /// ordered behind another and reported `waiting-prerequisite`, or the
    /// auto-merge gate found the entry's pull request held by one. `None`
    /// while nothing holds the entry on a prerequisite. Bounded by
    /// `overseer.max_prerequisite_wait_hours`, so a prerequisite that never
    /// lands still escalates once instead of waiting forever — see
    /// `overseer::monitor::apply::apply_prerequisite_wait`. Defaulted so
    /// ledgers written before the field existed still load.
    #[serde(default)]
    pub prerequisite_wait: Option<DateTime<Utc>>,
    /// Whether `daemon::merge_escalation::sweep_stuck` has already notified
    /// once about this entry's current escalation sitting past the stuck
    /// threshold. Kept apart from `merge_hold_cap_escalated` because that
    /// flag grants free reconsideration passes forever — it never sets this
    /// one, and the daemon's own age tracking must not repeat once it has
    /// spoken. Reset alongside `settled_at` wherever the entry leaves this
    /// escalation behind: `merge_hold_recheck::settle` and a successful
    /// `merge_recovery` handback.
    #[serde(default)]
    pub merge_hold_stuck_notified: bool,
    /// `(reason, head)` the last immediate Discord escalation for this entry
    /// reported, for an escalation reason outside `merge_escalation`'s
    /// terminal/transient vocabulary (a branch-update cap, an unprotected
    /// base). A later pass reporting the identical pair is the
    /// same unresolved condition already shown to the operator, not a new
    /// one, and suppresses its notification (dropr:414). Reset alongside
    /// `settled_at` wherever the entry leaves this escalation behind:
    /// `merge_hold_recheck::settle` and a successful `merge_recovery`
    /// handback. Defaulted so ledgers written before the field existed still
    /// load.
    #[serde(default)]
    pub escalation_notified_reason: Option<String>,
    #[serde(default)]
    pub escalation_notified_head: Option<String>,
    /// Whether the entry's `Escalated` phase came from a worker's own report
    /// rather than a merge-subsystem safety valve or a killed session. Gates
    /// `monitor::apply::apply_escalation_resolution` — see there for why.
    #[serde(default)]
    pub worker_escalated: bool,
    /// A pending one-time operator merge request granted through
    /// `robco_approve`'s no-live-session fallback — see [`OperatorOverride`].
    /// Defaulted so ledgers written before the field existed still load.
    #[serde(default)]
    pub operator_override: Option<OperatorOverride>,
    /// A merge request queued by the TUI `m` key or Discord's `!merge`
    /// against a pull request, whatever phase it was in — see
    /// [`MergeApproval`]. Defaulted so ledgers written before the field
    /// existed still load.
    #[serde(default)]
    pub merge_approval: Option<MergeApproval>,
    /// The pull request's own title, size, and any failing check, as of the
    /// gate's last successful read — see [`PrFacts`]. `None` before the first
    /// successful read, or for a ledger written before the field existed;
    /// [`InboxItem::remedy`](crate::ui::inbox::InboxItem) never depends on
    /// it, so a row with no facts yet still renders, just without them.
    #[serde(default)]
    pub pr_facts: Option<PrFacts>,
    /// When the worker reported `--kind done` for this entry — the worker's
    /// own claim that its work is finished, not proof of it. `Merged` /
    /// `Failed` / `Escalated` stay derived from what is observed (PR state,
    /// session state); this only lets the row distinguish "still working"
    /// from "says it is done and this is waiting on an operator". `None`
    /// while no such report has arrived, or for a ledger written before the
    /// field existed. See `monitor::apply::apply_inbox`.
    #[serde(default)]
    pub worker_finished_at: Option<DateTime<Utc>>,
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
    /// Consecutive named launches one task has been held on `branch_exists`,
    /// keyed by `Candidate::task_id`. A left-over branch is not a transient
    /// state — only an operator removing it, or the worker that owns it
    /// finishing, clears it — so this bounds the hold:
    /// `dispatch::worker::MAX_BRANCH_EXISTS_HOLDS` escalates the candidate
    /// once spent rather than holding it forever. Cleared the moment the
    /// branch conflict clears, so a branch the operator deletes launches
    /// again with a fresh count.
    pub branch_exists_holds: BTreeMap<String, u32>,
}

impl Ledger {
    /// Merges Discord's `!merge` queued an approval for while they were still
    /// waiting on the deterministic gate, and have not yet drained.
    ///
    /// Read by `robco status --debug`, so an operator can see how
    /// many pending merges already carry their own approval rather than a
    /// future escalation.
    pub fn queued_merge_approvals(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.merge_approval.is_some() && !terminal(entry.phase))
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

    /// Exposed beyond `save`/`load`'s fixed `ledger_path()` so a caller that
    /// already holds an explicit path — currently `runtime_request::drain_in`,
    /// checkpointing an applied request before its file is acked — can persist
    /// through the same atomic write without a second writer touching the file.
    pub(crate) fn save_to(&self, path: &Path) -> Result<()> {
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
