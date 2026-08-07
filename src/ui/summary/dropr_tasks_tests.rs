use super::*;
use ratatui::text::Text;

fn task(display_id: &str, status: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_string(),
        title: format!("Task {display_id}"),
        priority: String::new(),
        status: status.to_string(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
    }
}

fn blocked_task(display_id: &str, reason: Option<&str>) -> DroprTaskCandidate {
    DroprTaskCandidate {
        blocked_reason: reason.map(str::to_owned),
        ..task(display_id, "blocked")
    }
}

fn tasks(count: usize, status: &str) -> Vec<DroprTaskCandidate> {
    (0..count)
        .map(|index| task(&format!("#{index}"), status))
        .collect()
}

fn complete(tasks: Vec<DroprTaskCandidate>) -> DroprTaskFetch {
    DroprTaskFetch {
        tasks,
        problems: Vec::new(),
        answered: true,
    }
}

fn rendered(fetch: &DroprTaskFetch) -> Vec<String> {
    let text: Text<'static> = dropr_task_lines(fetch, Locale::En).into();
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

fn rendered_lines(tasks: &[DroprTaskCandidate]) -> Vec<String> {
    rendered(&complete(tasks.to_vec()))
}

#[test]
fn partitions_blocked_and_in_progress_from_next_tasks() {
    let tasks = [
        task("#1", "in_progress"),
        task("#2", "open"),
        task("#3", "blocked"),
    ];
    let (blocked, in_progress, next) = partition_tasks(&tasks);
    assert_eq!(blocked[0].display_id, "#3");
    assert_eq!(in_progress[0].display_id, "#1");
    assert_eq!(next[0].display_id, "#2");
}

/// The defect this rendering exists for: `open` and `blocked` are both "not
/// running", but only `open` is available work. Folding them together read as
/// the task being pickable when it was not.
#[test]
fn a_blocked_task_does_not_land_under_next_tasks() {
    let tasks = [task("#1", "open"), task("#2", "blocked")];
    let (blocked, _, next) = partition_tasks(&tasks);
    assert_eq!(next.len(), 1);
    assert_eq!(blocked.len(), 1);
}

/// The fetch now returns real dropr statuses rather than whatever the dispatch
/// endpoint nominated, so an `open` task has to land under "next tasks".
#[test]
fn an_open_task_is_a_next_task() {
    let lines = rendered_lines(&[task("#250", "open")]);
    assert!(lines.iter().any(|line| line == "next tasks"));
    assert!(lines.iter().any(|line| line == "#250  Task #250"));
}

#[test]
fn renders_in_progress_after_next_tasks() {
    let lines = rendered_lines(&[task("#2", "open"), task("#1", "in_progress")]);
    let in_progress = lines.iter().position(|line| line == "in progress").unwrap();
    let next = lines.iter().position(|line| line == "next tasks").unwrap();
    assert!(next < in_progress);
    assert!(lines.iter().any(|line| line == "▸ #1  Task #1"));
}

#[test]
fn omits_in_progress_heading_without_matching_tasks() {
    let lines = rendered_lines(&[task("#2", "open")]);
    assert!(!lines.iter().any(|line| line == "in progress"));
    assert!(lines.iter().any(|line| line == "next tasks"));
}

#[test]
fn lists_more_than_three_ready_tasks() {
    let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT, "open"));
    assert_eq!(
        lines.iter().filter(|line| line.starts_with('#')).count(),
        TASK_DISPLAY_LIMIT
    );
}

#[test]
fn a_complete_list_carries_no_notice() {
    let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT, "open"));
    assert!(!lines.iter().any(|line| line.starts_with('…')));
}

#[test]
fn a_truncated_list_counts_what_it_left_out() {
    let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT + 2, "open"));
    assert!(lines.iter().any(|line| line == "… and 2 more"));
}

#[test]
fn in_progress_truncation_is_reported_too() {
    // Both lists truncated silently before; the notice belongs to both.
    let lines = rendered_lines(&tasks(TASK_DISPLAY_LIMIT + 1, "in_progress"));
    assert!(lines.iter().any(|line| line == "… and 1 more"));
}

/// The boundary itself: the cap is the last count that renders in full, and one
/// row past it is where the notice has to appear.
#[test]
fn the_display_limit_is_the_last_count_rendered_in_full() {
    let at_limit = rendered_lines(&tasks(TASK_DISPLAY_LIMIT, "open"));
    let over_limit = rendered_lines(&tasks(TASK_DISPLAY_LIMIT + 1, "open"));
    assert_eq!(
        at_limit.iter().filter(|line| line.starts_with('#')).count(),
        TASK_DISPLAY_LIMIT
    );
    assert_eq!(
        over_limit
            .iter()
            .filter(|line| line.starts_with('#'))
            .count(),
        TASK_DISPLAY_LIMIT
    );
    assert!(!at_limit.iter().any(|line| line.starts_with('…')));
    assert!(over_limit.iter().any(|line| line == "… and 1 more"));
}

#[test]
fn a_saturated_fetch_reports_a_floor_not_a_count() {
    // The fetch came back full, so tasks beyond it never reached the panel
    // and the remainder it can see is a lower bound.
    let lines = rendered_lines(&tasks(TASK_FETCH_LIMIT, "open"));
    let hidden = TASK_FETCH_LIMIT - TASK_DISPLAY_LIMIT;
    assert!(
        lines
            .iter()
            .any(|line| line == &format!("… and at least {hidden} more"))
    );
}

/// The defect this rendering exists for: a failed fetch used to leave the panel
/// showing an empty list, which reads as "no tasks".
#[test]
fn a_failed_fetch_says_so_instead_of_showing_an_empty_list() {
    let lines = rendered(&DroprTaskFetch::failed(
        "root tasks: dropr refused: expired login",
    ));

    assert!(lines.iter().any(|line| line == "tasks unavailable"));
    assert!(
        lines
            .iter()
            .any(|line| line == "! root tasks: dropr refused: expired login")
    );
    assert!(!lines.iter().any(|line| line == "next tasks"));
    assert!(
        !lines
            .iter()
            .any(|line| line == "no open, in-progress, or blocked tasks")
    );
}

#[test]
fn an_empty_board_is_stated_rather_than_left_blank() {
    let lines = rendered(&complete(Vec::new()));

    assert!(
        lines
            .iter()
            .any(|line| line == "no open, in-progress, or blocked tasks")
    );
    assert!(!lines.iter().any(|line| line == "tasks unavailable"));
}

/// The `blocked` section reads as its own thing: distinct heading, distinct
/// row glyph, placed apart from the work an operator can actually pick up.
#[test]
fn a_blocked_task_gets_its_own_section() {
    let lines = rendered(&complete(vec![
        task("#1", "open"),
        blocked_task("#2", None),
    ]));

    let blocked_heading = lines.iter().position(|line| line == "blocked").unwrap();
    let next_heading = lines.iter().position(|line| line == "next tasks").unwrap();
    assert!(next_heading < blocked_heading);
    assert!(lines.iter().any(|line| line == "✖ #2  Task #2"));
    assert!(!lines.iter().any(|line| line == "#2  Task #2"));
}

/// The task's own blocker reason is what makes the unblock condition
/// reachable without opening dropr — see the task acceptance criteria.
#[test]
fn a_blocked_task_shows_its_reason() {
    let lines = rendered(&complete(vec![blocked_task(
        "#2",
        Some("waiting on an upstream release"),
    )]));

    assert!(
        lines
            .iter()
            .any(|line| line == "  waiting on an upstream release")
    );
}

#[test]
fn a_blocked_task_without_a_reason_renders_without_a_second_line() {
    let lines = rendered(&complete(vec![blocked_task("#2", None)]));

    assert_eq!(
        lines.iter().filter(|line| line.contains("#2")).count(),
        1,
        "no reason line should follow a task the lookup found nothing for"
    );
}

#[test]
fn a_blocked_reason_is_squashed_onto_one_line() {
    let lines = rendered(&complete(vec![blocked_task(
        "#2",
        Some("line one\n  line two\tand more"),
    )]));

    assert!(
        lines
            .iter()
            .any(|line| line == "  line one line two and more")
    );
}

/// Half an answer rendered as a whole one is the failure mode that made the
/// panel untrustworthy, so the rows come with the shortfall attached.
#[test]
fn a_partial_fetch_shows_its_rows_and_admits_what_is_missing() {
    let fetch = DroprTaskFetch {
        tasks: vec![task("#249", "in_progress")],
        problems: vec!["subtasks of #249: dropr refused: unknown workspace".to_owned()],
        answered: true,
    };

    let lines = rendered(&fetch);
    assert!(lines.iter().any(|line| line == "▸ #249  Task #249"));
    assert!(lines.iter().any(|line| line == "this list is incomplete"));
    assert!(
        lines
            .iter()
            .any(|line| line == "! subtasks of #249: dropr refused: unknown workspace")
    );
}

/// A partial fetch that got no rows at all still has to say why, rather than
/// claiming the board is empty.
#[test]
fn a_partial_fetch_with_no_rows_reports_the_problem_not_an_empty_board() {
    let fetch = DroprTaskFetch {
        tasks: Vec::new(),
        problems: vec!["subtasks of 2 parent task(s): the session died".to_owned()],
        answered: true,
    };

    let lines = rendered(&fetch);
    assert!(
        lines
            .iter()
            .any(|line| line == "no open, in-progress, or blocked tasks")
    );
    assert!(lines.iter().any(|line| line == "this list is incomplete"));
}
