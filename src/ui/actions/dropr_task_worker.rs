//! The off-thread half of a dropr task launch (dropr:508): the worker thread
//! that claims the task and creates the agent, and the one result it reports
//! back to the UI thread.
//!
//! Every step here waits on something slow — a dropr round trip, `git fetch`,
//! `git worktree add`, `tmux new-session` — which is why none of it may run
//! on the render thread. The shape is the one [`super::merge_worker`] already
//! uses for `m`: spawn a thread, hand the caller an `mpsc::Receiver`, and let
//! the event loop drain it each tick.
//!
//! The worker returns a typed [`TaskLaunchFailure`] rather than a rendered
//! message, so the locale tables keep doing their work on the UI thread where
//! `App::locale` lives.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::{
    agent,
    config::Config,
    dropr::{self, DroprTaskCandidate},
    model::{AgentNode, RepoNode},
    overseer::{exec::COMMAND_TIMEOUT, templates::worker_prompt},
};

use super::dropr_task_launch::resolve_launch_subtasks;

/// Identifies a task claim taken by this drill-down, distinct from the
/// Overseer daemon's own `OVERSEER_AGENT_ID` — this launch is operator-issued
/// and in-process, not a dispatch decision, so it should not read as one.
const DIRECT_LAUNCH_AGENT_ID: &str = "robco-ui";

/// Why a launch stopped, in the terms the UI needs to name the task it was
/// about. Each variant maps to one message template in
/// [`super::dropr_task_drill`].
pub(super) enum TaskLaunchFailure {
    /// `child_count > 0` but the subtask list could not be confirmed, so the
    /// prompt's `Close Dropr:` lines would be wrong (dropr:479).
    SubtasksUnconfirmed,
    /// dropr answered and declined the claim; carries its reason.
    ClaimRefused(String),
    /// The claim call never reached a verdict.
    DroprUnreachable,
    /// The claim was taken, but the agent could not be created. The claim has
    /// already been handed back by the time this is sent.
    Spawn(String),
    /// The worker thread never started, or died without sending a result.
    /// Raised by the drain, not by the worker itself.
    WorkerTerminated,
}

pub(super) type TaskLaunchResult = std::result::Result<AgentNode, TaskLaunchFailure>;

/// Everything the launch needs that only the UI thread can read. `repo` is a
/// clone rather than a borrow: it feeds both the cached-subtask lookup (which
/// reads `dropr_tasks`) and `agent::create_agent` (which reads `path` and
/// `name`).
pub(super) struct TaskLaunchTarget {
    pub repo: RepoNode,
    pub config: Config,
    pub workspace_id: String,
    pub candidate: DroprTaskCandidate,
    /// `"{display_id} {title}"`, already built by the caller because it also
    /// used it for the branch-collision check.
    pub title: String,
}

/// One launch in flight. The UI keeps exactly one of these at a time (see
/// `App::task_launch_job`), which is what stops a second `n` from starting a
/// duplicate worker.
pub(in crate::ui) struct TaskLaunchJob {
    /// Names the task in every message the drain renders, including the ones
    /// that arrive long after the operator moved on.
    pub display_id: String,
    /// The agent title, for the success message.
    pub title: String,
    /// Where the new agent belongs in the registry.
    pub repo_path: std::path::PathBuf,
    /// The row whose cached status flips once the launch lands.
    pub task_id: String,
    receiver: Receiver<TaskLaunchResult>,
}

impl TaskLaunchJob {
    pub(super) fn try_recv(&self) -> std::result::Result<TaskLaunchResult, TryRecvError> {
        self.receiver.try_recv()
    }
}

/// Starts the launch on its own thread and hands back the job that tracks it.
pub(super) fn spawn(target: TaskLaunchTarget) -> TaskLaunchJob {
    let (sender, receiver) = mpsc::channel();
    let display_id = target.candidate.display_id.clone();
    let title = target.title.clone();
    let repo_path = target.repo.path.clone();
    let task_id = target.candidate.id.clone();
    // A thread that never started drops the closure, and with it the sender,
    // so the drain's disconnect arm reports the dead launch — the same way it
    // reports a worker that died mid-launch. Nothing is left waiting forever.
    let _ = std::thread::Builder::new()
        .name("ui-task-launch".into())
        .spawn(move || {
            let _ = sender.send(run_launch(&target));
        });
    TaskLaunchJob {
        display_id,
        title,
        repo_path,
        task_id,
        receiver,
    }
}

fn run_launch(target: &TaskLaunchTarget) -> TaskLaunchResult {
    // Reuses the subtree this repo's own INFO pane already fetched
    // (dropr:475) whenever that fetch actually answered for this parent
    // (`subtrees_known`). When the fetch's own `SUBTREE_QUERY_LIMIT` (8
    // parents) skipped this task, this runs one scoped fetch for the one task
    // being launched (dropr:479) — off the render thread, so the operator
    // never waits on it.
    let subtasks = resolve_launch_subtasks(
        &target.repo,
        &target.candidate,
        &target.workspace_id,
        COMMAND_TIMEOUT,
        dropr::fetch_subtasks,
    )
    .map_err(|()| TaskLaunchFailure::SubtasksUnconfirmed)?;

    match dropr::claim_task(
        &target.workspace_id,
        &target.candidate.id,
        DIRECT_LAUNCH_AGENT_ID,
        COMMAND_TIMEOUT,
    ) {
        dropr::ClaimAttempt::Claimed => {}
        dropr::ClaimAttempt::Refused(reason) => {
            return Err(TaskLaunchFailure::ClaimRefused(reason));
        }
        dropr::ClaimAttempt::Unavailable => return Err(TaskLaunchFailure::DroprUnreachable),
    }

    let prompt = worker_prompt(
        &target.candidate.display_id,
        &target.candidate.id,
        &target.candidate.title,
        &target.repo.name,
        &subtasks,
        target.config.language.as_deref(),
        target.config.overseer.worker_prompt_template.as_deref(),
    );

    agent::create_agent(
        &target.repo,
        &target.title,
        Some(&prompt),
        &target.config,
        None,
    )
    .map_err(|err| {
        // The claim was taken for a worker that never started; holding it
        // would park the task away from the next operator or dispatch pass.
        // Same recovery `overseer::dispatch::worker::spawn_candidate` makes on
        // its own spawn failure — best effort, so a dropr that is itself down
        // leaves the claim held until its TTL expires.
        let _ = dropr::release_claim(
            &target.workspace_id,
            &target.candidate.id,
            DIRECT_LAUNCH_AGENT_ID,
            COMMAND_TIMEOUT,
        );
        TaskLaunchFailure::Spawn(err.to_string())
    })
}

/// A job whose worker is this sender, so the drain can be exercised without a
/// real claim or a real `git worktree add` behind it.
#[cfg(test)]
pub(super) fn test_job(
    display_id: &str,
    title: &str,
    repo_path: std::path::PathBuf,
    task_id: &str,
) -> (TaskLaunchJob, mpsc::Sender<TaskLaunchResult>) {
    let (sender, receiver) = mpsc::channel();
    (
        TaskLaunchJob {
            display_id: display_id.to_string(),
            title: title.to_string(),
            repo_path,
            task_id: task_id.to_string(),
            receiver,
        },
        sender,
    )
}
