//! Turning an escalate reason into what the operator should do about it.
//!
//! `decisions.jsonl` and the ledger record *why* something stopped moving, in
//! whatever vocabulary the recorder used — a snake_case code, a wrapped and
//! re-wrapped chain of codes, or a free-text sentence. None of that says
//! *what to do*. This module is the one place that answers that question, so
//! the Inbox row, its preview, and the `N/M actionable` count all read the
//! same verdict instead of three call sites guessing at the same reason.
//!
//! [`classify`] answers a narrower, older question — can `merge_recovery` hand
//! this failure back to the worker's own session — and stays the authority for
//! that decision. [`resolve`] answers the broader one an operator asks, and is
//! built to agree with `classify` everywhere `classify` has an opinion: see
//! `remedy::table`'s `table_agrees_with_classify_on_every_recoverable_entry`.

mod parse;
mod table;

pub(crate) use parse::resolve;

/// Who owns a merge failure.
///
/// Moved here — verbatim, aside from visibility — from
/// `daemon::merge_recovery`, which re-exports both names so the rest of that
/// module and its tests need no changes. `merge_recovery` still owns *when* to
/// act on the classification (budgets, deduplication, live sessions); this
/// module owns naming it, both for that caller and for [`resolve`]'s fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureClass {
    /// The worker can fix this from its own worktree.
    Recoverable,
    /// Nothing the worker can change; only an operator can move it.
    Operator,
}

/// Classifies a reason the merge gate recorded.
///
/// Unrecognised reasons classify as `Operator`. Driving a worker on a failure
/// mode nobody anticipated is the expensive mistake here, so the default is to
/// leave it for a human rather than to guess that a prompt would help.
pub(crate) fn classify(reason: &str) -> FailureClass {
    let recoverable = matches!(
        reason,
        // A dirty head conflicts with its base; a blocked one is missing a review
        // or a required check. Both are resolved by pushing to the same branch.
        "merge_state:dirty" | "merge_state:blocked" | "checks_not_green"
        // The branch keeps losing the race against other merges, which the worker
        // ends by rebasing its own branch onto the base.
            | crate::overseer::daemon::merge_state::UPDATE_CAP_REACHED
    ) || [
        // `gh pr merge` refused or failed; the branch itself is the thing to fix.
        "merge_exit:",
        "merge_error:",
    ]
    .iter()
    .any(|prefix| reason.starts_with(prefix));
    if recoverable {
        FailureClass::Recoverable
    } else {
        FailureClass::Operator
    }
}

/// What the operator should do about one Inbox row.
///
/// `Watch` is the only variant [`actionable`](Move::actionable) excludes: it
/// is the one case where nothing has failed and nothing is waiting on a
/// human, so counting it would be counting rows there is nothing to do about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Move {
    /// Type a fix (or the reason itself) into the worker's live session.
    Answer,
    /// Merge the pull request by hand; the automated gate will not.
    Merge,
    /// Reset a tripped circuit or a disabled switch.
    Reset,
    /// Retry or re-dispatch the underlying work.
    Retry,
    /// Read the reason and decide by hand; no key on this row does it.
    Review,
    /// Nothing to do yet — the row is informational.
    Watch,
}

impl Move {
    /// At most six columns, and unique across the six variants — the row
    /// format budgets one word for this at the 24-column sidebar minimum.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Answer => "ANSWER",
            Self::Merge => "MERGE",
            Self::Reset => "RESET",
            Self::Retry => "RETRY",
            Self::Review => "REVIEW",
            Self::Watch => "WATCH",
        }
    }

    /// Whether this row is one the `N/M actionable` count should count.
    pub(crate) fn actionable(self) -> bool {
        !matches!(self, Self::Watch)
    }
}

/// One Inbox row's guidance: what move it is, and the two sentences the
/// preview shows before the row's own reason.
///
/// Named `step` rather than `move` because the latter is a Rust keyword.
/// Every field is a `'static` compile-time value — nothing here is built from
/// the reason string at runtime — so a `Remedy` is as cheap to copy as the
/// `Move` it wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Remedy {
    pub(crate) step: Move,
    /// "what this means" — why the row is here, in the operator's terms.
    pub(crate) means: &'static str,
    /// "next step" — the key, command, or place that clears it.
    pub(crate) next: &'static str,
}

impl Remedy {
    pub(crate) fn tag(self) -> &'static str {
        self.step.tag()
    }

    pub(crate) fn actionable(self) -> bool {
        self.step.actionable()
    }
}

/// The worker is sitting on a confirmation prompt, not an escalated failure —
/// [`InboxItem`](crate::ui::inbox::InboxItem) routes `Question` rows here
/// directly rather than through [`resolve`], because there is no reason
/// string to parse: the fact of the prompt *is* the remedy.
pub(crate) const WORKER_QUESTION: Remedy = Remedy {
    step: Move::Answer,
    means: "the worker is waiting on a confirmation prompt in its own session",
    next: "press Enter and type the answer, or `y` if it is a yes/no prompt",
};

/// A ledger entry parked at `phase=escalated` with no decision reason
/// attached — nothing recorded why, and no pull request for the merge pass
/// to reconsider. `InboxItem::remedy` routes here for that shape directly,
/// the same way it routes a `Question` to [`WORKER_QUESTION`]: there is no
/// reason string to resolve, only the bare fact of the parked entry.
pub(crate) const LEDGER_PARKED: Remedy = Remedy {
    step: Move::Review,
    means: "this ledger entry is parked at phase=escalated with no recorded reason, \
            and the daemon will not retry it on its own",
    next: "open the pull request directly: merge it by hand, re-dispatch the task, \
           or clear the row with `d` once it is handled",
};

/// The same bare-ledger shape as [`LEDGER_PARKED`], but for an entry that
/// still has a pull request — a killed session or a rejected triage action,
/// most often. `command::escalation::escalate_workers` and
/// `triage::completion::apply_completion` grant these a merge-pass
/// reconsideration look (`LedgerEntry::grant_merge_reconsideration`), so
/// unlike `LEDGER_PARKED` the daemon *does* retry it on its own — this row
/// is informational until that pass either merges it or re-escalates it
/// with a reason.
pub(crate) const LEDGER_PARKED_RESUMABLE: Remedy = Remedy {
    step: Move::Watch,
    means: "this ledger entry is parked at phase=escalated with no recorded reason, \
            but it still has a pull request the daemon will recheck on its own",
    next: "nothing to do yet — the next merge pass either merges it or \
           re-escalates it with a reason",
};

/// What [`resolve`] substitutes when the table says `Answer` but there is no
/// live session to answer into — an instruction with nowhere to send it is
/// not the worker's move any more, it is the operator's.
const ORPHANED: Remedy = Remedy {
    step: Move::Review,
    means: "this needed an answer, but the worker's session is no longer live",
    next: "there is no session to send a fix to — handle the pull request \
           or task by hand",
};

/// The generic fallback for a reason `classify` recognises as the worker's to
/// fix but that carries no entry of its own in [`table`] — kept in sync with
/// `classify` automatically, since any reason it adds as `Recoverable` lands
/// here rather than silently defaulting to `Review`.
const RECOVERABLE_FALLBACK: Remedy = Remedy {
    step: Move::Answer,
    means: "the merge gate classified this failure as one the worker can fix",
    next: "press Enter and relay the reason to the worker, or look at the branch by hand",
};

/// The generic fallback for everything else: an unrecognised reason, by the
/// documented rule, is the operator's.
const OPERATOR_FALLBACK: Remedy = Remedy {
    step: Move::Review,
    means: "no specific guidance is registered for this reason",
    next: "review the decision log and the pull request, then decide the next step",
};

fn classify_fallback(reason: &str) -> Remedy {
    match classify(reason) {
        FailureClass::Recoverable => RECOVERABLE_FALLBACK,
        FailureClass::Operator => OPERATOR_FALLBACK,
    }
}

#[cfg(test)]
#[path = "remedy_tests.rs"]
mod tests;
