use super::*;
use crate::ui::summary::dropr_tasks::dropr_task_lines;
use ratatui::text::Text;

fn task(
    display_id: &str,
    status: &str,
    id: &str,
    parent: Option<&str>,
    child_count: usize,
) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_owned(),
        title: format!("Task {display_id}"),
        priority: String::new(),
        status: status.to_owned(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
        id: id.to_owned(),
        parent_task_id: parent.map(str::to_owned),
        child_count,
    }
}

fn root(display_id: &str, status: &str, id: &str, child_count: usize) -> DroprTaskCandidate {
    task(display_id, status, id, None, child_count)
}

fn child(display_id: &str, status: &str, parent: &str) -> DroprTaskCandidate {
    task(display_id, status, "", Some(parent), 0)
}

#[test]
fn a_task_with_no_parent_id_is_a_root() {
    assert!(is_root(&root("#1", "open", "id-1", 0)));
}

/// `dropr task ready --json` never populates `parent_task_id`, so an empty
/// string — not just `None` — has to read as "no parent" too.
#[test]
fn an_empty_parent_id_is_also_a_root() {
    assert!(is_root(&task("#1", "open", "id-1", Some(""), 0)));
}

#[test]
fn a_task_with_a_parent_id_is_not_a_root() {
    assert!(!is_root(&child("#2", "open", "id-1")));
}

#[test]
fn children_are_grouped_by_their_parents_id() {
    let tasks = [child("#2", "open", "id-1"), child("#3", "open", "id-1")];
    let grouped = children_by_parent(&tasks);
    assert_eq!(grouped["id-1"].len(), 2);
}

#[test]
fn a_childless_task_gets_no_nested_lines() {
    let parent = root("#1", "in_progress", "id-1", 0);
    let lines = nested_lines(
        Locale::En,
        &parent,
        &HashMap::new(),
        &HashSet::from(["id-1".to_owned()]),
    );
    assert!(lines.is_empty());
}

/// The compact fallback the task's acceptance criteria calls for: a parent
/// whose subtree was never walked (skipped past the walk limit, or its query
/// failed) must render the same as a childless one, not a fabricated `0/N`.
#[test]
fn an_unqueried_subtree_gets_no_nested_lines_even_with_a_known_child_count() {
    let parent = root("#1", "in_progress", "id-1", 5);
    let lines = nested_lines(Locale::En, &parent, &HashMap::new(), &HashSet::new());
    assert!(lines.is_empty());
}

#[test]
fn a_known_subtree_shows_the_closed_fraction_and_its_children() {
    let parent = root("#1", "in_progress", "id-1", 3);
    let kids = [child("#2", "open", "id-1")];
    let children = children_by_parent(&kids);
    let lines = nested_lines(
        Locale::En,
        &parent,
        &children,
        &HashSet::from(["id-1".to_owned()]),
    );
    let text: Vec<String> = lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(text.contains(&"  2/3 closed".to_owned()));
    assert!(text.iter().any(|line| line.contains("#2")));
}

#[test]
fn a_blocked_child_carries_its_reason() {
    let mut blocked = child("#2", "blocked", "id-1");
    blocked.blocked_reason = Some("waiting on review".to_owned());
    let lines = child_rows(&blocked);
    let text: Vec<String> = lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(text.iter().any(|line| line.contains("waiting on review")));
}

fn rendered(tasks: Vec<DroprTaskCandidate>) -> Vec<String> {
    let fetch = crate::dropr::DroprTaskFetch {
        tasks,
        problems: Vec::new(),
        answered: true,
        subtrees_known: HashSet::from(["id-1".to_owned()]),
    };
    let text: Text<'static> = dropr_task_lines(&fetch, Locale::En).into();
    text.lines
        .iter()
        .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

/// The end-to-end case the task exists for: a subtask must appear nested
/// under its parent, never a second time as its own "next tasks" row.
#[test]
fn a_subtask_nests_under_its_parent_instead_of_listing_twice() {
    let lines = rendered(vec![
        root("#1", "in_progress", "id-1", 1),
        child("#2", "open", "id-1"),
    ]);

    assert!(lines.iter().any(|line| line == "▸ #1  Task #1"));
    assert!(lines.iter().any(|line| line == "    #2  Task #2"));
    assert!(!lines.iter().any(|line| line == "next tasks"));
}
