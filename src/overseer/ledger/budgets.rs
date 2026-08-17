//! Small supporting types and methods split out of `ledger.rs` to keep that
//! file under this project's source file size limit: the per-condition
//! budgets a [`super::LedgerEntry`] carries alongside its phase (a merge
//! hold, a handback, a repository settling — none of which changes what
//! phase the entry is in on its own), the ledger's own daily counters, and
//! the [`LedgerEntry`] methods that operate on those budgets directly.

use chrono::{DateTime, NaiveDate, Utc};
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
    /// Undelivered handback attempts charged so far against `undelivered_head`.
    /// Independent from `charged`/`head`/`base`, which `refund` resets after
    /// every failed confirm — this pair must survive that reset, or a handback
    /// nobody's session ever confirms looks like a fresh candidate on every
    /// poll and the retry loop it exists to bound never bites.
    pub undelivered_charged: u32,
    /// Head sha the undelivered counter above is tracking. A new head resets
    /// it, the same way a new head resets `charged`'s deduplication.
    pub undelivered_head: Option<String>,
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

/// A repository whose merge has landed but whose primary checkout's local
/// `main` has not been confirmed to match it yet.
///
/// The merge gate reads this to keep a second merge out of the same repository
/// until the post-merge fast-forward of local `main` has actually run — see
/// `crate::overseer::daemon::merge_settle`. It lives in the ledger rather than
/// in the pass, because the fast-forward it waits on runs on a *later* pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MergeSettling {
    /// Auto-merge passes this repository has been held for. Bounded by
    /// `overseer.max_merge_settle_passes`, so a fast-forward that never
    /// succeeds does not park the repository forever.
    pub passes_held: u32,
}

/// A one-time operator decision to bypass the judge veto/escalate verdict or
/// the autonomy envelope's hard stop currently blocking a pull request —
/// granted through `overseer::runtime_request::RuntimeRequest::OperatorMergeOverride`
/// when the worker's own tmux session is no longer live to answer a fix into
/// (see `mcp::tools::approve`'s fallback). Scoped to the head the operator
/// saw: `daemon::merge_judge_gate` consumes it only when the pull request's
/// current head still matches, and clears it either way once consumed, so a
/// later, unreviewed push cannot ride on a decision made about an older
/// revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorOverride {
    pub head: String,
    pub granted_at: DateTime<Utc>,
}

/// A merge approval Discord's `!merge` queued against a pull request that had
/// not yet reached `Escalated` when the operator approved it — see
/// `overseer::discord::merge_actions::merge`'s queue path and
/// `overseer::discord::ledger_requests::LedgerRequest::Approve`.
///
/// Scoped to the head sha the operator saw, the same way [`OperatorOverride`]
/// is: a later push presents a revision the operator never approved, and this
/// approval is dropped rather than carried forward — the drop is itself
/// recorded in `decisions.jsonl`, because a silently dropped approval looks to
/// the operator like a merge that should already have happened.
///
/// Deliberately narrower than [`OperatorOverride`]: `daemon::merge_judge_gate`
/// consumes it only to satisfy the autonomy envelope's decision to escalate,
/// never a judge veto or escalate verdict. An operator approving from Discord
/// before the judge has spoken has not reviewed a verdict there is anything to
/// bypass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeApproval {
    pub head: String,
    pub granted_at: DateTime<Utc>,
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
