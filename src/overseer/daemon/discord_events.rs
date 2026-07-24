use crate::overseer::{
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    logging::{self, DecisionEntry, DecisionKind},
    monitor::Observations,
};

pub(super) fn record(
    previous: &Ledger,
    next: &Ledger,
    observed: &Observations,
) -> crate::Result<()> {
    for entry in &next.entries {
        let old_phase = previous
            .entries
            .iter()
            .find(|old| old.task_id == entry.task_id && old.agent_id == entry.agent_id)
            .map(|old| old.phase);
        if old_phase != Some(LedgerPhase::PrOpened) && entry.phase == LedgerPhase::PrOpened {
            event(entry, DecisionKind::Hold, "pr_opened")?;
        }
        if old_phase != Some(LedgerPhase::Merged) && entry.phase == LedgerPhase::Merged {
            event(entry, DecisionKind::Merge, "merged")?;
        }
    }
    for report in observed
        .inbox
        .iter()
        .filter(|report| report.kind == "blocked")
    {
        let Some(entry) = next.entries.iter().find(|entry| {
            entry.agent_id == report.agent_id
                || report.task_id.as_deref() == Some(entry.task_id.as_str())
        }) else {
            continue;
        };
        event(entry, DecisionKind::Escalate, "worker_blocked")?;
    }
    Ok(())
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
    use chrono::Utc;

    #[test]
    fn event_carries_pr_url() {
        let entry = LedgerEntry {
            task_id: "task".into(),
            display_id: "#1".into(),
            repo: "repo".into(),
            agent_id: "agent".into(),
            branch: "branch".into(),
            phase: LedgerPhase::PrOpened,
            dispatched_at: Utc::now(),
            retries: 0,
            pr_url: Some("https://pr".into()),
            branch_updates: 0,
        };
        let mut decision = DecisionEntry::new(DecisionKind::Hold, "pr_opened");
        decision.pr_url.clone_from(&entry.pr_url);
        assert_eq!(decision.pr_url.as_deref(), Some("https://pr"));
    }
}
