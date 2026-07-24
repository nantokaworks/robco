//! What the OVERSEER frame reads back out of the decision log.
//!
//! Stand-offs first: an overseer that declines to dispatch looks exactly like
//! an idle one from the outside, and reading the stand-offs out of the log is
//! what lets the frame tell the operator which is which.
//!
//! Then the decision lists themselves. Both are capped — the log is
//! append-only and what the UI holds is only a tail of it — so both end by
//! saying they are capped rather than letting the newest few read as the whole
//! history.

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

/// Decisions the OVERSEER summary lists inline.
///
/// The summary shares the pane with the health, ledger, and inbox blocks, so
/// its decision list stays short. The `Decisions` category is the longer view.
const SUMMARY_LIMIT: usize = 5;

/// Decisions the expanded `Decisions` category lists.
///
/// An operator who opened the category came for the log, so it lists as much of
/// the snapshot as it reasonably can rather than repeating the summary.
pub(super) const DETAIL_LIMIT: usize = 20;

// Sending an operator to a category that shows no more than the summary did
// would be a dead end.
const _: () = assert!(SUMMARY_LIMIT < DETAIL_LIMIT);
// Keeping the detail cap under what a snapshot holds is what keeps its notice
// honest: a snapshot filled to its own limit still has entries left over for
// the notice to point at.
const _: () = assert!(DETAIL_LIMIT < super::DECISION_SNAPSHOT_LIMIT);

/// Which of the two decision lists is being rendered. They differ in how many
/// entries they show and in where they send an operator who wants the rest.
#[derive(Clone, Copy)]
pub(super) enum DecisionList {
    /// The decision block at the foot of the OVERSEER summary.
    Summary,
    /// The `Decisions` category detail, which doubles as its preview.
    Detail,
}

impl DecisionList {
    fn limit(self) -> usize {
        match self {
            Self::Summary => SUMMARY_LIMIT,
            Self::Detail => DETAIL_LIMIT,
        }
    }

    /// What the list says when it left entries out.
    ///
    /// Neither wording carries a count. The log is append-only and what the UI
    /// holds is a tail of it, so how many entries lie beyond the rendered ones
    /// is not knowable here — only that they exist.
    fn more_hint(self) -> &'static str {
        match self {
            Self::Summary => "  older entries under Decisions",
            Self::Detail => "  older entries stay in the decision log",
        }
    }
}

pub(super) fn append_decisions(
    lines: &mut Vec<Line<'static>>,
    decisions: &[DecisionEntry],
    list: DecisionList,
) {
    lines.push(Line::from(Span::styled(
        "recent decisions",
        THEME.accent_bold_style(),
    )));
    // `decisions` is oldest-first (see `logging::tail`); show the newest first.
    let recent = decisions
        .iter()
        .rev()
        .take(list.limit())
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
        lines.push(Line::from(Span::styled(
            list.more_hint(),
            THEME.muted_style(),
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

    fn rendered(decisions: &[DecisionEntry], list: DecisionList) -> Vec<String> {
        let mut lines = Vec::new();
        append_decisions(&mut lines, decisions, list);
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
    fn the_summary_lists_more_than_three_decisions() {
        let lines = rendered(&decisions(SUMMARY_LIMIT), DecisionList::Summary);
        assert_eq!(entry_rows(&lines), SUMMARY_LIMIT);
    }

    #[test]
    fn a_complete_list_carries_no_notice() {
        let lines = rendered(&decisions(SUMMARY_LIMIT), DecisionList::Summary);
        assert!(!lines.iter().any(|line| line.contains("older entries")));
    }

    #[test]
    fn an_empty_log_says_none_and_stops() {
        let lines = rendered(&[], DecisionList::Detail);
        assert_eq!(lines, ["recent decisions", "  none"]);
    }

    #[test]
    fn a_truncated_summary_names_where_the_rest_is() {
        // The defect: the summary stopped after its cap and nothing on screen
        // said the log held more, let alone how to reach it.
        let lines = rendered(&decisions(SUMMARY_LIMIT + 1), DecisionList::Summary);
        assert_eq!(entry_rows(&lines), SUMMARY_LIMIT);
        assert_eq!(lines.last().unwrap(), "  older entries under Decisions");
    }

    #[test]
    fn the_detail_list_goes_deeper_than_the_summary() {
        let held = decisions(DETAIL_LIMIT);
        assert_eq!(
            entry_rows(&rendered(&held, DecisionList::Detail)),
            DETAIL_LIMIT
        );
        assert_eq!(
            entry_rows(&rendered(&held, DecisionList::Summary)),
            SUMMARY_LIMIT
        );
    }

    #[test]
    fn a_truncated_detail_list_points_at_the_log_without_counting() {
        // A snapshot filled to its own limit says nothing about how much
        // history sits behind it, so the notice states only that it exists.
        let lines = rendered(
            &decisions(super::super::DECISION_SNAPSHOT_LIMIT),
            DecisionList::Detail,
        );
        assert_eq!(
            lines.last().unwrap(),
            "  older entries stay in the decision log"
        );
        assert!(!lines.iter().any(|line| line.contains("more")));
    }

    #[test]
    fn the_newest_decision_is_listed_first() {
        let lines = rendered(&decisions(SUMMARY_LIMIT + 1), DecisionList::Summary);
        assert!(lines[1].contains(&format!("#{}", SUMMARY_LIMIT)));
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
