//! One pull request's turn through the auto-merge gate.
//!
//! Split out of `merge` so that file holds the *pass* — which entries are
//! candidates, what each per-repository barrier says, and what an outcome does
//! to the ledger — while this one holds the sequence a single pull request runs
//! through: read, conclusion, dependency, gate, autonomy envelope, merge.
//! `merge_gate` already holds the read-only middle of that sequence for the
//! same reason.

use super::{
    merge_allow::{Judgment, merge_allows},
    merge_apply::merge_now,
    merge_decision::{self, Halt, Outcome},
    merge_dependency, merge_gate, merge_queue, pr_facts,
    protection::ProtectionCache,
    pull_request::{self, base_sha, head_sha},
};
use crate::{Result, config::Config, overseer::ledger::LedgerEntry, registry::Registry};

/// Runs one pull request through the gate: read, conclusion, protection, merge
/// state, checks, merge state queue, autonomy envelope, merge. Every non-merge
/// exit names itself, so the caller has one place to record the decision and
/// one place to decide whether the failure is the owning worker's to fix.
///
/// `settling` names a repository whose own last merge has not landed in the
/// primary worktree yet. It stops the merge and nothing else: every step before
/// the merge either reads GitHub or updates a branch on GitHub's side, and those
/// are exactly the steps the pull request now at the head of the queue has to
/// get through before it can merge at all.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    entry: &mut LedgerEntry,
    url: &str,
    config: &Config,
    cache: &ProtectionCache,
    registry: &Registry,
    consecutive_failures: u32,
    heads: &mut merge_queue::Heads,
    settling: bool,
) -> Result<Outcome> {
    let value = match pull_request::read(&entry.repo, url) {
        Ok(value) => value,
        // The read failed, so there is no revision to attribute a failure to.
        Err(reason) => return Ok(Halt::hold(reason).on("", "")),
    };
    // Captured on every successful read, whatever the pass concludes below —
    // an Inbox row needs these facts regardless of which step ends up holding
    // the pull request (dropr:461).
    entry.pr_facts = pr_facts::extract(&value);
    let head = head_sha(&value).to_owned();
    let base = base_sha(&value).to_owned();
    // A pull request GitHub no longer reports as open is a fact rather than
    // something to wait for, and it is read first because everything below
    // costs GitHub calls that cannot change the answer.
    if let Some(conclusion) = pull_request::conclusion(&value) {
        return Ok(merge_decision::concluded(entry, conclusion).on(&head, &base));
    }
    let dependency = merge_dependency::probe(&entry.task_id, has_known_dropr_task(entry, registry));
    if let Some(halt) = merge_gate::gate(
        entry, url, &value, config, cache, registry, heads, dependency,
    ) {
        return Ok(halt.on(&head, &base));
    }
    // Every step before this one either reads GitHub or updates a branch on
    // GitHub's side, and none of them merges. The repository's own settling
    // wait stops here, before the autonomy envelope and the merge itself —
    // there is nothing left to overlap it with.
    if settling {
        return Ok(Outcome::Settling);
    }
    match merge_allows(entry, &value, config, consecutive_failures)? {
        Judgment::Allow => {}
        Judgment::Halt(halt) => return Ok(halt.on(&head, &base)),
    }
    Ok(
        match merge_now(
            entry,
            url,
            config.merge_strategy,
            config.overseer.protection_mode,
        )? {
            Ok(()) => Outcome::Merged,
            Err(halt) => halt.on(&head, &base),
        },
    )
}

/// Whether `entry`'s still-registered agent carries a dropr task number.
///
/// `model::AgentNode::task_number` is `None` for a manually-created or
/// registry-adopted agent (see that field's own doc) — the case
/// `observations::adopt_registry_children_from` fills a ledger entry for
/// with no real dropr task behind it. An agent no longer in the registry
/// tells us nothing either way, so the probe still runs as it always has;
/// only a *known* absence of a task number skips it (dropr:496).
fn has_known_dropr_task(entry: &LedgerEntry, registry: &Registry) -> bool {
    registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .find(|agent| agent.id == entry.agent_id)
        .is_none_or(|agent| agent.task_number.is_some())
}
