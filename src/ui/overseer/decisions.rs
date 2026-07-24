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
const MORE_HINT: &str = "  older entries stay in the decision log";

pub(super) fn append_decisions(lines: &mut Vec<Line<'static>>, decisions: &[DecisionEntry]) {
    lines.push(Line::from(Span::styled(
        "recent decisions",
        THEME.accent_bold_style(),
    )));
    // `decisions` is oldest-first (see `logging::tail`); show the newest first.
    let recent = decisions
        .iter()
        .rev()
        .take(DETAIL_LIMIT)
        .collect::<Vec<_>>();
    if recent.is_empty() {
        lines.push(Line::from(Span::styled("  none", THEME.muted_style())));
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
        lines.push(Line::from(Span::styled(MORE_HINT, THEME.muted_style())));
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

    fn decisions(count: usize) -> Vec<DecisionEntry> {
        (0..count)
            .map(|index| {
                decision(
                    DecisionKind::Dispatch,
                    &format!("#{index}"),
                    "worker spawned",
                )
            })
            .collect()
    }

    fn rendered(decisions: &[DecisionEntry]) -> Vec<String> {
        let mut lines = Vec::new();
        append_decisions(&mut lines, decisions);
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    fn entry_rows(lines: &[String]) -> usize {
        lines
            .iter()
            .filter(|line| line.starts_with("  · ") || line.starts_with("  ! "))
            .count()
    }

    #[test]
    fn a_complete_list_carries_no_notice() {
        let lines = rendered(&decisions(DETAIL_LIMIT));
        assert_eq!(entry_rows(&lines), DETAIL_LIMIT);
        assert!(!lines.iter().any(|line| line.contains("older entries")));
    }

    #[test]
    fn an_empty_log_says_none_and_stops() {
        let lines = rendered(&[]);
        assert_eq!(lines, ["recent decisions", "  none"]);
    }

    #[test]
    fn a_truncated_list_points_at_the_log_without_counting() {
        // A snapshot filled to its own limit says nothing about how much
        // history sits behind it, so the notice states only that it exists.
        let lines = rendered(&decisions(super::super::DECISION_SNAPSHOT_LIMIT));
        assert_eq!(entry_rows(&lines), DETAIL_LIMIT);
        assert_eq!(
            lines.last().unwrap(),
            "  older entries stay in the decision log"
        );
        assert!(!lines.iter().any(|line| line.contains("more")));
    }

    #[test]
    fn the_newest_decision_is_listed_first() {
        let lines = rendered(&decisions(DETAIL_LIMIT + 1));
        assert!(lines[1].contains(&format!("#{}", DETAIL_LIMIT)));
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
