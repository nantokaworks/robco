use serde::{Deserialize, Serialize};

use entries::{has_active_worker, task_entries};

mod claim;
mod decision_log;
mod entries;
pub(crate) mod naming;
mod resolve;
mod run;
mod worker;
pub(crate) use run::{RunOutcome, run_named};

/// A dropr task the operator named for a launch, resolved to the repository
/// and metadata a worker spawn needs. See [`resolve::resolve_task`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub task_id: String,
    pub display_id: String,
    pub title: String,
    pub repo: String,
    pub author: String,
    /// dropr task priority (`high` / `medium` / `low`), verbatim. Empty when
    /// the ready feed omitted it.
    pub priority: String,
    /// dropr workspace the task lives in. Carried from resolution so the
    /// pre-spawn claim can address the task without re-resolving the
    /// repository's workspace.
    pub workspace: String,
    /// dropr's numeric refinement of `priority`, verbatim. `None` when the
    /// ready feed did not carry one.
    pub priority_score: Option<i64>,
    /// dropr's task status (`open`, `in_progress`, `blocked`, ...), verbatim.
    /// `#[serde(default)]` so a request persisted by an older build still
    /// deserializes.
    #[serde(default)]
    pub status: String,
}
