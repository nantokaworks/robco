//! Makes sure an explicit merge request — the TUI `m` key, Discord's
//! `!merge`, or `robco_approve`'s no-live-session fallback — always has a
//! live ledger entry to land against, instead of being refused because of
//! the ledger's own bookkeeping (dropr:523).

use chrono::{DateTime, Utc};

use crate::{model::AgentNode, registry::Registry};

use super::{Ledger, LedgerEntry, LedgerPhase};

/// Builds a fresh ledger entry for a registry agent, live from `now`.
///
/// Used by automatic adoption (`daemon::observations::adopt_registry_children_from`)
/// and by [`ensure_landable`] below when an explicit merge request names an
/// agent with no entry yet. `dispatched_at` is always the observing pass's
/// own clock, never `agent.created_at`: a worker watched for the first time
/// long after it actually started must not inherit a staleness reading from
/// before anyone was watching it (dropr:523).
pub(crate) fn new_entry(agent: &AgentNode, repo: &str, now: DateTime<Utc>) -> LedgerEntry {
    LedgerEntry {
        task_id: agent.id.clone(),
        display_id: agent.title.clone(),
        repo: repo.to_string(),
        agent_id: agent.id.clone(),
        branch: agent.branch.clone(),
        phase: LedgerPhase::Dispatched,
        dispatched_at: now,
        settled_at: None,
        retries: 0,
        pr_url: None,
        branch_updates: 0,
        merge_recovery: Default::default(),
        merge_hold: Default::default(),
        merge_hold_cap_escalated: false,
        merge_hold_rechecks: 0,
        merge_hold_recheck_reason: None,
        merge_hold_recheck_head: None,
        prerequisite_wait: None,
        merge_hold_stuck_notified: false,
        escalation_notified_reason: None,
        escalation_notified_head: None,
        worker_escalated: false,
        operator_override: None,
        merge_approval: None,
        pr_facts: None,
    }
}

/// Finds the entry `target` names (its `agent_id` or `display_id` — the same
/// two keys `robco_approve` and the Inbox already key on), adopting one from
/// `registry` on the spot when none exists yet, and reviving a `Failed` or
/// `Escalated` settlement so the request can still land. Returns `None` only
/// when there is truly nothing to act on: no entry, and no registry agent to
/// adopt one from.
///
/// A `Merged` entry is left alone — that settlement is real, and reviving it
/// would replay cleanup on a worktree that is already gone.
///
/// An explicit merge request is the operator asking robco to act now, not
/// automatic adoption — so unlike `daemon::observations::adopt_registry_children_from`,
/// this does not exclude a worker's own subagent. Pressing `m` (or its
/// Discord/MCP equivalents) on a row is itself the request dropr:500 already
/// decided always proceeds.
pub(crate) fn ensure_landable<'a>(
    ledger: &'a mut Ledger,
    target: &str,
    registry: Option<&Registry>,
    now: DateTime<Utc>,
) -> Option<&'a mut LedgerEntry> {
    let index = match ledger
        .entries
        .iter()
        .position(|entry| entry.agent_id == target || entry.display_id == target)
    {
        Some(index) => index,
        None => {
            // Adoption from the registry only ever matches by `agent_id` —
            // a registry agent carries no ledger `display_id` to compare
            // against, so a request naming a display_id with no existing
            // entry has nothing here to adopt.
            let (agent, repo) = find_agent(registry?, target)?;
            ledger.entries.push(new_entry(agent, &repo, now));
            ledger.entries.len() - 1
        }
    };
    let entry = &mut ledger.entries[index];
    if matches!(entry.phase, LedgerPhase::Failed | LedgerPhase::Escalated) {
        revive(entry, now);
    }
    Some(entry)
}

/// Clears the terminal/hold bookkeeping a `Failed` or `Escalated` settlement
/// left behind, and sets the phase to whatever a live entry with this
/// entry's own pull-request state would carry — `PrOpened` once a pull
/// request is known, `Dispatched` otherwise. Never called for `Merged`; see
/// [`ensure_landable`].
fn revive(entry: &mut LedgerEntry, now: DateTime<Utc>) {
    entry.phase = if entry.pr_url.is_some() {
        LedgerPhase::PrOpened
    } else {
        LedgerPhase::Dispatched
    };
    entry.dispatched_at = now;
    entry.settled_at = None;
    entry.merge_recovery = Default::default();
    entry.merge_hold = Default::default();
    entry.merge_hold_cap_escalated = false;
    entry.merge_hold_rechecks = 0;
    entry.merge_hold_recheck_reason = None;
    entry.merge_hold_recheck_head = None;
    entry.merge_hold_stuck_notified = false;
    entry.escalation_notified_reason = None;
    entry.escalation_notified_head = None;
    entry.worker_escalated = false;
    entry.prerequisite_wait = None;
}

fn find_agent<'a>(registry: &'a Registry, target: &str) -> Option<(&'a AgentNode, String)> {
    registry.repos.iter().find_map(|repo| {
        repo.agents
            .iter()
            .find(|agent| agent.id == target)
            .map(|agent| (agent, repo.path.to_string_lossy().into_owned()))
    })
}

#[cfg(test)]
#[path = "landable_tests.rs"]
mod tests;
