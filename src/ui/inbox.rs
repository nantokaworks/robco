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
    status::{self, WatchStatusState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InboxKind {
    Escalation,
    Question,
}

impl InboxKind {
    /// Short tag at the head of an inbox row. It doubles as the kind half of an
    /// item's stable identity, so the row the cursor sits on survives a refresh
    /// that re-sorts the list — and so a dismissal recorded against one kind
    /// cannot hide the other kind's row for the same target.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Escalation => "ESC",
            Self::Question => "?",
        }
    }

    /// Spelled-out kind for the preview pane, which has the width for it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Escalation => "escalation",
            Self::Question => "question",
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

impl InboxItem {
    /// The `(kind, target_id)` pair the aggregation dedupes on and dismissals
    /// are recorded against.
    pub(crate) fn identity(&self) -> (String, String) {
        (self.kind.code().to_string(), self.target_id.clone())
    }

    /// What the operator should do about this row.
    ///
    /// `Question` and the ledger-parked shape of `Escalation` carry no reason
    /// string to resolve — the fact of the row *is* the remedy — so they route
    /// straight to a fixed constant. A decision-sourced `Escalation` carries
    /// its reason verbatim in `detail`, resolved against a live session.
    pub(crate) fn remedy(&self) -> Remedy {
        match self.kind {
            InboxKind::Question => remedy::WORKER_QUESTION,
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

#[derive(Debug, Clone)]
pub(crate) struct AgentQuestionReport {
    pub agent_id: String,
    pub title: String,
    pub tmux_session: String,
    pub status: Status,
    pub awaiting_confirmation: bool,
    pub at: DateTime<Utc>,
}

pub(crate) fn aggregate(
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    reports: &[AgentQuestionReport],
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

    for decision in decisions
        .iter()
        .filter(|entry| entry.kind == DecisionKind::Escalate)
    {
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
        items.push(InboxItem {
            kind: InboxKind::Escalation,
            target_session: session,
            target_id: entry.display_id.clone(),
            label: format!("{} — {repo} / {}", entry.display_id, entry.agent_id),
            // The ledger records no reason, so name what the row actually is:
            // an entry parked at `escalated`, which nothing ages out. The
            // leading `LEDGER_PARKED_MARKER` is what `InboxItem::remedy` reads
            // to route this row to `remedy::LEDGER_PARKED` instead of trying
            // to resolve the descriptive text that follows as a reason.
            detail: format!(
                "{LEDGER_PARKED_MARKER} — repo {repo}, agent {}, branch {}",
                entry.agent_id, entry.branch
            ),
            at: entry.dispatched_at,
        });
    }
    for report in reports
        .iter()
        .filter(|report| report.status == Status::Waiting && report.awaiting_confirmation)
    {
        items.push(InboxItem {
            kind: InboxKind::Question,
            target_session: Some(report.tmux_session.clone()),
            target_id: report.agent_id.clone(),
            label: format!("{} — {}", report.agent_id, report.title),
            detail: format!(
                "worker is waiting on a confirmation prompt: {}",
                report.title
            ),
            at: report.at,
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
        &question_reports(registry),
        &Dismissals::load()?,
        registry,
    ))
}

pub(crate) fn question_reports(registry: &Registry) -> Vec<AgentQuestionReport> {
    registry
        .repos
        .iter()
        .flat_map(|repo| {
            repo.agents.iter().map(move |agent| {
                let awaiting_confirmation = if agent.status == Status::Waiting {
                    status::classify_agent_status(
                        &repo.path,
                        &agent.worktree_path,
                        &agent.branch,
                        &agent.tmux_session,
                        &mut WatchStatusState::default(),
                    )
                    .is_some_and(|report| report.awaiting_confirmation)
                } else {
                    false
                };
                AgentQuestionReport {
                    agent_id: agent.id.clone(),
                    title: agent.title.clone(),
                    tmux_session: agent.tmux_session.clone(),
                    status: agent.status,
                    awaiting_confirmation,
                    at: agent.updated_at.with_timezone(&Utc),
                }
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "inbox_tests.rs"]
mod tests;
