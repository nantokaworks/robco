//! The dropr task lists in a repository summary.
//!
//! A capped list that says it is capped is fine; a capped list that looks
//! complete misinforms. The same goes for a list that is short because the
//! fetch failed. So this panel never renders rows alone: it renders rows plus
//! whatever the fetch could not answer, and it distinguishes an empty board
//! from a broken one.

use std::collections::{HashMap, HashSet};

use ratatui::text::{Line, Span};

use crate::{
    dropr::{DroprTaskCandidate, DroprTaskFetch, TASK_FETCH_LIMIT},
    locale::{Locale, fmt, t},
    ui::theme::DEFAULT as THEME,
};

pub(super) mod body;
mod nesting;
use nesting::{children_by_parent, is_root, nested_lines};

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

/// Splits the root rows into the panel's three sections.
///
/// `blocked` is split off first and never folded in with `next`: a blocked
/// task is not available work, and rendering it as though it were would
/// reintroduce the exact confusion the `blocked` status exists to remove.
/// What is left splits on `in_progress`, the distinction an operator acts on
/// for everything that remains — work already running versus work waiting to
/// be picked up. All three sections come from the same `task_list` query, so
/// membership is decided by the task's own status rather than by which
/// endpoint happened to return it.
///
/// Subtasks never appear here directly: they are only ever reached by nesting
/// under their parent's row (see `nesting::nested_lines`), so a subtask's own
/// status never puts it in a section of its own.
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
    for task in tasks.iter().filter(|task| is_root(task)) {
        match task.status.as_str() {
            "blocked" => blocked.push(task),
            "in_progress" => in_progress.push(task),
            _ => next.push(task),
        }
    }
    (blocked, in_progress, next)
}

/// Root tasks the operator can select and launch, in the exact order and cap
/// [`dropr_task_lines`] renders them: next tasks, then in-progress, then
/// blocked, each capped at [`TASK_DISPLAY_LIMIT`]. This is the single source
/// of truth the dropr task drill-down's `task` index counts against
/// (`ui::DroprTaskFocus`, dropr:475) — both this pane's own highlighting and
/// `ui::actions::dropr_task_drill` (walking the list, opening a body,
/// launching it) read this same list.
pub(in crate::ui) fn selectable_tasks(fetch: &DroprTaskFetch) -> Vec<&DroprTaskCandidate> {
    if !fetch.answered {
        return Vec::new();
    }
    let (blocked, in_progress, next) = partition_tasks(&fetch.tasks);
    [next, in_progress, blocked]
        .into_iter()
        .flat_map(|section| section.into_iter().take(TASK_DISPLAY_LIMIT))
        .collect()
}

pub(super) fn dropr_task_lines(
    fetch: &DroprTaskFetch,
    locale: Locale,
    selected_task: Option<usize>,
) -> Vec<Line<'static>> {
    if !fetch.answered {
        // No query answered, so there are no rows to qualify — showing an empty
        // list here would read as "this workspace has no tasks".
        return problem_lines(t(locale, "tasks unavailable"), &fetch.problems);
    }

    let (blocked, in_progress, next) = partition_tasks(&fetch.tasks);
    let children = children_by_parent(&fetch.tasks);
    let next_base = 0;
    let in_progress_base = next_base + next.len().min(TASK_DISPLAY_LIMIT);
    let blocked_base = in_progress_base + in_progress.len().min(TASK_DISPLAY_LIMIT);
    let mut lines = task_section(
        locale,
        Span::styled(t(locale, "next tasks"), THEME.accent_style()),
        &next,
        &children,
        &fetch.subtrees_known,
        next_base,
        selected_task,
        |task, selected| {
            vec![selectable_row(
                task.display_id.clone(),
                &task.title,
                selected,
            )]
        },
    );
    lines.extend(task_section(
        locale,
        Span::styled(t(locale, "in progress"), THEME.subagent_style()),
        &in_progress,
        &children,
        &fetch.subtrees_known,
        in_progress_base,
        selected_task,
        |task, selected| {
            vec![selectable_row(
                format!("▸ {}", task.display_id),
                &task.title,
                selected,
            )]
        },
    ));
    lines.extend(task_section(
        locale,
        Span::styled(t(locale, "blocked"), THEME.needs_decision_style(false)),
        &blocked,
        &children,
        &fetch.subtrees_known,
        blocked_base,
        selected_task,
        blocked_row,
    ));
    if lines.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            t(locale, "no open, in-progress, or blocked tasks"),
            THEME.muted_style(),
        )));
    }
    if !fetch.problems.is_empty() {
        lines.extend(problem_lines(
            t(locale, "this list is incomplete"),
            &fetch.problems,
        ));
    }
    lines
}

/// A selectable task row's line: `{prefix}  {title}`, styled to read as the
/// current cursor position when `selected`. `prefix` already carries any
/// section glyph (`▸`, `✖`) ahead of the display id, so this only ever
/// changes the row's style, never its text — a launched-but-unselected row
/// looks exactly as it always has.
fn selectable_row(prefix: impl Into<String>, title: &str, selected: bool) -> Line<'static> {
    let style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    Line::from(Span::styled(format!("{}  {title}", prefix.into()), style))
}

/// One blocked task's row: the task itself, styled so it reads as needing a
/// decision rather than as available work, plus — when the fetch found one —
/// the reason from its `blocker` scribble. That second line is what makes the
/// unblock condition reachable without leaving robco for dropr.
fn blocked_row(task: &DroprTaskCandidate, selected: bool) -> Vec<Line<'static>> {
    let style = if selected {
        THEME.selection_style()
    } else {
        THEME.needs_decision_style(false)
    };
    let mut lines = vec![Line::from(Span::styled(
        format!("✖ {}  {}", task.display_id, task.title),
        style,
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
///
/// `pub(super)` because `nesting::child_rows` renders a blocked subtask's
/// reason the same way a top-level blocked row does.
pub(super) fn squash_reason(reason: &str) -> String {
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

/// `base_index` is this section's offset into the flat selectable-task order
/// [`selectable_tasks`] builds, so `base_index + offset` (the row's position
/// within this section, capped the same way) is the `task` index a
/// `ui::DroprTaskFocus` on this row carries — see that function's doc
/// comment.
#[allow(clippy::too_many_arguments)]
fn task_section(
    locale: Locale,
    heading: Span<'static>,
    tasks: &[&DroprTaskCandidate],
    children: &HashMap<&str, Vec<&DroprTaskCandidate>>,
    subtrees_known: &HashSet<String>,
    base_index: usize,
    selected_task: Option<usize>,
    row: impl Fn(&DroprTaskCandidate, bool) -> Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(""), Line::from(heading)];
    for (offset, task) in tasks.iter().take(TASK_DISPLAY_LIMIT).enumerate() {
        let selected = selected_task == Some(base_index + offset);
        lines.extend(row(task, selected));
        lines.extend(nested_lines(locale, task, children, subtrees_known));
    }
    if let Some(notice) = truncation_notice(locale, tasks.len()) {
        lines.push(Line::from(Span::styled(notice, THEME.muted_style())));
    }
    lines
}

/// The line that keeps a capped list from reading as a complete one.
///
/// `held` is what the panel actually has; a fetch that came back full may have
/// left more behind, so the remainder it can report is a floor rather than a
/// count and the wording says so.
///
/// `pub(super)` because `nesting::nested_lines` reuses it verbatim for a
/// root's capped child list — the same "floor, not a count" reasoning applies
/// there, since a subtree query is itself capped at `TASK_FETCH_LIMIT`.
pub(super) fn truncation_notice(locale: Locale, held: usize) -> Option<String> {
    let hidden = held.saturating_sub(TASK_DISPLAY_LIMIT);
    if hidden == 0 {
        return None;
    }
    Some(if held >= TASK_FETCH_LIMIT {
        fmt(locale, "… and at least {} more", &[&hidden.to_string()])
    } else {
        fmt(locale, "… and {} more", &[&hidden.to_string()])
    })
}

#[cfg(test)]
#[path = "dropr_tasks_selection_tests.rs"]
mod selection_tests;
#[cfg(test)]
#[path = "dropr_tasks_tests.rs"]
mod tests;
