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

use super::merge_delivery::{DeliveryConfirmation, confirm_delivered, is_busy, send};
use crate::{
    Result,
    overseer::{
        ledger::{LedgerEntry, LedgerPhase},
        logging::{self, DecisionEntry, DecisionKind},
        templates,
    },
    registry::Registry,
    tmux,
};

#[path = "merge_recovery_plan.rs"]
mod plan;
use plan::disabled;
pub(super) use plan::{CAP_REACHED, RecoveryPlan, plan};

#[path = "merge_recovery_pending.rs"]
mod pending;
pub(super) use pending::discard as discard_pending;

#[path = "merge_recovery_note.rs"]
mod note;
use note::note_on_task;

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

/// The daemon-wide context `consider` needs beyond the entry it is acting on:
/// bundled into one struct purely to stay under clippy's argument-count limit,
/// since these four always travel together from `merge_repo_pass::hold`.
pub(super) struct RecoveryEnv<'a> {
    pub(super) config: &'a crate::overseer::config::OverseerConfig,
    pub(super) server: &'a tmux::TmuxServer,
    pub(super) registry: &'a Registry,
    pub(super) language: Option<&'a str>,
}

/// Acts on a recorded merge failure: hands it back, escalates it, or leaves it.
pub(super) fn consider(
    entry: &mut LedgerEntry,
    reason: &str,
    head_sha: &str,
    base_sha: &str,
    env: &RecoveryEnv,
) -> Result<()> {
    match plan(
        entry,
        reason,
        head_sha,
        base_sha,
        env.config.merge_recovery_enabled,
        env.config.max_merge_recoveries,
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
            env.server,
            env.registry,
            env.language,
            env.config.max_merge_recoveries,
        ),
    }
}

/// Delivers the remediation prompt to the worker's live session.
fn dispatch(
    entry: &mut LedgerEntry,
    reason: &str,
    server: &tmux::TmuxServer,
    registry: &Registry,
    language: Option<&str>,
    max_recoveries: u32,
) -> Result<()> {
    let Some(session) = live_session(server, &entry.agent_id, registry) else {
        // A worker whose session is gone cannot be handed anything. Naming the
        // session keeps the escalation actionable rather than reading as a
        // recovery that silently did nothing.
        pending::discard(entry);
        entry.phase = LedgerPhase::Escalated;
        entry.worker_escalated = false;
        let reason = skipped(&format!("missing_session:{}", entry.agent_id));
        return log(entry, DecisionKind::Escalate, &reason, "");
    };
    if is_busy(server, &session) {
        // Typing over a live turn corrupts whatever the worker is composing —
        // refusing the send is correct, the same call
        // `mcp::tools::report::guard_delivery` makes for a `robco report`
        // into a busy session. `pending::withhold` remembers the failed
        // attempt on the entry so a later pass, once the worker frees up,
        // retries it instead of the daemon silently giving up (dropr:530),
        // and refunds the charge `plan` took on the way in — the same as any
        // other attempt that could not send anything.
        return match pending::withhold(entry, reason, max_recoveries) {
            pending::Outcome::Abandoned { reason, head } => {
                // A worker that is never idle must not be retried forever.
                entry.phase = LedgerPhase::Escalated;
                entry.worker_escalated = false;
                log(entry, DecisionKind::Escalate, &reason, &head)
            }
            pending::Outcome::Held(reason) => log(entry, DecisionKind::Hold, &reason, ""),
        };
    }
    // The session is idle, so this pass either delivers the current failure
    // or records why it could not — either way, whatever this entry was
    // withholding from an earlier busy pass is superseded rather than left
    // to look pending after the fact.
    pending::discard(entry);
    let prompt = templates::merge_recovery_prompt(
        &entry.display_id,
        &entry.task_id,
        entry.pr_url.as_deref().unwrap_or("unknown"),
        reason,
        language,
    );
    if let Err(error) = send(server, &session, &prompt) {
        // The budget was charged before the attempt ran, so a session that keeps
        // refusing input escalates through the cap instead of retrying forever.
        return log(
            entry,
            DecisionKind::Hold,
            &skipped(&format!("send_failed:{error}")),
            "",
        );
    }
    let hold_reason = match confirm_delivered(server, &session) {
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
    // merge pass reads. An earlier escalation had already parked it here;
    // that escalation is superseded rather than left to strand the pull
    // request. Whatever
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
fn live_session(server: &tmux::TmuxServer, agent_id: &str, registry: &Registry) -> Option<String> {
    let session = registry
        .repos
        .iter()
        .flat_map(|repo| &repo.agents)
        .find(|agent| agent.id == agent_id)
        .map(|agent| agent.tmux_session.clone())?;
    tmux::has_session(server, &session).ok()?.then_some(session)
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
#[path = "merge_recovery_dispatch_tests.rs"]
mod dispatch_tests;
#[cfg(test)]
#[path = "merge_recovery_tests.rs"]
mod tests;
