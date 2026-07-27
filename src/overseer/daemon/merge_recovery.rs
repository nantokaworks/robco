//! Handing a failed merge back to the worker that owns the branch.
//!
//! When the auto-merge gate cannot land a pull request, most of the reasons it
//! records name something the worker could fix from inside its own worktree: a
//! conflict with the base, a red check, a reviewer's veto. The worker is still
//! alive at that point — `monitor::reconcile_entry` only tears a worker down once
//! its entry reaches `LedgerPhase::Merged` — so the failure can be delivered to
//! the live session instead of parking the pull request until a human looks.
//!
//! Two rails bound the loop. Classification decides which failures a worker can
//! act on at all; anything unrecognised is an operator's problem, because a
//! failure nobody anticipated must not silently drive a worker. The per-entry
//! budget decides how often, and a spent budget escalates rather than re-prompts.
//!
//! Overseer keeps sole possession of the merge throughout: the worker fixes and
//! pushes, and the next merge pass re-evaluates the pull request normally.

use crate::{
    Result,
    overseer::{
        exec::COMMAND_TIMEOUT,
        ledger::{LedgerEntry, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
        templates,
    },
    registry::Registry,
    tmux,
};

/// Reason recorded when the per-entry handback budget is spent.
pub(super) const CAP_REACHED: &str = "merge_recovery_cap_reached";

/// Reason recorded when a failure was handed back, carrying the failure verbatim
/// so the decision log says what the worker was asked to fix.
fn dispatched(reason: &str) -> String {
    format!("merge_recovery_dispatched:{reason}")
}

/// Reason recorded when a warranted handback did not happen. Only refusals that
/// stop a handback that was otherwise due are recorded: an operator-only failure
/// or a revision already handed back is the steady state, and a decision per poll
/// interval for those would bury the ones that mean something.
fn skipped(why: &str) -> String {
    format!("merge_recovery_skipped:{why}")
}

/// Who owns a merge failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureClass {
    /// The worker can fix this from its own worktree.
    Recoverable,
    /// Nothing the worker can change; only an operator can move it.
    Operator,
}

/// Classifies a reason the merge gate recorded.
///
/// Unrecognised reasons classify as `Operator`. Driving a worker on a failure
/// mode nobody anticipated is the expensive mistake here, so the default is to
/// leave it for a human rather than to guess that a prompt would help.
pub(super) fn classify(reason: &str) -> FailureClass {
    let recoverable = matches!(
        reason,
        // A dirty head conflicts with its base; a blocked one is missing a review
        // or a required check. Both are resolved by pushing to the same branch.
        "merge_state:dirty" | "merge_state:blocked" | "checks_not_green"
        // The branch keeps losing the race against other merges, which the worker
        // ends by rebasing its own branch onto the base.
            | crate::overseer::daemon::merge_state::UPDATE_CAP_REACHED
    ) || [
        // `gh pr merge` refused or failed; the branch itself is the thing to fix.
        "merge_exit:",
        "merge_error:",
        // The merge judge's own words are the remediation instruction.
        "judge_veto:",
        "judge_escalate:",
    ]
    .iter()
    .any(|prefix| reason.starts_with(prefix));
    if recoverable {
        FailureClass::Recoverable
    } else {
        FailureClass::Operator
    }
}

/// What to do about one recorded failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryPlan {
    /// The failure is an operator's, or this revision has already been handed
    /// back. Nothing is recorded: these are the steady states, and a decision per
    /// poll interval would bury the real ones.
    Idle,
    /// Charge one handback and prompt the worker.
    Dispatch,
    /// The budget is spent; the entry escalates to an operator.
    CapReached,
    /// A failure the owning worker could have fixed, left alone because recovery
    /// is switched off. Recorded once per revision rather than acted on: whether
    /// to switch the setting on is the operator's call, and until this was
    /// counted the entire recoverable classification was invisible in the
    /// default configuration.
    Dropped,
}

/// Charges one handback to `entry` and reports how to spend it.
///
/// The attempt is charged before it runs, mirroring `merge_state::plan_update`,
/// so a handback that never reaches its worker still consumes budget instead of
/// retrying forever. A new head resets the deduplication — the worker pushed, so
/// the next failure is a genuinely new one — but never the budget, or a worker
/// that pushes a broken fix each round would loop indefinitely.
pub(super) fn plan(
    entry: &mut LedgerEntry,
    reason: &str,
    head_sha: &str,
    enabled: bool,
    max_recoveries: u32,
) -> RecoveryPlan {
    // Without a head sha there is no deduplication key, and a handback that
    // cannot be deduplicated would re-prompt the worker on every pass.
    if head_sha.is_empty() || classify(reason) == FailureClass::Operator {
        return RecoveryPlan::Idle;
    }
    if !enabled {
        return dropped(entry, head_sha);
    }
    if entry.merge_recovery.head.as_deref() == Some(head_sha) {
        return RecoveryPlan::Idle;
    }
    if entry.merge_recovery.charged >= max_recoveries {
        return RecoveryPlan::CapReached;
    }
    entry.merge_recovery.charged = entry.merge_recovery.charged.saturating_add(1);
    entry.merge_recovery.head = Some(head_sha.to_owned());
    RecoveryPlan::Dispatch
}

/// Counts one failure the disabled setting left unhanded, at most once per
/// revision.
///
/// The deduplication key is the head sha the handback would have used, so the
/// cost is one decision per revision rather than one per poll — the same shape
/// the enabled path already has. The count is kept on the entry so
/// `robco overseer status` can report the setting's consequence beside the
/// setting itself.
fn dropped(entry: &mut LedgerEntry, head_sha: &str) -> RecoveryPlan {
    if entry.merge_recovery.dropped_head.as_deref() == Some(head_sha) {
        return RecoveryPlan::Idle;
    }
    entry.merge_recovery.dropped_head = Some(head_sha.to_owned());
    entry.merge_recovery.dropped = entry.merge_recovery.dropped.saturating_add(1);
    RecoveryPlan::Dropped
}

/// Reason recorded for a recoverable failure nobody was handed, carrying the
/// failure verbatim so the log says what the setting cost.
fn disabled(reason: &str) -> String {
    format!("merge_recovery_disabled:{reason}")
}

/// Acts on a recorded merge failure: hands it back, escalates it, or leaves it.
pub(super) fn consider(
    entry: &mut LedgerEntry,
    reason: &str,
    head_sha: &str,
    config: &crate::overseer::config::OverseerConfig,
    registry: &Registry,
    language: Option<&str>,
) -> Result<()> {
    match plan(
        entry,
        reason,
        head_sha,
        config.merge_recovery_enabled,
        config.max_merge_recoveries,
    ) {
        RecoveryPlan::Idle => Ok(()),
        // Recorded, not acted on: the entry keeps its phase and its worker is
        // never touched, so the daemon behaves exactly as it did with recovery
        // off — it just no longer does so silently.
        RecoveryPlan::Dropped => log(entry, DecisionKind::Hold, &disabled(reason)),
        RecoveryPlan::CapReached => {
            entry.phase = LedgerPhase::Escalated;
            log(entry, DecisionKind::Escalate, CAP_REACHED)
        }
        RecoveryPlan::Dispatch => dispatch(entry, reason, registry, language),
    }
}

/// Delivers the remediation prompt to the worker's live session.
fn dispatch(
    entry: &mut LedgerEntry,
    reason: &str,
    registry: &Registry,
    language: Option<&str>,
) -> Result<()> {
    let Some(session) = live_session(&entry.agent_id, registry) else {
        // A worker whose session is gone cannot be handed anything. Naming the
        // session keeps the escalation actionable rather than reading as a
        // recovery that silently did nothing.
        entry.phase = LedgerPhase::Escalated;
        let reason = skipped(&format!("missing_session:{}", entry.agent_id));
        return log(entry, DecisionKind::Escalate, &reason);
    };
    let prompt = templates::merge_recovery_prompt(
        &entry.display_id,
        &entry.task_id,
        entry.pr_url.as_deref().unwrap_or("unknown"),
        reason,
        language,
    );
    if let Err(error) = send(&session, &prompt) {
        // The budget was charged before the attempt ran, so a session that keeps
        // refusing input escalates through the cap instead of retrying forever.
        return log(
            entry,
            DecisionKind::Hold,
            &skipped(&format!("send_failed:{error}")),
        );
    }
    // The worker now owns the failure, so the entry returns to the phase the
    // merge pass reads. A judge veto had already escalated it; that escalation
    // is superseded rather than left to strand the pull request.
    entry.phase = LedgerPhase::PrOpened;
    log(entry, DecisionKind::Hold, &dispatched(reason))?;
    note_on_task(entry, reason);
    Ok(())
}

/// The worker's tmux session, when it is both registered and still running.
fn live_session(agent_id: &str, registry: &Registry) -> Option<String> {
    let session = registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .find(|agent| agent.id == agent_id)
        .map(|agent| agent.tmux_session.clone())?;
    tmux::has_session(&session).ok()?.then_some(session)
}

/// Types the prompt into the session and submits it, the way triage drives a
/// live worker through `TriageAction::RobcoAnswer`.
fn send(session: &str, prompt: &str) -> Result<()> {
    tmux::send_literal_text(session, &tmux::single_line(prompt))?;
    tmux::send_keys(session, &["Enter"])
}

/// Records the handback on the dropr task, which is the source of truth for what
/// happened to the work.
///
/// A scribble that does not land must not abort the merge pass: the prompt has
/// already been delivered, and failing here would leave the worker working on a
/// failure Overseer then forgot it had charged.
fn note_on_task(entry: &LedgerEntry, reason: &str) {
    let content = format!(
        "Overseer could not merge {} and handed the failure back to worker `{}` (handback {}): {reason}",
        entry.pr_url.as_deref().unwrap_or("the pull request"),
        entry.agent_id,
        entry.merge_recovery.charged
    );
    if let Err(error) =
        crate::dropr::scribble_create_timeout(&entry.task_id, &content, COMMAND_TIMEOUT)
    {
        // A note that did not land leaves the handback recorded nowhere an
        // operator looks, so it escalates on its own instead of riding inside
        // another decision's reason where the alert digest never reads it.
        let _ = log(
            entry,
            DecisionKind::Escalate,
            &format!("handback note not recorded in dropr: {error}"),
        );
    }
}

fn log(entry: &LedgerEntry, kind: DecisionKind, reason: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.pr_url = entry.pr_url.clone();
    decision.source = Some("merge_recovery".into());
    logging::append(&decision)
}

#[cfg(test)]
#[path = "merge_recovery_tests.rs"]
mod tests;
