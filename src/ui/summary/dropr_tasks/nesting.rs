//! Nesting a root task's subtasks under its own row (dropr:442).
//!
//! `dropr::repo_tasks` fetches roots and their subtasks flattened into one
//! `Vec<DroprTaskCandidate>`; without this module the panel rendered every
//! subtask a second time as though it were unrelated open work. This module
//! is what turns that flat list back into parent/child rows.

use std::collections::{HashMap, HashSet};

use ratatui::text::{Line, Span};

use crate::{dropr::DroprTaskCandidate, locale::Locale, ui::theme::DEFAULT as THEME};

use super::{TASK_DISPLAY_LIMIT, squash_reason, truncation_notice};

/// Whether `task` is a root, i.e. not nested under another task. A subtask
/// carries its parent's nanoid in `parent_task_id`; a root carries `None` or
/// (from `dropr task ready --json`, which never populates the field) an empty
/// string.
pub(super) fn is_root(task: &DroprTaskCandidate) -> bool {
    task.parent_task_id.as_deref().unwrap_or("").is_empty()
}

/// Groups every fetched subtask under its parent's nanoid.
///
/// `fetch.tasks` holds roots and subtasks flattened into one list, so this is
/// what lets a root row find the children it should nest instead of the panel
/// listing every subtask a second time as though it were independent work.
pub(super) fn children_by_parent(
    tasks: &[DroprTaskCandidate],
) -> HashMap<&str, Vec<&DroprTaskCandidate>> {
    let mut children: HashMap<&str, Vec<&DroprTaskCandidate>> = HashMap::new();
    for task in tasks {
        if let Some(parent) = task.parent_task_id.as_deref().filter(|id| !id.is_empty()) {
            children.entry(parent).or_default().push(task);
        }
    }
    children
}

/// A root's nested subtask rows: a closed/total fraction, then the fetched
/// children themselves.
///
/// Only rendered when the fetch actually queried this root's subtree —
/// `subtrees_known` is what tells a genuinely childless answer (all subtasks
/// closed, filtered out of every fetched status) apart from a walk that
/// skipped or failed this root and simply does not know. Rendering a fraction
/// in the latter case would look precise while being no better than a guess,
/// which is exactly what this pane exists to avoid.
pub(super) fn nested_lines(
    locale: Locale,
    task: &DroprTaskCandidate,
    children: &HashMap<&str, Vec<&DroprTaskCandidate>>,
    subtrees_known: &HashSet<String>,
) -> Vec<Line<'static>> {
    if task.child_count == 0 || !subtrees_known.contains(task.id.as_str()) {
        return Vec::new();
    }
    let kids = children
        .get(task.id.as_str())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    // Closed subtasks are filtered out of every fetch (`DISPLAY_STATUSES`), so
    // the gap between the known total and what actually came back is exactly
    // how many are done — this is a row-level summary value, so it stays
    // English regardless of locale (dropr:388).
    let closed = task.child_count.saturating_sub(kids.len());
    let mut lines = vec![Line::from(Span::styled(
        format!("  {closed}/{} closed", task.child_count),
        THEME.muted_style(),
    ))];
    for child in kids.iter().take(TASK_DISPLAY_LIMIT) {
        lines.extend(child_rows(child));
    }
    if let Some(notice) = truncation_notice(locale, kids.len()) {
        lines.push(Line::from(Span::styled(
            format!("  {notice}"),
            THEME.muted_style(),
        )));
    }
    lines
}

/// One subtask's indented row, styled the same way its status would read at
/// the top level so an in-progress or blocked child is not mistaken for
/// available work. A blocked child also carries its blocker reason, same as a
/// top-level blocked row.
fn child_rows(task: &DroprTaskCandidate) -> Vec<Line<'static>> {
    match task.status.as_str() {
        "in_progress" => vec![Line::from(Span::styled(
            format!("    ▸ {}  {}", task.display_id, task.title),
            THEME.subagent_style(),
        ))],
        "blocked" => {
            let mut lines = vec![Line::from(Span::styled(
                format!("    ✖ {}  {}", task.display_id, task.title),
                THEME.needs_decision_style(false),
            ))];
            if let Some(reason) = task
                .blocked_reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
            {
                lines.push(Line::from(Span::styled(
                    format!("      {}", squash_reason(reason)),
                    THEME.muted_style(),
                )));
            }
            lines
        }
        _ => vec![Line::from(format!(
            "    {}  {}",
            task.display_id, task.title
        ))],
    }
}

#[cfg(test)]
#[path = "nesting_tests.rs"]
mod tests;
