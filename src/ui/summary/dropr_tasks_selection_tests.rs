//! Selection/highlight coverage for `Selection::DroprTask` (dropr:470), split
//! out of `dropr_tasks_tests.rs` to keep that file under this project's
//! source file size limit.

use super::*;

fn task(display_id: &str, status: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        priority: String::new(),
        status: status.to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: String::new(),
        parent_task_id: None,
        child_count: 0,
    }
}

fn complete(tasks: Vec<DroprTaskCandidate>) -> DroprTaskFetch {
    DroprTaskFetch {
        tasks,
        problems: Vec::new(),
        answered: true,
        subtrees_known: Default::default(),
    }
}

/// True when the row whose text is `text` is styled as the current selection.
fn is_selected(fetch: &DroprTaskFetch, selected_task: Option<usize>, text: &str) -> bool {
    let lines: Vec<Line<'static>> = dropr_task_lines(fetch, Locale::En, selected_task);
    lines
        .iter()
        .find(|line| line.to_string() == text)
        .is_some_and(|line| {
            line.spans
                .iter()
                .any(|span| span.style == THEME.selection_style())
        })
}

#[test]
fn selectable_tasks_orders_next_then_in_progress_then_blocked_each_capped() {
    let fetch = complete(vec![
        task("#1", "blocked"),
        task("#2", "open"),
        task("#3", "in_progress"),
    ]);
    let ids: Vec<_> = selectable_tasks(&fetch)
        .into_iter()
        .map(|task| task.display_id.as_str())
        .collect();
    assert_eq!(ids, vec!["#2", "#3", "#1"]);
}

#[test]
fn a_selected_task_row_is_styled_as_the_current_selection() {
    let fetch = complete(vec![task("#250", "open")]);
    assert!(is_selected(&fetch, Some(0), "#250  Task #250"));
    assert!(!is_selected(&fetch, None, "#250  Task #250"));
    assert!(!is_selected(&fetch, Some(1), "#250  Task #250"));
}

/// The selectable index is flat across sections: an in-progress row's index
/// picks up where the (possibly empty) next-tasks section left off.
#[test]
fn selection_index_continues_across_sections() {
    let fetch = complete(vec![task("#1", "open"), task("#2", "in_progress")]);
    assert!(is_selected(&fetch, Some(1), "▸ #2  Task #2"));
    assert!(!is_selected(&fetch, Some(0), "▸ #2  Task #2"));
}
