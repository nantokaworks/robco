use crate::overseer::{
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    logging::{self, DecisionEntry, DecisionKind},
    monitor::Observations,
};

/// Every phase transition that produces a Discord event, as
/// `(phase reached, decision kind, reason)`. `merged` covers the terminal
/// phase most workers reach; `failed` and `escalated` are the remaining
/// terminal phases and share one config toggle (`notify_task_finished`) but
/// keep distinct reasons, so an operator's client can still tell the
/// outcomes apart.
const PHASE_EVENTS: &[(LedgerPhase, DecisionKind, &str)] = &[
    (
        LedgerPhase::Dispatched,
        DecisionKind::Dispatch,
        "task_started",
    ),
    (LedgerPhase::PrOpened, DecisionKind::Hold, "pr_opened"),
    (LedgerPhase::Merged, DecisionKind::Merge, "merged"),
    (LedgerPhase::Failed, DecisionKind::Hold, "task_failed"),
    (
        LedgerPhase::Escalated,
        DecisionKind::Escalate,
        "task_escalated",
    ),
];

pub(super) fn record(
    previous: &Ledger,
    next: &Ledger,
    observed: &Observations,
) -> crate::Result<()> {
    for (entry, kind, reason) in transitions(previous, next, observed) {
        event(entry, kind, reason)?;
    }
    Ok(())
}

/// Pure diff of `previous` against `next` (plus inbox reports), separated
/// from `record`'s side effect so the transition logic is testable without
/// touching the shared decision log.
fn transitions<'a>(
    previous: &Ledger,
    next: &'a Ledger,
    observed: &Observations,
) -> Vec<(&'a LedgerEntry, DecisionKind, &'static str)> {
    let mut events = Vec::new();
    for entry in &next.entries {
        let old_phase = previous
            .entries
            .iter()
            .find(|old| old.task_id == entry.task_id && old.agent_id == entry.agent_id)
            .map(|old| old.phase);
        for &(phase, kind, reason) in PHASE_EVENTS {
            if old_phase != Some(phase) && entry.phase == phase {
                events.push((entry, kind, reason));
            }
        }
    }
    for report in observed
        .inbox
        .iter()
        .filter(|report| report.kind == "blocked")
    {
        if let Some(entry) = next
            .entries
            .iter()
            .find(|entry| entry.agent_id == report.agent_id)
        {
            events.push((entry, DecisionKind::Escalate, "worker_blocked"));
        }
    }
    events
}

fn event(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> crate::Result<()> {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.pr_url.clone_from(&entry.pr_url);
    decision.source = Some("daemon_event".into());
    logging::append(&decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overseer::monitor::InboxObservation;
    use chrono::Utc;

    fn entry(task: &str, agent: &str, phase: LedgerPhase) -> LedgerEntry {
        LedgerEntry {
            task_id: task.into(),
            display_id: format!("#{task}"),
            repo: "repo".into(),
            agent_id: agent.into(),
            branch: task.into(),
            phase,
            dispatched_at: Utc::now(),
            settled_at: None,
            retries: 0,
            pr_url: Some("https://pr".into()),
            branch_updates: 0,
            merge_recovery: Default::default(),
            merge_hold: Default::default(),
            manual_merge_skip: None,
            merge_judge_fail_safes: 0,
            merge_hold_cap_escalated: false,
            merge_hold_rechecks: 0,
        }
    }

    fn ledger(entries: Vec<LedgerEntry>) -> Ledger {
        Ledger {
            entries,
            ..Ledger::default()
        }
    }

    fn reasons<'a>(events: &[(&LedgerEntry, DecisionKind, &'a str)]) -> Vec<&'a str> {
        events.iter().map(|(_, _, reason)| *reason).collect()
    }

    #[test]
    fn event_carries_pr_url() {
        let entry = entry("task", "agent", LedgerPhase::PrOpened);
        let mut decision = DecisionEntry::new(DecisionKind::Hold, "pr_opened");
        decision.pr_url.clone_from(&entry.pr_url);
        assert_eq!(decision.pr_url.as_deref(), Some("https://pr"));
    }

    #[test]
    fn a_brand_new_dispatched_entry_fires_task_started_once() {
        let previous = ledger(vec![]);
        let next = ledger(vec![entry("1", "worker-1", LedgerPhase::Dispatched)]);
        let events = transitions(&previous, &next, &Observations::default());
        assert_eq!(reasons(&events), ["task_started"]);
    }

    #[test]
    fn an_unchanged_dispatched_entry_fires_nothing() {
        let board = ledger(vec![entry("1", "worker-1", LedgerPhase::Dispatched)]);
        let events = transitions(&board, &board, &Observations::default());
        assert!(events.is_empty());
    }

    #[test]
    fn each_terminal_transition_fires_its_own_reason_once() {
        for (phase, reason) in [
            (LedgerPhase::Merged, "merged"),
            (LedgerPhase::Failed, "task_failed"),
            (LedgerPhase::Escalated, "task_escalated"),
        ] {
            let previous = ledger(vec![entry("1", "worker-1", LedgerPhase::Working)]);
            let next = ledger(vec![entry("1", "worker-1", phase)]);
            let events = transitions(&previous, &next, &Observations::default());
            assert_eq!(reasons(&events), [reason], "phase {phase:?}");
        }
    }

    #[test]
    fn merged_does_not_also_produce_a_finished_event() {
        let previous = ledger(vec![entry("1", "worker-1", LedgerPhase::PrOpened)]);
        let next = ledger(vec![entry("1", "worker-1", LedgerPhase::Merged)]);
        let events = transitions(&previous, &next, &Observations::default());
        assert_eq!(reasons(&events), ["merged"]);
    }

    #[test]
    fn a_blocked_inbox_report_fires_worker_blocked_for_its_agent() {
        let board = ledger(vec![entry("1", "worker-1", LedgerPhase::Working)]);
        let observed = Observations {
            inbox: vec![InboxObservation {
                at: Utc::now(),
                agent_id: "worker-1".into(),
                kind: "blocked".into(),
            }],
            ..Observations::default()
        };
        let events = transitions(&board, &board, &observed);
        assert_eq!(reasons(&events), ["worker_blocked"]);
    }
}
