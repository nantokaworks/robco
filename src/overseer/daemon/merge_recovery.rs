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

use super::merge_delivery::{DeliveryConfirmation, confirm_delivered, send};
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

/// `FailureClass` and `classify` moved verbatim to `overseer::remedy`, which
/// is now the single place a reason's classification is named — `remedy`
/// also uses `classify` as the fallback for any reason its own table does not
/// list. Re-exported so this module and `merge_recovery_tests.rs` (which
/// reads both through `use super::*`) need no further changes.
pub(super) use crate::overseer::remedy::{FailureClass, classify};

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
/// the next failure is a genuinely new one — and so does a base that moved under
/// a stationary head: `merge_state:dirty` and `merge_state:blocked` are
/// properties of the (head, base) pair, not of the head alone, so a later merge
/// to the base branch that leaves this head conflicting again is just as
/// genuinely new a failure as a pushed fix would be. Neither ever resets the
/// budget, or a worker that pushes a broken fix each round — or a busy base that
/// keeps moving — would loop indefinitely.
pub(super) fn plan(
    entry: &mut LedgerEntry,
    reason: &str,
    head_sha: &str,
    base_sha: &str,
    enabled: bool,
    max_recoveries: u32,
) -> RecoveryPlan {
    // Without a head sha there is no deduplication key, and a handback that
    // cannot be deduplicated would re-prompt the worker on every pass.
    if head_sha.is_empty() || classify(reason) == FailureClass::Operator {
        return RecoveryPlan::Idle;
    }
    if !enabled {
        return dropped(entry, head_sha, base_sha);
    }
    if entry.merge_recovery.head.as_deref() == Some(head_sha)
        && entry.merge_recovery.base.as_deref() == Some(base_sha)
    {
        return RecoveryPlan::Idle;
    }
    if entry.merge_recovery.charged >= max_recoveries {
        return RecoveryPlan::CapReached;
    }
    entry.merge_recovery.charged = entry.merge_recovery.charged.saturating_add(1);
    entry.merge_recovery.head = Some(head_sha.to_owned());
    entry.merge_recovery.base = Some(base_sha.to_owned());
    RecoveryPlan::Dispatch
}

/// Counts one failure the disabled setting left unhanded, at most once per
/// revision.
///
/// The deduplication key is the (head, base) pair the handback would have used,
/// so the cost is one decision per revision rather than one per poll — the same
/// shape the enabled path already has. The count is kept on the entry so
/// `robco overseer status` can report the setting's consequence beside the
/// setting itself.
fn dropped(entry: &mut LedgerEntry, head_sha: &str, base_sha: &str) -> RecoveryPlan {
    if entry.merge_recovery.dropped_head.as_deref() == Some(head_sha)
        && entry.merge_recovery.dropped_base.as_deref() == Some(base_sha)
    {
        return RecoveryPlan::Idle;
    }
    entry.merge_recovery.dropped_head = Some(head_sha.to_owned());
    entry.merge_recovery.dropped_base = Some(base_sha.to_owned());
    entry.merge_recovery.dropped = entry.merge_recovery.dropped.saturating_add(1);
    RecoveryPlan::Dropped
}

/// Reason recorded for a recoverable failure nobody was handed, carrying the
/// failure verbatim so the log says what the setting cost.
fn disabled(reason: &str) -> String {
    format!("merge_recovery_disabled:{reason}")
}

/// Reason recorded when `send` reported success but the target session never
/// showed it started a turn — the paste-swallowed-`Enter` failure mode this
/// module exists to catch. Carries the failure verbatim like `dispatched`, so
/// the operator can see what the worker was never actually told.
fn undelivered(reason: &str) -> String {
    format!("merge_recovery_undelivered:{reason}")
}

/// Reason recorded when the delivery probe itself could not read the pane —
/// distinct from `undelivered`, which means the pane was read cleanly and
/// simply showed no working marker. A probe failure says nothing about
/// whether the worker got the prompt; conflating it with `undelivered` would
/// hide that the decision log's read on this attempt is itself unreliable.
fn capture_failed(reason: &str, detail: &str) -> String {
    format!("merge_recovery_capture_failed:{detail}:{reason}")
}

/// Reason recorded when an undelivered handback exhausts its own bound. Named
/// differently from `undelivered` so the decision log tells apart "held,
/// still retrying" from "gave up, an operator has to look" — the former is
/// the steady state of a slow session, the latter is a worker nobody's
/// prompt ever reached.
fn undeliverable(reason: &str) -> String {
    format!("merge_recovery_undeliverable:{reason}")
}

/// Bounds how many times one revision's handback may fail delivery
/// confirmation before it escalates, mirroring `plan`'s (head, base)-keyed
/// budget for the charged path. This tracks `undelivered_head` rather than
/// `entry.merge_recovery.head`, because `refund` clears the latter after
/// every failed confirm — reusing it here would make each retry look like a
/// fresh candidate and defeat the bound this function exists to enforce.
/// Returns whether the bound is now spent.
pub(super) fn undelivered_cap_reached(
    entry: &mut LedgerEntry,
    head_sha: &str,
    max_recoveries: u32,
) -> bool {
    if entry.merge_recovery.undelivered_head.as_deref() != Some(head_sha) {
        entry.merge_recovery.undelivered_head = Some(head_sha.to_owned());
        entry.merge_recovery.undelivered_charged = 0;
    }
    entry.merge_recovery.undelivered_charged =
        entry.merge_recovery.undelivered_charged.saturating_add(1);
    entry.merge_recovery.undelivered_charged >= max_recoveries
}

/// Clears the undelivered bound once a handback is actually confirmed
/// delivered, so a later failure on a fresh revision starts its own count
/// rather than inheriting one from a revision that did land.
fn clear_undelivered(entry: &mut LedgerEntry) {
    entry.merge_recovery.undelivered_head = None;
    entry.merge_recovery.undelivered_charged = 0;
}

/// Acts on a recorded merge failure: hands it back, escalates it, or leaves it.
pub(super) fn consider(
    entry: &mut LedgerEntry,
    reason: &str,
    head_sha: &str,
    base_sha: &str,
    config: &crate::overseer::config::OverseerConfig,
    registry: &Registry,
    language: Option<&str>,
) -> Result<()> {
    match plan(
        entry,
        reason,
        head_sha,
        base_sha,
        config.merge_recovery_enabled,
        config.max_merge_recoveries,
    ) {
        RecoveryPlan::Idle => Ok(()),
        // Recorded, not acted on: the entry keeps its phase and its worker is
        // never touched, so the daemon behaves exactly as it did with recovery
        // off — it just no longer does so silently.
        RecoveryPlan::Dropped => log(entry, DecisionKind::Hold, &disabled(reason), ""),
        RecoveryPlan::CapReached => {
            entry.phase = LedgerPhase::Escalated;
            entry.worker_escalated = false;
            log(entry, DecisionKind::Escalate, CAP_REACHED, head_sha)
        }
        RecoveryPlan::Dispatch => dispatch(
            entry,
            reason,
            registry,
            language,
            config.max_merge_recoveries,
        ),
    }
}

/// Delivers the remediation prompt to the worker's live session.
fn dispatch(
    entry: &mut LedgerEntry,
    reason: &str,
    registry: &Registry,
    language: Option<&str>,
    max_recoveries: u32,
) -> Result<()> {
    let Some(session) = live_session(&entry.agent_id, registry) else {
        // A worker whose session is gone cannot be handed anything. Naming the
        // session keeps the escalation actionable rather than reading as a
        // recovery that silently did nothing.
        entry.phase = LedgerPhase::Escalated;
        entry.worker_escalated = false;
        let reason = skipped(&format!("missing_session:{}", entry.agent_id));
        return log(entry, DecisionKind::Escalate, &reason, "");
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
            "",
        );
    }
    let hold_reason = match confirm_delivered(&session) {
        DeliveryConfirmation::Confirmed => None,
        // A probe that could not read the pane at all proves nothing about
        // the session either way; a clean read that never showed the working
        // marker is a genuine (if still unproven) non-delivery. Both still
        // refund and retry the same way — subtask #436 owns the retry/escalate
        // bound — but the decision log tells them apart instead of collapsing
        // both into "not working".
        DeliveryConfirmation::NotConfirmed => Some(undelivered(reason)),
        DeliveryConfirmation::CaptureFailed(detail) => Some(capture_failed(reason, &detail)),
    };
    if let Some(hold_reason) = hold_reason {
        // tmux reported success, but nothing confirms the worker actually
        // received the prompt — the exact gap that let a handback sit unsent in
        // an input box while the decision log read as though it had landed.
        // The attempt is un-charged so the next pass gets to retry it instead of
        // quietly losing budget to an instruction nobody was told about, and
        // `PrOpened` is deliberately not set: the worker was not handed
        // anything, so the phase the merge pass reads must not change.
        let head = entry.merge_recovery.head.clone().unwrap_or_default();
        refund(entry);
        if undelivered_cap_reached(entry, &head, max_recoveries) {
            // The worker was never told, and retrying has not fixed that — an
            // operator has to look, the same way a spent `charged` budget
            // escalates rather than re-prompting forever.
            entry.phase = LedgerPhase::Escalated;
            entry.worker_escalated = false;
            return log(entry, DecisionKind::Escalate, &undeliverable(reason), &head);
        }
        return log(entry, DecisionKind::Hold, &hold_reason, "");
    }
    clear_undelivered(entry);
    // The worker now owns the failure, so the entry returns to the phase the
    // merge pass reads. A judge veto had already escalated it; that escalation
    // is superseded rather than left to strand the pull request. Whatever
    // escalation the entry is leaving behind is over too, so its age marker,
    // stuck notice, and last-notified (reason, head) pair go with it — a later
    // re-escalation starts fresh rather than inheriting one that already ran
    // most of the way to `merge_escalation::STUCK_AFTER`, or a suppression
    // that condition already earned.
    entry.phase = LedgerPhase::PrOpened;
    entry.settled_at = None;
    entry.merge_hold_stuck_notified = false;
    entry.escalation_notified_reason = None;
    entry.escalation_notified_head = None;
    log(entry, DecisionKind::Hold, &dispatched(reason), "")?;
    note_on_task(entry, reason);
    Ok(())
}

/// Un-charges a dispatch attempt whose delivery could not be confirmed, and
/// clears the dedup key so the same (head, base) pair is a candidate again on
/// the next pass — a handback nobody received must not spend the budget it
/// exists to protect, nor must it look like this revision was already handled.
fn refund(entry: &mut LedgerEntry) {
    entry.merge_recovery.charged = entry.merge_recovery.charged.saturating_sub(1);
    entry.merge_recovery.head = None;
    entry.merge_recovery.base = None;
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

/// Records the handback on the dropr task, which is the source of truth for what
/// happened to the work.
///
/// A scribble that does not land must not abort the merge pass: the prompt has
/// already been delivered, and failing here would leave the worker working on a
/// failure Overseer then forgot it had charged.
fn note_on_task(entry: &mut LedgerEntry, reason: &str) {
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
            "",
        );
    }
}

fn log(entry: &mut LedgerEntry, kind: DecisionKind, reason: &str, head: &str) -> Result<()> {
    let mut decision = DecisionEntry::new(kind, reason);
    decision.task = Some(entry.task_id.clone());
    decision.repo = Some(entry.repo.clone());
    decision.pr_url = entry.pr_url.clone();
    decision.source = Some("merge_recovery".into());
    decision.escalation_notify = super::merge_escalation::notify(entry, kind, reason, head);
    logging::append(&decision)
}

#[cfg(test)]
#[path = "merge_recovery_tests.rs"]
mod tests;
