use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::{
    model::Status,
    overseer::{
        ledger::PrFacts,
        remedy::{self, Remedy},
    },
};

#[path = "inbox_aggregate.rs"]
mod inbox_aggregate;
pub(crate) use inbox_aggregate::{agent_session_reports, aggregate, current};

/// The one kind of row the Inbox raises: something waiting for an operator's
/// decision. `Question` — a worker sitting on a confirmation prompt — never
/// fired once in 32 days of `inbox.jsonl` and was removed (dropr:460); every
/// row is now an escalation of one shape or another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InboxKind {
    Escalation,
}

impl InboxKind {
    /// Short tag at the head of an inbox row. It doubles as the kind half of an
    /// item's stable identity, so the row the cursor sits on survives a refresh
    /// that re-sorts the list.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Escalation => "ESC",
        }
    }

    /// Spelled-out kind for the preview pane, which has the width for it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Escalation => "escalation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboxItem {
    pub kind: InboxKind,
    /// The repository's short registry name (never its path), taken from the
    /// ledger entry the row is built from. `None` for a decision-sourced row
    /// whose `task` names no ledger entry the aggregation can find — the row
    /// renders the same, just without this part.
    pub repo: Option<String>,
    pub target_session: Option<String>,
    pub target_id: String,
    pub label: String,
    /// The item's reason in full. `label` is written to fit a sidebar row and
    /// gets trimmed to the frame width; this is what the preview pane shows so
    /// the operator can read the whole escalation before acting on it.
    pub detail: String,
    pub at: DateTime<Utc>,
    /// The pull request this row is about, when the ledger entry behind it has
    /// one — used only to show the number in the preview; `remedy` never
    /// reads this.
    pub pr_url: Option<String>,
    /// The pull request's own title, size, and failing check, as of the
    /// daemon's last successful read (dropr:461). `None` when the row has no
    /// matching ledger entry, or the daemon has not read one yet — the row
    /// still renders, just without this part of the preview.
    pub pr_facts: Option<PrFacts>,
    /// A one-sentence, model-written description of this row's own case
    /// (dropr:462), when the board review has written one and it still
    /// matches [`case_signature`](Self::case_signature). `None` renders the
    /// row exactly as it did before the reviewer model existed — a missing or
    /// stale summary is never worse than no summary at all.
    pub sentence: Option<String>,
}

/// Every ledger-sourced row's `detail` starts with this, so [`InboxItem::remedy`]
/// can tell it apart from a decision-sourced row without a field of its own —
/// adding one would force an edit to every literal `InboxItem { .. }` the test
/// suite builds by hand.
pub(crate) const LEDGER_PARKED_MARKER: &str = "ledger entry parked at phase=escalated";

/// The same shape as [`LEDGER_PARKED_MARKER`], but for an entry that still
/// has a pull request and so is eligible for the merge pass's own
/// reconsideration look — see `remedy::LEDGER_PARKED_RESUMABLE`. Checked
/// before `LEDGER_PARKED_MARKER` in [`InboxItem::remedy`] since it shares
/// that marker as a prefix.
pub(crate) const LEDGER_PARKED_RESUMABLE_MARKER: &str =
    "ledger entry parked at phase=escalated, resumable";

impl InboxItem {
    /// The `(kind, target_id)` pair the aggregation dedupes on and dismissals
    /// are recorded against.
    pub(crate) fn identity(&self) -> (String, String) {
        (self.kind.code().to_string(), self.target_id.clone())
    }

    /// What the operator should do about this row.
    ///
    /// The ledger-parked shape carries no reason string to resolve — the fact
    /// of the row *is* the remedy — so it routes straight to a fixed constant.
    /// Every other row carries its reason verbatim in `detail`, resolved
    /// against a live session.
    pub(crate) fn remedy(&self) -> Remedy {
        match self.kind {
            InboxKind::Escalation if self.detail.starts_with(LEDGER_PARKED_RESUMABLE_MARKER) => {
                remedy::LEDGER_PARKED_RESUMABLE
            }
            InboxKind::Escalation if self.detail.starts_with(LEDGER_PARKED_MARKER) => {
                remedy::LEDGER_PARKED
            }
            InboxKind::Escalation => remedy::resolve(&self.detail, self.target_session.is_some()),
        }
    }

    /// Whether the `N/M actionable` count on the Inbox category row should
    /// count this row — derived fresh each call rather than cached, so it can
    /// never drift from what [`remedy`](Self::remedy) itself would say.
    pub(crate) fn actionable(&self) -> bool {
        self.remedy().actionable()
    }

    /// A fingerprint of this row's own case — its reason and, when known, its
    /// pull request's facts. The board review stores this alongside the
    /// sentence it writes about a row (`row_summaries::RowSummary::signature`);
    /// a row whose `detail` or `pr_facts` has since changed no longer matches
    /// it, so a stored sentence about a case that has moved on is never read
    /// back as current.
    pub(crate) fn case_signature(&self) -> String {
        format!("{}|{:?}", self.detail, self.pr_facts)
    }
}

/// One aggregation of the Inbox.
pub(crate) struct Inbox {
    /// What the operator sees: newest first, dismissed alerts removed.
    pub items: Vec<InboxItem>,
    /// The identity of every item the sources produced this pass, dismissed or
    /// not. Pruning the dismissal list needs this unfiltered set — pruning
    /// against `items` would delete the entries doing the hiding, and every
    /// dismissed row would come straight back.
    pub targets: HashSet<(String, String)>,
}

/// What an escalation row needs to know about the agent it names: whether its
/// session is still alive, and where to send a response.
#[derive(Debug, Clone)]
pub(crate) struct AgentSessionReport {
    pub agent_id: String,
    pub tmux_session: String,
    pub status: Status,
}

#[cfg(test)]
#[path = "inbox_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "inbox_summary_tests.rs"]
mod summary_tests;
