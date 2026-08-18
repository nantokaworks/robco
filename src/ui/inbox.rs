use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::{
    Result,
    model::Status,
    overseer::{
        dismissals::Dismissals,
        ledger::{Ledger, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
        remedy::{self, Remedy},
    },
    registry::Registry,
};

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
    pub target_session: Option<String>,
    pub target_id: String,
    pub label: String,
    /// The item's reason in full. `label` is written to fit a sidebar row and
    /// gets trimmed to the frame width; this is what the preview pane shows so
    /// the operator can read the whole escalation before acting on it.
    pub detail: String,
    pub at: DateTime<Utc>,
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

pub(crate) fn aggregate(
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    reports: &[AgentSessionReport],
    dismissals: &Dismissals,
    registry: &Registry,
) -> Inbox {
    let agents = reports
        .iter()
        .map(|report| (report.agent_id.as_str(), report))
        .collect::<HashMap<_, _>>();
    let tasks = ledger
        .entries
        .iter()
        .map(|entry| (entry.task_id.as_str(), entry))
        .chain(
            ledger
                .entries
                .iter()
                .map(|entry| (entry.display_id.as_str(), entry)),
        )
        .collect::<HashMap<_, _>>();
    let mut items = Vec::new();

    // A release-pipeline skip (`DecisionKind::Skip`, `overseer::
    // release_pipeline::skip`) rides in here alongside a plain `Escalate`:
    // it already reaches Discord (`discord::notifications::from_decision`),
    // and an unready checkout blocks every release after it, so the operator
    // needs the same TUI Inbox visibility a task escalation gets.
    for decision in decisions.iter().filter(|entry| {
        entry.kind == DecisionKind::Escalate
            || (entry.kind == DecisionKind::Skip
                && entry
                    .reason
                    .starts_with(crate::overseer::release_pipeline::SKIPPED_PREFIX))
    }) {
        let ledger_entry = decision
            .task
            .as_deref()
            .and_then(|target| tasks.get(target).copied());
        let target_id = ledger_entry
            .map(|entry| entry.display_id.as_str())
            .or(decision.task.as_deref())
            .unwrap_or("overseer");
        let session = ledger_entry
            .and_then(|entry| agents.get(entry.agent_id.as_str()))
            .filter(|report| !matches!(report.status, Status::Dead | Status::BranchOnly))
            .map(|report| report.tmux_session.clone());
        items.push(InboxItem {
            kind: InboxKind::Escalation,
            target_session: session,
            target_id: target_id.to_string(),
            label: format!("{target_id} — {}", decision.reason),
            detail: decision.reason.clone(),
            at: decision.at,
        });
    }
    for entry in ledger
        .entries
        .iter()
        .filter(|entry| entry.phase == LedgerPhase::Escalated)
    {
        let session = agents
            .get(entry.agent_id.as_str())
            .filter(|report| !matches!(report.status, Status::Dead | Status::BranchOnly))
            .map(|report| report.tmux_session.clone());
        let repo = registry.repo_label(&entry.repo);
        // The ledger records no reason, so name what the row actually is: an
        // entry parked at `escalated`. One still has a pull request the
        // merge pass will reconsider on its own
        // (`LedgerEntry::grant_merge_reconsideration`); the other has
        // nothing left for the daemon to act on. The leading marker is what
        // `InboxItem::remedy` reads to route the row without trying to
        // resolve the descriptive text that follows as a reason.
        let marker = if entry.pr_url.is_some() {
            LEDGER_PARKED_RESUMABLE_MARKER
        } else {
            LEDGER_PARKED_MARKER
        };
        items.push(InboxItem {
            kind: InboxKind::Escalation,
            target_session: session,
            target_id: entry.display_id.clone(),
            label: format!("{} — {repo} / {}", entry.display_id, entry.agent_id),
            detail: format!(
                "{marker} — repo {repo}, agent {}, branch {}",
                entry.agent_id, entry.branch
            ),
            at: entry.dispatched_at,
        });
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.at));
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert((item.kind, item.target_id.clone())));
    let targets = items.iter().map(InboxItem::identity).collect();
    // The suppression filter is the last step, so `targets` still names every
    // identity the sources produced.
    items.retain(|item| !dismissals.suppresses(item.kind.code(), &item.target_id, item.at));
    Inbox { items, targets }
}

/// Aggregate the Inbox straight off disk, the way the TUI's background refresh
/// does. Used by `robco overseer clear-inbox`, which has no App to read from.
pub(crate) fn current(registry: &Registry) -> Result<Inbox> {
    Ok(aggregate(
        &Ledger::load()?,
        &logging::tail(super::overseer::DECISION_SNAPSHOT_LIMIT)?,
        &agent_session_reports(registry),
        &Dismissals::load()?,
        registry,
    ))
}

pub(crate) fn agent_session_reports(registry: &Registry) -> Vec<AgentSessionReport> {
    registry
        .repos
        .iter()
        .flat_map(|repo| {
            repo.agents.iter().map(|agent| AgentSessionReport {
                agent_id: agent.id.clone(),
                tmux_session: agent.tmux_session.clone(),
                status: agent.status,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "inbox_tests.rs"]
mod tests;
