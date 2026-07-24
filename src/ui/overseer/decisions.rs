//! Tasks the overseer is standing off because another agent holds their claim.
//!
//! An overseer that declines to dispatch looks exactly like an idle one from
//! the outside. Reading the stand-offs back out of the decision log is what
//! lets the OVERSEER frame tell the operator which is which.

use ratatui::text::{Line, Span};

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

pub(super) fn append_decisions(lines: &mut Vec<Line<'static>>, decisions: &[DecisionEntry]) {
    lines.push(Line::from(Span::styled(
        "recent decisions",
        THEME.accent_bold_style(),
    )));
    // `decisions` is oldest-first (see `logging::tail`); show the newest three first.
    let recent = decisions.iter().rev().take(3).collect::<Vec<_>>();
    if recent.is_empty() {
        lines.push(Line::from(Span::styled("  none", THEME.muted_style())));
        return;
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(kind: DecisionKind, task: &str, reason: &str) -> DecisionEntry {
        let mut entry = DecisionEntry::new(kind, reason);
        entry.task = Some(task.into());
        entry
    }

    #[test]
    fn an_external_claim_names_the_holder() {
        let decisions = [decision(
            DecisionKind::Hold,
            "#216",
            "claimed_elsewhere:manual-run",
        )];
        assert_eq!(standoffs(&decisions), ["#216 → manual-run"]);
    }

    #[test]
    fn a_later_dispatch_clears_the_standoff() {
        // The operator's manual run finished and the overseer picked the task
        // up; the frame must stop reporting a stand-off that ended.
        let decisions = [
            decision(DecisionKind::Hold, "#216", "claimed_elsewhere:manual-run"),
            decision(DecisionKind::Dispatch, "#216", "worker spawned"),
        ];
        assert!(standoffs(&decisions).is_empty());
    }

    #[test]
    fn a_repeated_standoff_is_reported_once() {
        let decisions = [
            decision(DecisionKind::Hold, "#216", "claimed_elsewhere:manual-run"),
            decision(DecisionKind::Hold, "#216", "claimed_elsewhere:other-agent"),
        ];
        assert_eq!(standoffs(&decisions), ["#216 → other-agent"]);
    }

    #[test]
    fn unrelated_decisions_are_ignored() {
        let decisions = [
            decision(DecisionKind::Skip, "#216", "daily_limit"),
            DecisionEntry::new(DecisionKind::Hold, "claimed_elsewhere:no-task"),
        ];
        assert!(standoffs(&decisions).is_empty());
    }
}
