use crate::overseer::{
    ledger::{Ledger, LedgerEntry, LedgerPhase},
    logging::{self, DecisionEntry, DecisionKind},
    monitor::Observations,
};
use chrono::{DateTime, Duration, Utc};

/// How recently a pull request has to have merged for the reconcile pass's
/// discovery of it to read as news.
///
/// An entry stuck in `escalated` or `failed` can sit for weeks before its
/// branch's real fate is re-checked (dropr task #335). When that check finally
/// lands, the merge it finds can be minutes old — a hand-merge answering the
/// escalation right now — or weeks old — the daemon only just learning what
/// already happened. Comparing the pull request's own `mergedAt` against now
/// tells the two apart; a fixed multiple of the default poll interval is a
/// generous margin for a pass that ran a little late or was itself backed
/// off.
const RECONCILE_NOTIFY_FRESHNESS: Duration = Duration::minutes(15);

/// Every phase transition that produces a Discord event, as
/// `(phase reached, decision kind, reason)`. `merged` covers the terminal
/// phase most workers reach; `failed` and `escalated` are the remaining
/// terminal phases and share the `errors` notify tier but keep distinct
/// reasons, so an operator's client can still tell the outcomes apart.
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
    now: DateTime<Utc>,
) -> crate::Result<()> {
    for (entry, kind, reason) in transitions(previous, next, observed, now) {
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
    now: DateTime<Utc>,
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
                if phase == LedgerPhase::Merged
                    && matches!(
                        old_phase,
                        Some(LedgerPhase::Escalated | LedgerPhase::Failed)
                    )
                    && reconciled_silently(entry, observed, now)
                {
                    continue;
                }
                // A `PrOpened -> Escalated` transition is the merge gate's
                // own doing (`daemon::merge`, `merge_recovery`,
                // `merge_judge_fail_safe`, `merge_decision::concluded`, and
                // nothing else moves an entry out of `PrOpened` into
                // `Escalated`). Every one of those paths already logs its
                // own, more specific `Escalate` decision — classified
                // terminal or transient by `merge_escalation` — so this
                // generic phase event would only ever be a second Discord
                // message for the same escalation. Every other route into
                // `Escalated` (a blocked worker report, a dropr task lock
                // release, a killed worker) has no such decision of its own,
                // so it still needs this one.
                if phase == LedgerPhase::Escalated && old_phase == Some(LedgerPhase::PrOpened) {
                    continue;
                }
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

/// Whether an entry's move into `merged` from `escalated` or `failed` is the
/// reconcile pass catching up on a pull request that settled long before this
/// pass looked, rather than news of something that just happened.
///
/// Reads the pull request's own `mergedAt`, not any ledger timestamp: the
/// entry's `settled_at` says when it escalated or failed, which tells nothing
/// about when — relative to *this* pass — the actual merge landed. A read
/// that could not report a merge time (an old `gh`, or a PR this observation
/// stream never matched) is treated as fresh rather than silently swallowed,
/// so a real event is never dropped for want of a field to compare.
fn reconciled_silently(entry: &LedgerEntry, observed: &Observations, now: DateTime<Utc>) -> bool {
    observed
        .prs
        .iter()
        .find(|pr| pr.task_id.as_deref() == Some(entry.task_id.as_str()))
        .and_then(|pr| pr.merged_at)
        .is_some_and(|merged_at| now.signed_duration_since(merged_at) > RECONCILE_NOTIFY_FRESHNESS)
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
#[path = "discord_events_tests.rs"]
mod tests;
