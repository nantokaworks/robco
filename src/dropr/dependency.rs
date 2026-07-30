//! Whether a task carries an unresolved `blocks` dependency edge.
//!
//! `dropr task ready --json` already excludes a task with an unresolved
//! `blocks` edge from its own candidate feed (dropr:375's dispatch-side
//! finding — verified empirically, not merely inferred from `task_next`'s own
//! documentation, which describes a different endpoint). That self-clears
//! dispatch for free. It says nothing about a pull request already open,
//! though, so the auto-merge gate reads the edges directly.

use std::{process::Command, time::Duration};

use serde::Deserialize;

/// The prerequisite task a `blocks` edge is still waiting on.
pub struct BlockingPrerequisite {
    pub display_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyFetchError {
    Command,
    Exit,
    Parse,
}

#[derive(Debug, Deserialize)]
struct DependencyEdge {
    #[serde(default)]
    blocking: bool,
    #[serde(default)]
    depends_on_display_id: String,
}

/// The first unresolved `blocks` edge naming `task_id` as the dependent side,
/// if any. `dropr task dependency list` returns every edge touching the task
/// on either side; only `blocking: true` rows — a `blocks` edge whose target
/// has not closed — hold this task back, so every other row is ignored here.
pub fn blocking_prerequisite_timeout(
    task_id: &str,
    timeout: Duration,
) -> Result<Option<BlockingPrerequisite>, DependencyFetchError> {
    let program = crate::config::resolve_program("dropr").ok_or(DependencyFetchError::Command)?;
    let mut command = Command::new(program);
    command.args(dependency_list_args(task_id));
    let output = crate::overseer::exec::run_timeout(command, timeout)
        .map_err(|_| DependencyFetchError::Command)?;
    if !output.status.success() {
        return Err(DependencyFetchError::Exit);
    }
    parse_blocking(&output.stdout).ok_or(DependencyFetchError::Parse)
}

/// The argv `dropr task dependency list` is invoked with, spelled once so a
/// test can pin it against the CLI surface that exists — see
/// `dropr::ready_args` for the sibling this mirrors.
fn dependency_list_args(task_id: &str) -> [&str; 6] {
    ["task", "dependency", "list", "--task", task_id, "--json"]
}

fn parse_blocking(raw: &[u8]) -> Option<Option<BlockingPrerequisite>> {
    let edges: Vec<DependencyEdge> = serde_json::from_slice(raw).ok()?;
    Some(
        edges
            .into_iter()
            .find(|edge| edge.blocking)
            .map(|edge| BlockingPrerequisite {
                display_id: edge.depends_on_display_id,
            }),
    )
}

#[cfg(test)]
#[path = "dependency_tests.rs"]
mod tests;
