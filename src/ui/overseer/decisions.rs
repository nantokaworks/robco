//! What the OVERSEER frame reads back out of the decision log.
//!
//! Stand-offs first: an overseer that declines to dispatch looks exactly like
//! an idle one from the outside, and reading the stand-offs out of the log is
//! what lets the frame tell the operator which is which.
//!
//! Then the decision list itself. It is capped — the log is append-only and
//! what the UI holds is only a tail of it — so it ends by saying it is capped
//! rather than letting the newest few read as the whole history.

use ratatui::text::{Line, Span};

use crate::locale::{Locale, t};
use crate::model::MergeLifecycle;
use crate::overseer::discord::humanize;
use crate::overseer::ledger::{Ledger, LedgerPhase};
use crate::overseer::logging::{DecisionEntry, DecisionKind};
use crate::ui::theme::DEFAULT as THEME;

/// Reason prefix `overseer::dispatch::claim` writes when it stands off a task
/// an agent outside the overseer's ledger has claimed.
const EXTERNAL_CLAIM_PREFIX: &str = "claimed_elsewhere:";

/// Current stand-offs, as `task → holder`, newest state per task.
///
/// A later dispatch of the same task clears its entry, so the set reflects what
/// is being stood off now rather than every stand-off the log ever recorded.
pub(super) fn standoffs(decisions: &[DecisionEntry]) -> Vec<String> {
    let mut held: Vec<(&str, &str)> = Vec::new();
    for entry in decisions {
        let Some(task) = entry.task.as_deref() else {
            continue;
        };
        let holder = entry.reason.strip_prefix(EXTERNAL_CLAIM_PREFIX);
        if holder.is_some() || entry.kind == DecisionKind::Dispatch {
            held.retain(|(recorded, _)| *recorded != task);
        }
        if let Some(holder) = holder {
            held.push((task, holder));
        }
    }
    held.into_iter()
        .map(|(task, holder)| format!("{task} → {holder}"))
        .collect()
}

/// Decisions the expanded `Decisions` category lists.
///
/// An operator who opened the category came for the log, so it lists as much of
/// the snapshot as it reasonably can.
pub(super) const DETAIL_LIMIT: usize = 20;

// Keeping the cap under what a snapshot holds is what keeps the notice honest:
// a snapshot filled to its own limit still has entries left over for the notice
// to point at.
const _: () = assert!(DETAIL_LIMIT < super::DECISION_SNAPSHOT_LIMIT);

/// What the list says when it left entries out.
///
/// The wording carries no count. The log is append-only and what the UI holds
/// is a tail of it, so how many entries lie beyond the rendered ones is not
/// knowable here — only that they exist.
const MORE_HINT: &str = "older entries stay in the decision log";

/// The reason `agent_id`'s worker still needs a human decision, or `None`
/// once the Overseer has closed the loop on its own.
///
/// `LedgerPhase::Escalated` is terminal and never reverts on its own
/// (`overseer::ledger`) — including when the triage judge resolves the
/// exception itself (e.g. answering the worker's question), which logs a
/// `Hold` decision but leaves the phase exactly where the original
/// `--kind blocked` report set it (`overseer::triage::completion`). So the
/// ledger phase alone cannot distinguish "still needs a person" from
/// "the Overseer already handled it" — the most recent Escalate / Hold / Skip
/// decision for the entry's task is the tie-breaker, the same way `standoffs`
/// above tracks the latest state per task by walking the oldest-first log.
pub(super) fn blocked_reason(
    locale: Locale,
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    agent_id: &str,
) -> Option<String> {
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id && entry.phase == LedgerPhase::Escalated)?;
    let mut reason = Some(t(locale, "worker blocked").to_string());
    for decision in decisions {
        if decision.task.as_deref() != Some(entry.task_id.as_str()) {
            continue;
        }
        reason = match decision.kind {
            DecisionKind::Escalate => Some(decision.reason.clone()),
            DecisionKind::Hold | DecisionKind::Skip => None,
            _ => reason,
        };
    }
    reason
}

/// Where `agent_id`'s pull request stands, read straight off its ledger
/// entry — no decision-log tie-breaker needed, unlike [`blocked_reason`]:
/// `merge_hold` is live and self-clearing on the very next auto-merge pass,
/// so the entry's current state already says what is true right now.
///
/// `None` covers both "no pull request open for this agent" (the entry is
/// missing, or in a phase before or after `PrOpened`) and "checks are clean
/// and no hold is recorded, i.e. the pull request already merged" — both
/// fall back to the plain session-status glyph. A pull request that merged
/// is `LedgerPhase::Merged`, not `PrOpened`, so it is excluded here on
/// purpose: it is genuinely finished and keeps its ordinary `Status::Done`
/// glyph rather than a lifecycle glyph.
pub(super) fn merge_lifecycle(ledger: &Ledger, agent_id: &str) -> Option<MergeLifecycle> {
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id)?;
    if entry.merge_approval.is_some() && !crate::overseer::ledger::terminal(entry.phase) {
        return Some(MergeLifecycle::ApprovedWaiting);
    }
    if entry.phase != LedgerPhase::PrOpened {
        return None;
    }
    Some(match entry.merge_hold.reason.as_deref() {
        Some("checks_waiting") => MergeLifecycle::ChecksRunning,
        Some("checks_not_green") => MergeLifecycle::ChecksFailing,
        Some(reason) if reason.starts_with("merge_error:") => MergeLifecycle::ChecksFailing,
        // Every gate exit — clearing all the way through to a merge, or
        // halting with a recorded reason — resolves within the same pass
        // (`overseer::daemon::merge_evaluate::evaluate`), so a `PrOpened`
        // entry with no hold reason recorded is not a state the ledger is
        // ever saved in. Defensive fallback only.
        Some(_) | None => MergeLifecycle::OnHold,
    })
}

/// Whether `agent_id`'s worker has reported `--kind done` and the entry has
/// not since settled. The report is the worker's own claim, not proof —
/// `Merged` / `Failed` / `Escalated` are read from the ledger's own observed
/// phase elsewhere, never from this — so this only lets a row distinguish
/// "still working" from "says it is finished and this is waiting on an
/// operator or the merge gate".
pub(super) fn worker_finished(ledger: &Ledger, agent_id: &str) -> bool {
    ledger.entries.iter().any(|entry| {
        entry.agent_id == agent_id
            && entry.worker_finished_at.is_some()
            && !crate::overseer::ledger::terminal(entry.phase)
    })
}

/// The raw gate reason behind `merge_lifecycle`'s bucket, for the Info pane.
/// Deliberately verbatim rather than remapped to a friendlier label — the
/// same convention `blocked_reason` and the decision list already follow for
/// operator-facing reason text (see `crate::ui::inbox`).
pub(super) fn merge_hold_detail(locale: Locale, ledger: &Ledger, agent_id: &str) -> Option<String> {
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id)?;
    if entry.merge_approval.is_some() && !crate::overseer::ledger::terminal(entry.phase) {
        return Some(t(locale, "approval queued; waiting for the merge gate").to_string());
    }
    if entry.phase != LedgerPhase::PrOpened {
        return None;
    }
    Some(
        entry
            .merge_hold
            .reason
            .clone()
            .unwrap_or_else(|| t(locale, "waiting on the merge gate").to_string()),
    )
}

/// Source stamped on the decision `daemon::discord_events` writes to mirror a
/// phase change into Discord. Its reason is the phase's own name
/// (`task_failed`, `task_escalated`), never why the entry stopped, and it is
/// appended *after* the pass that recorded the real reason — so a newest-wins
/// read has to skip it, or every reason ends up hidden behind its own label.
const PHASE_MIRROR_SOURCE: &str = "daemon_event";

/// Why `agent_id`'s ledger entry stopped in a terminal phase the operator did
/// not ask for — `Failed` or `Escalated` — as one short English sentence for
/// the agent's own tree row (dropr:524). `None` for an agent with no entry,
/// for one still live, and for one that merged: `Merged` is terminal too, but
/// it is the phase the work was dispatched to reach, so it has nothing to
/// explain.
///
/// The phase itself carries no reason — `LedgerEntry` has no such field — so
/// the text comes from the decision log, where the pass that moved the entry
/// wrote one: `monitor::apply::escalate` through `Action::Escalate`, and
/// `monitor::apply::fail` through `Action::LogDecision`, which
/// `logging::log_message` records as a `Hold`. Each phase therefore reads its
/// own decision kind, and the newest one wins — the same walk over this
/// oldest-first log that [`standoffs`] and [`blocked_reason`] already do.
///
/// Unlike [`blocked_reason`], a later `Hold` does not clear the answer. That
/// function asks whether a person is still needed, which the Overseer can
/// answer for itself; this asks where the entry ended up, and a terminal
/// phase never reverts (`overseer::ledger::terminal`).
///
/// The reason is humanized through the same table the Inbox row uses
/// (`ui::overseer::inbox_rows::row_reason`), so one condition reads the same
/// wherever it is shown. It stays English, like every other row-level string
/// — see the workspace localization policy.
pub(super) fn terminal_reason(
    ledger: &Ledger,
    decisions: &[DecisionEntry],
    agent_id: &str,
) -> Option<String> {
    let entry = ledger
        .entries
        .iter()
        .find(|entry| entry.agent_id == agent_id)?;
    let (kind, fallback) = match entry.phase {
        LedgerPhase::Failed => (DecisionKind::Hold, "task_failed"),
        LedgerPhase::Escalated => (DecisionKind::Escalate, "task_escalated"),
        _ => return None,
    };
    let logged = decisions
        .iter()
        .filter(|decision| {
            decision.task.as_deref() == Some(entry.task_id.as_str())
                && decision.kind == kind
                && decision.source.as_deref() != Some(PHASE_MIRROR_SOURCE)
        })
        .map(|decision| decision.reason.trim())
        .rfind(|reason| !reason.is_empty());
    // The snapshot holds only a tail of the log (`DECISION_SNAPSHOT_LIMIT`),
    // so an old enough entry has no decision of its own left in it. The
    // fallback is the phase's own event code, which the same table turns into
    // a sentence: it says less than the reason would, but it still says the
    // row stopped.
    let reason = logged.unwrap_or(fallback);
    Some(
        humanize::static_sentence(reason)
            .unwrap_or(reason)
            .to_string(),
    )
}

pub(super) fn append_decisions(
    lines: &mut Vec<Line<'static>>,
    decisions: &[DecisionEntry],
    locale: Locale,
) {
    lines.push(Line::from(Span::styled(
        t(locale, "recent decisions"),
        THEME.accent_bold_style(),
    )));
    // `decisions` is oldest-first (see `logging::tail`); show the newest first.
    let recent = decisions
        .iter()
        .rev()
        .take(DETAIL_LIMIT)
        .collect::<Vec<_>>();
    if recent.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", t(locale, "none")),
            THEME.muted_style(),
        )));
        return;
    }
    let truncated = decisions.len() > recent.len();
    for entry in recent {
        let task = entry.task.as_deref().unwrap_or("-");
        let label = match entry.kind {
            DecisionKind::Escalate | DecisionKind::CircuitOpen => "!",
            _ => "·",
        };
        lines.push(Line::from(format!(
            "  {label} {} {task} — {}",
            entry.at.format("%m-%d %H:%M"),
            entry.reason
        )));
    }
    if truncated {
        lines.push(Line::from(Span::styled(
            format!("  {}", t(locale, MORE_HINT)),
            THEME.muted_style(),
        )));
    }
}

#[cfg(test)]
#[path = "decisions_tests.rs"]
mod tests;

// `terminal_reason`'s own file: `decisions_tests.rs` is already at the source
// size limit, the same reason `decisions_standoffs_tests.rs` was split out.
#[cfg(test)]
#[path = "decisions_terminal_tests.rs"]
mod terminal_tests;
