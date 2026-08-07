use super::*;

/// A recorded batch runner: hands back canned answers in order and keeps every
/// query it was asked, so a test can assert on the arguments that went out.
struct Recorder {
    answers: Vec<Answers>,
    asked: Vec<Vec<Value>>,
    timeouts: Vec<Duration>,
}

impl Recorder {
    fn new(answers: Vec<Answers>) -> Self {
        Self {
            answers,
            asked: Vec::new(),
            timeouts: Vec::new(),
        }
    }

    fn run(&mut self, queries: Vec<Value>, timeout: Duration) -> Answers {
        self.asked.push(queries);
        self.timeouts.push(timeout);
        if self.answers.is_empty() {
            panic!("the fetch asked more batches than the test provided answers for");
        }
        self.answers.remove(0)
    }
}

fn row(display_id: &str, status: &str, priority: &str, children: usize) -> TaskRow {
    TaskRow {
        id: format!("id-{}", display_id.trim_start_matches('#')),
        child_count: children,
        task: DroprTaskCandidate {
            display_id: display_id.to_owned(),
            title: format!("Task {display_id}"),
            priority: priority.to_owned(),
            status: status.to_owned(),
            priority_score: None,
            blocked_reason: None,
            updated_at: None,
        },
    }
}

fn run(answers: Vec<Answers>) -> (DroprTaskFetch, Recorder) {
    let mut recorder = Recorder::new(answers);
    let fetch = fetch_within("workspace", FETCH_BUDGET, |queries, timeout| {
        recorder.run(queries, timeout)
    });
    (fetch, recorder)
}

/// Whether the row list may be read as the whole answer.
fn is_complete(fetch: &DroprTaskFetch) -> bool {
    fetch.answered && fetch.problems.is_empty()
}

fn ids(fetch: &DroprTaskFetch) -> Vec<&str> {
    fetch
        .tasks
        .iter()
        .map(|task| task.display_id.as_str())
        .collect()
}

/// The defect this module exists for: `task_list` returns root tasks only
/// unless `parent_task_id` is set, and `include_descendants` is inert without
/// it. If either argument goes missing, no subtask can ever be displayed.
#[test]
fn the_subtree_query_names_a_parent_and_asks_for_descendants() {
    let (_, recorder) = run(vec![
        Ok(vec![Ok(vec![row("#249", "in_progress", "high", 5)])]),
        Ok(vec![Ok(vec![row("#250", "open", "high", 0)])]),
    ]);

    let subtree = &recorder.asked[1][0];
    assert_eq!(subtree["parent_task_id"], "id-249");
    assert_eq!(subtree["include_descendants"], true);
    assert_eq!(subtree["workspace_id"], "workspace");
    assert_eq!(subtree["status"], DISPLAY_STATUSES);
}

/// The pane renders `open` rows, so the fetch has to ask for them; asking only
/// for `in_progress` is what hid every waiting task.
#[test]
fn the_root_query_asks_for_the_statuses_the_pane_renders() {
    let (_, recorder) = run(vec![Ok(vec![Ok(Vec::new())])]);

    let roots = &recorder.asked[0][0];
    assert_eq!(roots["status"], "open,in_progress,blocked");
    assert_eq!(roots["limit"], TASK_FETCH_LIMIT);
    assert!(roots.get("parent_task_id").is_none());
}

#[test]
fn subtasks_join_the_root_tasks_in_one_list() {
    let (fetch, recorder) = run(vec![
        Ok(vec![Ok(vec![
            row("#249", "in_progress", "high", 5),
            row("#256", "in_progress", "high", 0),
        ])]),
        Ok(vec![Ok(vec![
            row("#250", "open", "high", 0),
            row("#254", "open", "medium", 0),
        ])]),
    ]);

    assert_eq!(ids(&fetch), ["#249", "#250", "#256", "#254"]);
    assert!(is_complete(&fetch));
    // Only the parent with children is asked about a second time.
    assert_eq!(recorder.asked[1].len(), 1);
}

#[test]
fn a_childless_board_asks_only_the_root_query() {
    let (fetch, recorder) = run(vec![Ok(vec![Ok(vec![row("#256", "open", "high", 0)])])]);

    assert_eq!(recorder.asked.len(), 1);
    assert!(is_complete(&fetch));
}

/// Priority decides which rows survive the pane's display cap, so the most
/// urgent has to sort first; the task number is the tie-break so `#9` comes
/// before `#10` rather than after it.
#[test]
fn rows_are_ordered_by_priority_then_task_number() {
    let (fetch, _) = run(vec![Ok(vec![Ok(vec![
        row("#10", "open", "low", 0),
        row("#9", "open", "low", 0),
        row("#40", "open", "", 0),
        row("#30", "open", "high", 0),
        row("#20", "open", "medium", 0),
    ])])]);

    assert_eq!(ids(&fetch), ["#30", "#20", "#9", "#10", "#40"]);
}

#[test]
fn a_task_reachable_twice_is_listed_once() {
    let (fetch, _) = run(vec![
        Ok(vec![Ok(vec![row("#249", "in_progress", "high", 1)])]),
        Ok(vec![Ok(vec![
            row("#250", "open", "high", 0),
            row("#250", "open", "high", 0),
        ])]),
    ]);

    assert_eq!(ids(&fetch), ["#249", "#250"]);
}

#[test]
fn a_failed_root_query_is_a_failure_not_an_empty_board() {
    let (fetch, _) = run(vec![Err("dropr mcp-stdio gave no answer".to_owned())]);

    assert!(!fetch.answered);
    assert!(fetch.tasks.is_empty());
    assert!(!is_complete(&fetch));
    assert!(fetch.problems[0].contains("root tasks"));
    assert!(fetch.problems[0].contains("gave no answer"));
}

/// An empty board and a broken fetch both hold zero rows; only `answered` tells
/// them apart, and the pane renders them differently because of it.
#[test]
fn an_empty_board_is_a_complete_answer() {
    let (fetch, _) = run(vec![Ok(vec![Ok(Vec::new())])]);

    assert!(fetch.answered);
    assert!(fetch.tasks.is_empty());
    assert!(is_complete(&fetch));
}

#[test]
fn a_failed_subtree_query_keeps_the_roots_and_says_what_is_missing() {
    let (fetch, _) = run(vec![
        Ok(vec![Ok(vec![row("#249", "in_progress", "high", 5)])]),
        Ok(vec![Err("dropr refused: unknown workspace".to_owned())]),
    ]);

    assert!(fetch.answered);
    assert_eq!(ids(&fetch), ["#249"]);
    // Answered, but not complete: the roots alone must not read as the answer.
    assert!(!is_complete(&fetch));
    assert_eq!(
        fetch.problems,
        ["subtasks of #249: dropr refused: unknown workspace"]
    );
}

#[test]
fn a_failed_subtree_batch_names_how_many_parents_it_lost() {
    let (fetch, _) = run(vec![
        Ok(vec![Ok(vec![
            row("#249", "in_progress", "high", 5),
            row("#100", "open", "high", 2),
        ])]),
        Err("the session died".to_owned()),
    ]);

    assert!(fetch.answered);
    assert_eq!(
        fetch.problems,
        ["subtasks of 2 parent task(s): the session died"]
    );
}

#[test]
fn more_parents_than_the_walk_allows_reports_the_shortfall() {
    let roots = (0..SUBTREE_QUERY_LIMIT + 3)
        .map(|index| row(&format!("#{index}"), "open", "high", 1))
        .collect();
    let subtrees = (0..SUBTREE_QUERY_LIMIT).map(|_| Ok(Vec::new())).collect();
    let (fetch, recorder) = run(vec![Ok(vec![Ok(roots)]), Ok(subtrees)]);

    assert_eq!(recorder.asked[1].len(), SUBTREE_QUERY_LIMIT);
    assert_eq!(
        fetch.problems,
        ["subtasks of 3 further parent task(s) were not asked for"]
    );
}

/// A parent whose id did not come back cannot be queried, and inventing an
/// empty `parent_task_id` would silently re-run the root query instead.
#[test]
fn a_parent_without_an_id_is_not_walked() {
    let mut orphan = row("#1", "open", "high", 3);
    orphan.id = String::new();
    let (fetch, recorder) = run(vec![Ok(vec![Ok(vec![orphan])])]);

    assert_eq!(recorder.asked.len(), 1);
    assert!(is_complete(&fetch));
}

#[test]
fn a_spent_budget_is_reported_as_a_timeout() {
    let mut recorder = Recorder::new(Vec::new());
    let fetch = fetch_within("workspace", Duration::ZERO, |queries, timeout| {
        recorder.run(queries, timeout)
    });

    assert!(!fetch.answered);
    assert!(
        recorder.asked.is_empty(),
        "no query should have been started"
    );
    assert!(fetch.problems[0].contains("ran out of its"));
}

/// The subtree batch must inherit what is left of the budget, not start a fresh
/// one — two full-length batches would outlive the UI's stale window.
#[test]
fn the_subtree_batch_runs_inside_the_remaining_budget() {
    let (_, recorder) = run(vec![
        Ok(vec![Ok(vec![row("#249", "in_progress", "high", 1)])]),
        Ok(vec![Ok(Vec::new())]),
    ]);

    assert!(recorder.timeouts[0] <= FETCH_BUDGET);
    assert!(recorder.timeouts[1] <= recorder.timeouts[0]);
}
