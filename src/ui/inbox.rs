use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::{
    model::Status,
    overseer::{
        ledger::{Ledger, LedgerPhase},
        logging::{DecisionEntry, DecisionKind},
    },
    registry::Registry,
    status::{self, WatchStatusState},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InboxKind {
    Escalation,
    Question,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboxItem {
    pub kind: InboxKind,
    pub target_session: Option<String>,
    pub target_id: String,
    pub label: String,
    pub at: DateTime<Utc>,
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
) -> Vec<InboxItem> {
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
        items.push(InboxItem {
            kind: InboxKind::Escalation,
            target_session: session,
            target_id: entry.display_id.clone(),
            label: format!("{} — {} / {}", entry.display_id, entry.repo, entry.agent_id),
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
            at: report.at,
        });
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.at));
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert((item.kind, item.target_id.clone())));
    items
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
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::overseer::ledger::LedgerEntry;

    fn at(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, second).unwrap()
    }

    fn report(awaiting_confirmation: bool) -> AgentQuestionReport {
        AgentQuestionReport {
            agent_id: "agent-1".into(),
            title: "worker".into(),
            tmux_session: "robco-agent-1".into(),
            status: Status::Waiting,
            awaiting_confirmation,
            at: at(3),
        }
    }

    fn report_with_status(status: Status) -> AgentQuestionReport {
        AgentQuestionReport {
            status,
            ..report(false)
        }
    }

    fn escalated_ledger() -> Ledger {
        Ledger {
            entries: vec![LedgerEntry {
                task_id: "task-1".into(),
                display_id: "#159".into(),
                repo: "robco".into(),
                agent_id: "agent-1".into(),
                branch: "task-159".into(),
                phase: LedgerPhase::Escalated,
                dispatched_at: at(1),
                settled_at: None,
                retries: 0,
                pr_url: None,
                branch_updates: 0,
                merge_recovery: Default::default(),
                manual_merge_skip: None,
            }],
            ..Ledger::default()
        }
    }

    #[test]
    fn question_and_live_escalation_are_answerable() {
        let mut decision = DecisionEntry::new(DecisionKind::Escalate, "needs user");
        decision.at = at(2);
        decision.task = Some("task-1".into());
        let items = aggregate(&escalated_ledger(), &[decision], &[report(true)]);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, InboxKind::Question);
        assert_eq!(items[0].target_session.as_deref(), Some("robco-agent-1"));
        assert_eq!(items[1].kind, InboxKind::Escalation);
        assert_eq!(items[1].target_session.as_deref(), Some("robco-agent-1"));
        assert!(items[1].label.contains("needs user"));
    }

    #[test]
    fn excludes_waiting_agents_without_confirmation_prompt() {
        assert!(aggregate(&Ledger::default(), &[], &[report(false)]).is_empty());
    }

    #[test]
    fn global_and_stale_escalations_are_display_only() {
        let global = DecisionEntry::new(DecisionKind::Escalate, "global alert");
        let stale = aggregate(&escalated_ledger(), &[], &[]);
        let global = aggregate(&Ledger::default(), &[global], &[]);

        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].target_session, None);
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].target_session, None);
        assert_eq!(global[0].target_id, "overseer");
    }

    #[test]
    fn escalation_requires_a_live_target_session() {
        for status in [Status::Dead, Status::BranchOnly] {
            let items = aggregate(&escalated_ledger(), &[], &[report_with_status(status)]);
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].target_session, None);
        }

        let items = aggregate(
            &escalated_ledger(),
            &[],
            &[report_with_status(Status::Running)],
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].target_session.as_deref(), Some("robco-agent-1"));
    }
}
