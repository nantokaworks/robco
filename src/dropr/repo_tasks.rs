//! The dropr task list a repository summary shows.
//!
//! Two things this module exists to get right.
//!
//! `task_list` answers about one level of the task hierarchy per call — its
//! contract says "by default it returns root tasks only; pass `parent_task_id`
//! to drill into a branch, and `include_descendants=true` to walk the full
//! subtree" — so no single query can see a subtask. The fetch therefore asks
//! about the roots first and then batches one subtree query per root that has
//! children.
//!
//! And every query that did not answer is named in the result. A list that is
//! missing rows must not render as a complete one, and an empty list must not
//! be indistinguishable from a fetch that failed.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    DroprTaskCandidate,
    mcp::{self, ToolOutcome},
};

mod blocker;

/// Statuses the summary renders. `draft` is left out on purpose: an unpublished
/// task is not work the pane should claim exists. `closed` is left out because
/// the pane is about what is outstanding. `blocked` is included, but the pane
/// (`ui::summary::dropr_tasks`) renders it as its own section so it is never
/// folded in with tasks that are actually available to pick up.
const DISPLAY_STATUSES: &str = "open,in_progress,blocked";

/// Rows one `task_list` query asks for. Bounds the pane's input: the display
/// cap in `ui::summary::dropr_tasks` is what keeps it scannable, and this is
/// what keeps the fetch from dragging a whole board across the wire.
pub const TASK_FETCH_LIMIT: usize = 20;

/// Subtrees one fetch will walk. A board with more parent tasks than this gets
/// a reported shortfall rather than a silently shorter list.
const SUBTREE_QUERY_LIMIT: usize = 8;

/// Wall-clock the whole fetch may spend.
///
/// Held below the UI's stale-result window so a slow dropr comes back saying it
/// timed out, instead of coming back late and having its answer discarded with
/// nothing shown in its place.
pub const FETCH_BUDGET: Duration = Duration::from_secs(20);

/// Time left in which a query is still worth starting. Below this, the answer
/// would be cut off mid-flight and reported as a transport fault rather than as
/// the deadline it actually is.
const MIN_QUERY_TIME: Duration = Duration::from_secs(1);

/// What a display fetch came back with.
///
/// The invariant that makes this worth a struct: `tasks` may be read as the
/// whole answer only while `problems` is empty. `answered == false` means no
/// query answered at all, so an empty `tasks` says nothing about the workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DroprTaskFetch {
    pub tasks: Vec<DroprTaskCandidate>,
    /// One line per query that did not answer, phrased for an operator.
    pub problems: Vec<String>,
    pub answered: bool,
}

impl DroprTaskFetch {
    /// A fetch that answered nothing, carrying the reason to show instead of
    /// rows.
    pub fn failed(problem: impl Into<String>) -> Self {
        Self {
            tasks: Vec::new(),
            problems: vec![problem.into()],
            answered: false,
        }
    }
}

/// A `task_list` row. The pane renders `task`; `id` and `child_count` are what
/// tell the fetch which subtrees it still has to ask about.
#[derive(Debug, Clone, Deserialize)]
struct TaskRow {
    #[serde(default)]
    id: String,
    #[serde(default)]
    child_count: usize,
    #[serde(flatten)]
    task: DroprTaskCandidate,
}

pub fn fetch(workspace_id: &str) -> DroprTaskFetch {
    let deadline = Instant::now() + FETCH_BUDGET;
    let mut fetch = fetch_within(workspace_id, FETCH_BUDGET, mcp_asker);
    blocker::attach_blocked_reasons(
        &mut fetch,
        workspace_id,
        deadline,
        blocker::mcp_scribble_asker,
    );
    fetch
}

/// Answers to one batch of `task_list` calls. The outer `Err` is the session
/// failing; a per-call `Err` is dropr answering that one call with a refusal.
type Answers = Result<Vec<Result<Vec<TaskRow>, String>>, String>;

/// Runs one batch over `dropr mcp-stdio`. [`fetch_within`] takes the batch
/// runner as a parameter so the walk is testable without a `dropr` on PATH.
fn mcp_asker(arguments: Vec<Value>, timeout: Duration) -> Answers {
    let calls = arguments
        .into_iter()
        .map(|arguments| ("task_list", arguments))
        .collect::<Vec<_>>();
    let outcomes = mcp::call_tools(&calls, timeout).ok_or_else(|| {
        "dropr mcp-stdio gave no answer — the binary is missing, it died, or it timed out"
            .to_string()
    })?;
    Ok(outcomes.into_iter().map(read_rows).collect())
}

fn read_rows(outcome: ToolOutcome) -> Result<Vec<TaskRow>, String> {
    match outcome {
        ToolOutcome::Ok(payload) => rows(&payload)
            .ok_or_else(|| "task_list answered in a shape robco cannot read".to_string()),
        // A refusal is dropr's verdict — an expired login, an unknown
        // workspace — and the operator can only act on it if they see it.
        ToolOutcome::Refused(message) => Err(format!("dropr refused: {}", one_line(&message))),
    }
}

fn rows(payload: &Value) -> Option<Vec<TaskRow>> {
    let tasks = payload.get("tasks")?.as_array()?;
    Some(
        tasks
            .iter()
            .filter_map(|task| serde_json::from_value(task.clone()).ok())
            .collect(),
    )
}

fn fetch_within<F>(workspace_id: &str, budget: Duration, mut ask: F) -> DroprTaskFetch
where
    F: FnMut(Vec<Value>, Duration) -> Answers,
{
    let deadline = Instant::now() + budget;
    let roots = match one_answer(ask_within(
        &mut ask,
        vec![root_query(workspace_id)],
        deadline,
    )) {
        Ok(roots) => roots,
        Err(problem) => return DroprTaskFetch::failed(format!("root tasks: {problem}")),
    };

    let parents = roots
        .iter()
        .filter(|row| row.child_count > 0 && !row.id.is_empty())
        .collect::<Vec<_>>();
    let mut fetch = DroprTaskFetch {
        tasks: roots.iter().map(|row| row.task.clone()).collect(),
        problems: Vec::new(),
        answered: true,
    };
    if let Some(skipped) = parents
        .len()
        .checked_sub(SUBTREE_QUERY_LIMIT)
        .filter(|n| *n > 0)
    {
        fetch.problems.push(format!(
            "subtasks of {skipped} further parent task(s) were not asked for"
        ));
    }
    let walked = &parents[..parents.len().min(SUBTREE_QUERY_LIMIT)];
    if walked.is_empty() {
        return settle(fetch);
    }

    let queries = walked
        .iter()
        .map(|row| subtree_query(workspace_id, &row.id))
        .collect();
    match ask_within(&mut ask, queries, deadline) {
        Ok(answers) => {
            for (parent, answer) in walked.iter().zip(answers) {
                match answer {
                    Ok(rows) => fetch.tasks.extend(rows.into_iter().map(|row| row.task)),
                    Err(problem) => fetch
                        .problems
                        .push(format!("subtasks of {}: {problem}", parent.task.display_id)),
                }
            }
        }
        Err(problem) => fetch.problems.push(format!(
            "subtasks of {} parent task(s): {problem}",
            walked.len()
        )),
    }
    settle(fetch)
}

fn one_answer(answers: Answers) -> Result<Vec<TaskRow>, String> {
    answers?
        .into_iter()
        .next()
        .unwrap_or_else(|| Err("dropr mcp-stdio answered no call at all".to_string()))
}

/// Runs a batch with whatever is left of the budget, and reports a spent budget
/// as the deadline it is rather than letting a doomed query fail as a fault.
///
/// Generic over the answer shape so the `blocker` submodule's own
/// `scribble_list` batches share this deadline bookkeeping instead of
/// duplicating it.
fn ask_within<F, T>(ask: &mut F, queries: Vec<Value>, deadline: Instant) -> Result<T, String>
where
    F: FnMut(Vec<Value>, Duration) -> Result<T, String>,
{
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout < MIN_QUERY_TIME {
        return Err(format!(
            "the fetch ran out of its {}s budget",
            FETCH_BUDGET.as_secs()
        ));
    }
    ask(queries, timeout)
}

fn root_query(workspace_id: &str) -> Value {
    json!({
        "workspace_id": workspace_id,
        "status": DISPLAY_STATUSES,
        "limit": TASK_FETCH_LIMIT,
    })
}

/// The one query shape that can see a subtask. `include_descendants` only takes
/// effect alongside `parent_task_id`, so the two always travel together.
fn subtree_query(workspace_id: &str, parent_task_id: &str) -> Value {
    json!({
        "workspace_id": workspace_id,
        "parent_task_id": parent_task_id,
        "include_descendants": true,
        "status": DISPLAY_STATUSES,
        "limit": TASK_FETCH_LIMIT,
    })
}

/// Dedupes and orders the rows the fetch collected.
///
/// Ordering is what decides which rows survive the pane's display cap, so it
/// puts the most urgent first: priority, then task number so the tie-break
/// reads as task order.
fn settle(mut fetch: DroprTaskFetch) -> DroprTaskFetch {
    let mut seen = HashSet::new();
    fetch
        .tasks
        .retain(|task| seen.insert(task.display_id.clone()));
    fetch.tasks.sort_by_key(|task| {
        (
            priority_rank(&task.priority),
            display_number(&task.display_id),
        )
    });
    fetch
}

/// Display order for priorities, most urgent first. An unset or unrecognised
/// priority sorts last: it is the least informative row to keep.
fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

/// The number in a `#N` display id, so `#9` sorts before `#10`.
fn display_number(display_id: &str) -> u64 {
    display_id
        .trim_start_matches('#')
        .split('-')
        .next()
        .unwrap_or_default()
        .parse()
        .unwrap_or(u64::MAX)
}

/// Squashes a server message onto the single line a summary row can hold.
fn one_line(message: &str) -> String {
    message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

#[cfg(test)]
#[path = "repo_tasks_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "repo_tasks_answer_tests.rs"]
mod answer_tests;
