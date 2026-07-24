//! The dropr task lists in a repository summary.
//!
//! A capped list that says it is capped is fine; a capped list that looks
//! complete misinforms. Both lists here are capped — by the display limit
//! below, and before that by what the fetch was willing to ask for — so both
//! end with a line naming what they left out.

use ratatui::text::{Line, Span};

use crate::{
    dropr::{DroprTaskCandidate, IN_PROGRESS_FETCH_LIMIT, READY_FETCH_LIMIT},
    ui::theme::DEFAULT as THEME,
};

/// Task rows either list gets before the rest are counted instead of listed.
///
/// The DROPR section is the last thing in the summary and the preview pane
/// scrolls, so a longer list pushes nothing else off screen; the cap is about
/// keeping the panel scannable at a glance, not about fitting the terminal.
const TASK_DISPLAY_LIMIT: usize = 8;

// The truncation notice can only tell an exact remainder from a lower bound
// while the display cap bites before the fetch limit does.
const _: () = assert!(TASK_DISPLAY_LIMIT < READY_FETCH_LIMIT);
const _: () = assert!(TASK_DISPLAY_LIMIT < IN_PROGRESS_FETCH_LIMIT);

fn partition_tasks(
    tasks: &[DroprTaskCandidate],
) -> (Vec<&DroprTaskCandidate>, Vec<&DroprTaskCandidate>) {
    tasks.iter().partition(|task| task.status == "in_progress")
}

pub(super) fn dropr_task_lines(tasks: &[DroprTaskCandidate]) -> Vec<Line<'static>> {
    let (in_progress, next) = partition_tasks(tasks);
    let mut lines = task_section(
        Span::styled("next tasks", THEME.accent_style()),
        &next,
        READY_FETCH_LIMIT,
        |task| format!("{}  {}", task.display_id, task.title),
    );
    lines.extend(task_section(
        Span::styled("in progress", THEME.subagent_style()),
        &in_progress,
        IN_PROGRESS_FETCH_LIMIT,
        |task| format!("▸ {}  {}", task.display_id, task.title),
    ));
    lines
}

fn task_section(
    heading: Span<'static>,
    tasks: &[&DroprTaskCandidate],
    fetch_limit: usize,
    row: impl Fn(&DroprTaskCandidate) -> String,
) -> Vec<Line<'static>> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(""), Line::from(heading)];
    lines.extend(
        tasks
            .iter()
            .take(TASK_DISPLAY_LIMIT)
            .map(|task| Line::from(row(task))),
    );
    if let Some(notice) = truncation_notice(tasks.len(), fetch_limit) {
        lines.push(Line::from(Span::styled(notice, THEME.muted_style())));
    }
    lines
}

/// The line that keeps a capped list from reading as a complete one.
///
/// `held` is what the panel actually has; a fetch that came back full may have
/// left more behind, so the remainder it can report is a floor rather than a
/// count and the wording says so.
fn truncation_notice(held: usize, fetch_limit: usize) -> Option<String> {
    let hidden = held.saturating_sub(TASK_DISPLAY_LIMIT);
    if hidden == 0 {
        return None;
    }
    Some(if held >= fetch_limit {
        format!("… and at least {hidden} more")
    } else {
        format!("… and {hidden} more")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Text;

    fn task(display_id: &str, status: &str) -> DroprTaskCandidate {
        DroprTaskCandidate {
            display_id: display_id.to_string(),
            title: format!("Task {display_id}"),
            priority: String::new(),
            status: status.to_string(),
        }
    }

    fn tasks(count: usize, status: &str) -> Vec<DroprTaskCandidate> {
        (0..count)
            .map(|index| task(&format!("#{index}"), status))
            .collect()
    }

    fn rendered_lines(tasks: &[DroprTaskCandidate]) -> Vec<String> {
        let text: Text<'static> = dropr_task_lines(tasks).into();
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn partitions_in_progress_from_next_tasks() {
        let tasks = [task("#1", "in_progress"), task("#2", "ready")];
        let (in_progress, next) = partition_tasks(&tasks);
        assert_eq!(in_progress[0].display_id, "#1");
        assert_eq!(next[0].display_id, "#2");
    }

    #[test]
    fn renders_in_progress_after_next_tasks() {
        let lines = rendered_lines(&[task("#2", "ready"), task("#1", "in_progress")]);
        let in_progress = lines.iter().position(|line| line == "in progress").unwrap();
        let next = lines.iter().position(|line| line == "next tasks").unwrap();
        assert!(next < in_progress);
        assert!(lines.iter().any(|line| line == "▸ #1  Task #1"));
    }

    #[test]
    fn omits_in_progress_heading_without_matching_tasks() {
        let lines = rendered_lines(&[task("#2", "ready")]);
        assert!(!lines.iter().any(|line| line == "in progress"));
        assert!(lines.iter().any(|line| line == "next tasks"));
    }

    #[test]
    fn lists_more_than_three_ready_tasks() {
        let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT, "ready"));
        assert_eq!(
            lines.iter().filter(|line| line.starts_with('#')).count(),
            TASK_DISPLAY_LIMIT
        );
    }

    #[test]
    fn a_complete_list_carries_no_notice() {
        let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT, "ready"));
        assert!(!lines.iter().any(|line| line.starts_with('…')));
    }

    #[test]
    fn a_truncated_list_counts_what_it_left_out() {
        let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT + 2, "ready"));
        assert!(lines.iter().any(|line| line == "… and 2 more"));
    }

    #[test]
    fn in_progress_truncation_is_reported_too() {
        // Both lists truncated silently before; the notice belongs to both.
        let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT + 1, "in_progress"));
        assert!(lines.iter().any(|line| line == "… and 1 more"));
    }

    #[test]
    fn a_saturated_fetch_reports_a_floor_not_a_count() {
        // The fetch came back full, so tasks beyond it never reached the panel
        // and the remainder it can see is a lower bound.
        let lines = rendered_lines(&tasks(READY_FETCH_LIMIT, "ready"));
        let hidden = READY_FETCH_LIMIT - TASK_DISPLAY_LIMIT;
        assert!(
            lines
                .iter()
                .any(|line| line == &format!("… and at least {hidden} more"))
        );
    }
}
