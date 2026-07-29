//! The dropr task lists in a repository summary.
//!
//! A capped list that says it is capped is fine; a capped list that looks
//! complete misinforms. The same goes for a list that is short because the
//! fetch failed. So this panel never renders rows alone: it renders rows plus
//! whatever the fetch could not answer, and it distinguishes an empty board
//! from a broken one.

use ratatui::text::{Line, Span};

use crate::{
    dropr::{DroprTaskCandidate, DroprTaskFetch, TASK_FETCH_LIMIT},
    ui::theme::DEFAULT as THEME,
};

/// Task rows either list gets before the rest are counted instead of listed.
///
/// The DROPR section is the last thing in the summary and the preview pane
/// scrolls, so a longer list pushes nothing else off screen; the cap is about
/// keeping the panel scannable at a glance, not about fitting the terminal.
/// The fetch orders rows by priority, so what the cap drops is the least urgent.
const TASK_DISPLAY_LIMIT: usize = 8;

// The truncation notice can only tell an exact remainder from a lower bound
// while the display cap bites before the fetch limit does.
const _: () = assert!(TASK_DISPLAY_LIMIT < TASK_FETCH_LIMIT);

/// Splits the rows into the panel's three sections.
///
/// `blocked` is split off first and never folded in with `next`: a blocked
/// task is not available work, and rendering it as though it were would
/// reintroduce the exact confusion the `blocked` status exists to remove.
/// What is left splits on `in_progress`, the distinction an operator acts on
/// for everything that remains — work already running versus work waiting to
/// be picked up. All three sections come from the same `task_list` query, so
/// membership is decided by the task's own status rather than by which
/// endpoint happened to return it.
fn partition_tasks(
    tasks: &[DroprTaskCandidate],
) -> (
    Vec<&DroprTaskCandidate>,
    Vec<&DroprTaskCandidate>,
    Vec<&DroprTaskCandidate>,
) {
    let mut blocked = Vec::new();
    let mut in_progress = Vec::new();
    let mut next = Vec::new();
    for task in tasks {
        match task.status.as_str() {
            "blocked" => blocked.push(task),
            "in_progress" => in_progress.push(task),
            _ => next.push(task),
        }
    }
    (blocked, in_progress, next)
}

pub(super) fn dropr_task_lines(fetch: &DroprTaskFetch) -> Vec<Line<'static>> {
    if !fetch.answered {
        // No query answered, so there are no rows to qualify — showing an empty
        // list here would read as "this workspace has no tasks".
        return problem_lines("tasks unavailable", &fetch.problems);
    }

    let (blocked, in_progress, next) = partition_tasks(&fetch.tasks);
    let mut lines = task_section(
        Span::styled("next tasks", THEME.accent_style()),
        &next,
        |task| vec![Line::from(format!("{}  {}", task.display_id, task.title))],
    );
    lines.extend(task_section(
        Span::styled("in progress", THEME.subagent_style()),
        &in_progress,
        |task| vec![Line::from(format!("▸ {}  {}", task.display_id, task.title))],
    ));
    lines.extend(task_section(
        Span::styled("blocked", THEME.needs_decision_style(false)),
        &blocked,
        blocked_row,
    ));
    if lines.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "no open, in-progress, or blocked tasks",
            THEME.muted_style(),
        )));
    }
    if !fetch.problems.is_empty() {
        lines.extend(problem_lines("this list is incomplete", &fetch.problems));
    }
    lines
}

/// One blocked task's row: the task itself, styled so it reads as needing a
/// decision rather than as available work, plus — when the fetch found one —
/// the reason from its `blocker` scribble. That second line is what makes the
/// unblock condition reachable without leaving robco for dropr.
fn blocked_row(task: &DroprTaskCandidate) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("✖ {}  {}", task.display_id, task.title),
        THEME.needs_decision_style(false),
    ))];
    if let Some(reason) = task
        .blocked_reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
    {
        lines.push(Line::from(Span::styled(
            format!("  {}", squash_reason(reason)),
            THEME.muted_style(),
        )));
    }
    lines
}

/// Longest reason line the panel will hold before it stops being scannable.
const REASON_DISPLAY_LIMIT: usize = 100;

/// Squashes a scribble body onto the single line a blocked row can spend on it.
fn squash_reason(reason: &str) -> String {
    reason
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(REASON_DISPLAY_LIMIT)
        .collect()
}

/// The block that keeps a short list from reading as a whole one.
fn problem_lines(heading: &str, problems: &[String]) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(heading.to_string(), THEME.failure_style())),
    ];
    lines.extend(
        problems
            .iter()
            .map(|problem| Line::from(Span::styled(format!("! {problem}"), THEME.failure_style()))),
    );
    lines
}

fn task_section(
    heading: Span<'static>,
    tasks: &[&DroprTaskCandidate],
    row: impl Fn(&DroprTaskCandidate) -> Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(""), Line::from(heading)];
    lines.extend(
        tasks
            .iter()
            .take(TASK_DISPLAY_LIMIT)
            .flat_map(|task| row(task)),
    );
    if let Some(notice) = truncation_notice(tasks.len()) {
        lines.push(Line::from(Span::styled(notice, THEME.muted_style())));
    }
    lines
}

/// The line that keeps a capped list from reading as a complete one.
///
/// `held` is what the panel actually has; a fetch that came back full may have
/// left more behind, so the remainder it can report is a floor rather than a
/// count and the wording says so.
fn truncation_notice(held: usize) -> Option<String> {
    let hidden = held.saturating_sub(TASK_DISPLAY_LIMIT);
    if hidden == 0 {
        return None;
    }
    Some(if held >= TASK_FETCH_LIMIT {
        format!("… and at least {hidden} more")
    } else {
        format!("… and {hidden} more")
    })
}

#[cfg(test)]
#[path = "dropr_tasks_tests.rs"]
mod tests;
