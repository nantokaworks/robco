use super::*;
use crate::dropr::DroprTaskCandidate;

fn blocked_task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        display_id: display_id.to_owned(),
        title: format!("Task {display_id}"),
        priority: "high".to_owned(),
        status: "blocked".to_owned(),
        priority_score: None,
        blocked_reason: None,
        updated_at: None,
    }
}

fn open_task(display_id: &str) -> DroprTaskCandidate {
    DroprTaskCandidate {
        status: "open".to_owned(),
        ..blocked_task(display_id)
    }
}

fn complete(tasks: Vec<DroprTaskCandidate>) -> DroprTaskFetch {
    DroprTaskFetch {
        tasks,
        problems: Vec::new(),
        answered: true,
    }
}

/// A recorded batch runner: hands back canned answers in order and keeps
/// every query it was asked, mirroring the parent module's own `Recorder`.
struct Recorder {
    answers: Vec<ScribbleAnswers>,
    asked: Vec<Vec<Value>>,
}

impl Recorder {
    fn new(answers: Vec<ScribbleAnswers>) -> Self {
        Self {
            answers,
            asked: Vec::new(),
        }
    }

    fn run(&mut self, queries: Vec<Value>, _timeout: Duration) -> ScribbleAnswers {
        self.asked.push(queries);
        if self.answers.is_empty() {
            panic!("the lookup asked more batches than the test provided answers for");
        }
        self.answers.remove(0)
    }
}

fn run(fetch: &mut DroprTaskFetch, answers: Vec<ScribbleAnswers>) -> Recorder {
    let mut recorder = Recorder::new(answers);
    attach_blocked_reasons(
        fetch,
        "workspace",
        Instant::now() + crate::dropr::FETCH_BUDGET,
        |q, t| recorder.run(q, t),
    );
    recorder
}

/// The defect this lookup exists to fix: a blocked task's status says nothing
/// about why, and the reason lives only in its `blocker` scribble.
#[test]
fn a_blocked_task_gets_its_reason_from_its_blocker_scribble() {
    let mut fetch = complete(vec![blocked_task("#249")]);

    let recorder = run(
        &mut fetch,
        vec![Ok(vec![Ok(vec![ScribbleRow {
            body: "waiting on an upstream release".to_owned(),
        }])])],
    );

    assert_eq!(
        fetch.tasks[0].blocked_reason.as_deref(),
        Some("waiting on an upstream release")
    );
    let query = &recorder.asked[0][0];
    assert_eq!(query["task_id"], "#249");
    assert_eq!(query["kind"], "blocker");
    assert_eq!(query["unresolved_only"], true);
}

#[test]
fn a_board_with_no_blocked_tasks_asks_no_scribble_queries() {
    let mut fetch = complete(vec![open_task("#1")]);

    let recorder = run(&mut fetch, Vec::new());

    assert!(recorder.asked.is_empty());
}

#[test]
fn an_unanswered_fetch_gets_no_blocked_reason_lookup() {
    let mut fetch = DroprTaskFetch::failed("root tasks: dropr refused: expired login");

    let recorder = run(&mut fetch, Vec::new());

    assert!(recorder.asked.is_empty());
}

#[test]
fn no_unresolved_blocker_scribble_leaves_the_reason_absent() {
    let mut fetch = complete(vec![blocked_task("#249")]);

    run(&mut fetch, vec![Ok(vec![Ok(Vec::new())])]);

    assert_eq!(fetch.tasks[0].blocked_reason, None);
}

/// A lookup failure must not erase the task the walk already found — only the
/// reason is missing, so only the reason is reported as a problem.
#[test]
fn a_failed_blocker_lookup_is_reported_without_losing_the_task() {
    let mut fetch = complete(vec![blocked_task("#249")]);

    run(
        &mut fetch,
        vec![Ok(vec![Err("dropr refused: unknown workspace".to_owned())])],
    );

    assert_eq!(fetch.tasks.len(), 1);
    assert_eq!(fetch.tasks[0].blocked_reason, None);
    assert_eq!(
        fetch.problems,
        ["blocker reason of #249: dropr refused: unknown workspace"]
    );
}

#[test]
fn more_blocked_tasks_than_the_lookup_allows_reports_the_shortfall() {
    let tasks = (0..BLOCKER_QUERY_LIMIT + 2)
        .map(|index| blocked_task(&format!("#{index}")))
        .collect();
    let mut fetch = complete(tasks);
    let answers = (0..BLOCKER_QUERY_LIMIT).map(|_| Ok(Vec::new())).collect();

    let recorder = run(&mut fetch, vec![Ok(answers)]);

    assert_eq!(recorder.asked[0].len(), BLOCKER_QUERY_LIMIT);
    assert_eq!(
        fetch.problems,
        ["blocker reason of 2 further blocked task(s) was not asked for"]
    );
}
