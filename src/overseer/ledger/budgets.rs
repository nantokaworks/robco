//! Small supporting types and methods split out of `ledger.rs` to keep that
//! file under this project's source file size limit: the per-condition
//! budgets a [`super::LedgerEntry`] carries alongside its phase (a merge
//! hold, a handback, a repository settling — none of which changes what
//! phase the entry is in on its own), the ledger's own daily counters, and
//! the [`LedgerEntry`] methods that operate on those budgets directly.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::LedgerEntry;

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

/// The ledger's daily dispatch bookkeeping: today's count against
/// `overseer.daily_dispatch_limit`, reset once `date` no longer matches, and
/// the consecutive-failure count the dispatch/merge circuit breaker shares.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LedgerCounters {
    pub date: Option<NaiveDate>,
    pub dispatched_today: u32,
    pub consecutive_failures: u32,
}

impl LedgerEntry {
    /// Grants a merge-pass reconsideration look, the way
    /// `daemon::merge_hold_recheck::escalated` does for a hold-cap
    /// escalation — see `daemon::merge_hold_recheck::due`. For an
    /// escalation raised outside the merge subsystem there is no gate
    /// reason or head sha to seed, so the first reconsideration pass
    /// always counts as a change and spends one look.
    pub fn grant_merge_reconsideration(&mut self, reason: &str) {
        self.merge_hold_cap_escalated = true;
        self.merge_hold_rechecks = 0;
        self.merge_hold_recheck_reason = Some(reason.to_owned());
        self.merge_hold_recheck_head = None;
    }
}
