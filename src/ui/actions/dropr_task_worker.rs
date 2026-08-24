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

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use crate::{
    agent::{
        dropr_task::{DroprTaskLaunch, LaunchError, launch},
        tui_launch_env,
    },
    config::Config,
    dropr::{self, DroprTaskCandidate},
    model::{AgentNode, RepoNode},
    overseer::exec::COMMAND_TIMEOUT,
};

use super::dropr_task_launch::resolve_launch_subtasks;

/// Identifies a task claim taken by this drill-down, distinct from the
/// Overseer daemon's own `OVERSEER_AGENT_ID` — this launch is operator-issued
/// and in-process, not a dispatch decision, so it should not read as one.
const DIRECT_LAUNCH_AGENT_ID: &str = "robco-ui";

/// Why a launch stopped, in the terms the UI needs to name the task it was
/// about. Each variant maps to one message template in
/// [`super::dropr_task_finish`]. `pub(in crate::ui)`, not `pub(super)`: the
/// placeholder tree row's tests (dropr:517, `ui::tree::launch_row`) build
/// fake jobs through `test_job`, whose signature carries this type.
pub(in crate::ui) enum TaskLaunchFailure {
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

pub(in crate::ui) type TaskLaunchResult = std::result::Result<AgentNode, TaskLaunchFailure>;

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

/// One launch in flight. Several can exist at once, keyed by `task_id` in
/// `App::task_launch_jobs` (dropr:517) — that key, not this type, is what
/// stops a second `n` on the *same* row from starting a duplicate worker.
pub(in crate::ui) struct TaskLaunchJob {
    /// Names the task in every message the drain renders, including the ones
    /// that arrive long after the operator moved on.
    pub display_id: String,
    /// The agent title, for the success message and for the placeholder tree
    /// row shown while this is still running (dropr:517) — the same text the
    /// row reads once it becomes a real agent, so nothing visibly changes at
    /// the hand-off.
    pub title: String,
    /// Where the new agent belongs in the registry.
    pub repo_path: std::path::PathBuf,
    /// The row whose cached status flips once the launch starts, and flips
    /// back if it fails.
    pub task_id: String,
    /// The row's own status before this launch's optimistic flip
    /// (dropr:517), so a failure can undo exactly that flip rather than
    /// guessing what the row used to say.
    pub original_status: String,
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
    let original_status = target.candidate.status.clone();
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
        original_status,
        receiver,
    }
}

fn run_launch(target: &TaskLaunchTarget) -> TaskLaunchResult {
    run_launch_with(target, dropr::fetch_subtasks, launch)
}

/// The generic core `run_launch` wraps with the real subtask fetch and the
/// real `agent::dropr_task::launch` — parameterized the same way
/// `agent::dropr_task::launch_with` is over `claim` / `release`, so a test
/// can check exactly what this call site hands to `launch` (including
/// `extra_args` / `extra_env`, dropr:546) without a live dropr claim or a
/// real `git worktree add` / `tmux new-session` behind it.
fn run_launch_with<F, L>(target: &TaskLaunchTarget, fetch: F, launch: L) -> TaskLaunchResult
where
    F: FnOnce(&str, &str, Duration) -> Vec<dropr::Subtask>,
    L: FnOnce(DroprTaskLaunch) -> Result<AgentNode, LaunchError>,
{
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
        fetch,
    )
    .map_err(|()| TaskLaunchFailure::SubtasksUnconfirmed)?;

    // The same environment `create_agent` resolves for the TUI's plain
    // agent creation (`ui/input.rs`) — a worker started from the `n` key is
    // still a TUI launch, so it gets the profile's `autonomous_args`, the
    // paired env blocklist, and `SessionEnv` exactly the same way (dropr:538,
    // widened by dropr:546). Passing `&[]` / `&[]` here — as this call site
    // did before dropr:546 — silently drops all three.
    let (extra_args, extra_env) = tui_launch_env(&target.config);

    launch(DroprTaskLaunch {
        repo: &target.repo,
        config: &target.config,
        workspace_id: &target.workspace_id,
        candidate: &target.candidate,
        subtasks: &subtasks,
        claim_agent_id: DIRECT_LAUNCH_AGENT_ID,
        parent_agent_id: None,
        extra_args: &extra_args,
        extra_env: &extra_env,
    })
    .map_err(|err| match err {
        // "locked" is the one reason with a friendlier name available; every
        // other reason (blocked, dependency_blocked, ...) already names
        // itself well enough to show as-is.
        LaunchError::ClaimRefused(reason) if reason == "locked" => {
            TaskLaunchFailure::ClaimRefused("claimed by another agent".to_string())
        }
        LaunchError::ClaimRefused(reason) => TaskLaunchFailure::ClaimRefused(reason),
        LaunchError::DroprUnreachable => TaskLaunchFailure::DroprUnreachable,
        LaunchError::Spawn(err) => TaskLaunchFailure::Spawn(err.to_string()),
    })
}

/// A job whose worker is this sender, so the drain can be exercised without a
/// real claim or a real `git worktree add` behind it. `pub(in crate::ui)`
/// rather than `pub(super)`: the placeholder tree row (dropr:517,
/// `ui::tree::launch_row`) needs the same fake jobs to test what it draws.
#[cfg(test)]
pub(in crate::ui) fn test_job(
    display_id: &str,
    title: &str,
    repo_path: std::path::PathBuf,
    task_id: &str,
    original_status: &str,
) -> (TaskLaunchJob, mpsc::Sender<TaskLaunchResult>) {
    let (sender, receiver) = mpsc::channel();
    (
        TaskLaunchJob {
            display_id: display_id.to_string(),
            title: title.to_string(),
            repo_path,
            task_id: task_id.to_string(),
            original_status: original_status.to_string(),
            receiver,
        },
        sender,
    )
}

#[cfg(test)]
#[path = "dropr_task_worker_tests.rs"]
mod tests;
