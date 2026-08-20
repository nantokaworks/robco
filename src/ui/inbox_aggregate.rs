//! Building an [`Inbox`] from the ledger, the decision log, and live agent
//! sessions — split out of `inbox.rs` so the row types stay next to their own
//! methods and this file stays under the size limit.

use std::collections::{HashMap, HashSet};

use crate::{
    Result,
    model::Status,
    overseer::{
        dismissals::Dismissals,
        ledger::{Ledger, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
        row_summaries::RowSummaries,
    },
    registry::Registry,
};

use super::{
    AgentSessionReport, Inbox, InboxItem, InboxKind, LEDGER_PARKED_MARKER,
    LEDGER_PARKED_RESUMABLE_MARKER,
};

pub(crate) fn aggregate(
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    reports: &[AgentSessionReport],
    dismissals: &Dismissals,
    registry: &Registry,
    summaries: &RowSummaries,
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
        // A decision recorded before its task's ledger entry reached
        // `Merged` still needs the operator's eyes; once merged, the work
        // landed and the row has nothing left to act on. `Failed` and
        // `Escalated` stay visible: they are terminal too, but neither one
        // means the work is done.
        if matches!(ledger_entry, Some(entry) if entry.phase == LedgerPhase::Merged) {
            continue;
        }
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
            repo: ledger_entry.map(|entry| registry.repo_label(&entry.repo).to_string()),
            target_session: session,
            target_id: target_id.to_string(),
            label: format!("{target_id} — {}", decision.reason),
            detail: decision.reason.clone(),
            at: decision.at,
            pr_url: ledger_entry.and_then(|entry| entry.pr_url.clone()),
            pr_facts: ledger_entry.and_then(|entry| entry.pr_facts.clone()),
            sentence: None,
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
            repo: Some(repo.to_string()),
            target_session: session,
            target_id: entry.display_id.clone(),
            label: format!("{} — {repo} / {}", entry.display_id, entry.agent_id),
            detail: format!(
                "{marker} — repo {repo}, agent {}, branch {}",
                entry.agent_id, entry.branch
            ),
            at: entry.dispatched_at,
            pr_url: entry.pr_url.clone(),
            pr_facts: entry.pr_facts.clone(),
            sentence: None,
        });
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.at));
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert((item.kind, item.target_id.clone())));
    let targets = items.iter().map(InboxItem::identity).collect();
    // The suppression filter is the last step, so `targets` still names every
    // identity the sources produced.
    items.retain(|item| !dismissals.suppresses(item.kind.code(), &item.target_id, item.at));
    for item in &mut items {
        item.sentence = summaries
            .get(&item.target_id, &item.case_signature())
            .map(str::to_owned);
    }
    Inbox { items, targets }
}

/// Aggregate the Inbox straight off disk, the way the TUI's background refresh
/// does. Used by `robco inbox clear`, which has no App to read from.
pub(crate) fn current(registry: &Registry) -> Result<Inbox> {
    Ok(aggregate(
        &Ledger::load()?,
        &logging::tail(crate::ui::overseer::DECISION_SNAPSHOT_LIMIT)?,
        &agent_session_reports(registry),
        &Dismissals::load()?,
        registry,
        // Loaded once per aggregation rather than per row: the table is
        // small, and a per-item load would turn one refresh into one file
        // read per row.
        &RowSummaries::load().unwrap_or_default(),
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
