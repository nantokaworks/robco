//! `robco_approve` — confirm a live worker's prompt, or, when no live
//! session is left to confirm into, request a merge for its pull request
//! instead.
//!
//! The two paths answer the same operator intent — "yes, proceed" — over
//! whichever channel still reaches the work: typing into a live tmux session
//! when one exists, or handing the merge pass a request to pick up on its
//! next tick when it does not (see
//! `overseer::runtime_request::RuntimeRequest::OperatorMergeOverride`).
//!
//! **Trust model.** Nothing at the MCP layer distinguishes a human operator's
//! call from an autonomous agent's — the same is true of every other
//! mutating tool here, `robco_merge` included. The live-session path is
//! low-stakes (it can only answer a prompt the target agent itself is
//! already, visibly, waiting on) and stays ungated. The session-less
//! fallback is not: it drives a merge through the daemon with nobody left to
//! answer for it directly, so it is gated the same way `robco_merge` gates
//! deleting a worktree and branch — an explicit `confirm: true` the caller
//! cannot pass by accident. As with `robco_merge`, the gate signals intent;
//! it does not authenticate the caller. Agents operating under
//! RUN/orchestration discipline are told never to call either without
//! explicit authorization — that instruction, not this flag, is what keeps
//! an autonomous session from invoking it on its own judgment.
//!
//! **Status snapshot vs. send.** `live_status` classifies an agent as
//! anything other than `Dead`/`BranchOnly` only while its tmux session is
//! observed alive, so it should be impossible to reach the live-session send
//! below with a session that is truly gone — except for the gap between the
//! two calls. A worker's shell can exit and take its tmux session with it in
//! that gap, so `send_keys` below re-validates for real instead of trusting
//! the snapshot: on failure it checks whether the session is confirmed gone
//! *now* and, only then, falls through to the confirm-gated fallback rather
//! than surfacing a raw tmux error the caller cannot act on.

use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    model::{AgentNode, RepoNode, Status},
    overseer::{
        ledger::Ledger,
        runtime_request::{self, RuntimeRequest},
    },
    registry::Registry,
    tmux,
};

use super::{ToolResult, exec_err, find_agent, invalid_params, live_status};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApproveArgs {
    pub agent_id: String,
    /// Required only for the session-less fallback (see the module doc);
    /// answering a live prompt needs no confirmation, the same way it never
    /// has.
    #[serde(default)]
    pub confirm: bool,
}

pub(super) fn approve(registry: &Registry, args: &ApproveArgs) -> ToolResult<Value> {
    approve_with(
        registry,
        args,
        |repo, agent| live_status(repo, agent).status,
        |session| tmux::send_keys(session, &["y", "Enter"]),
        tmux::has_session,
        || Ledger::load().map_err(exec_err),
        |request| runtime_request::enqueue(request).map_err(exec_err),
    )
}

/// Core of [`approve`], with every side effect injected so a test can drive
/// the full session → fallback → ledger flow without touching real tmux, the
/// ledger file, or the runtime-request queue — see `grant_operator_override`
/// for the same shape.
fn approve_with(
    registry: &Registry,
    args: &ApproveArgs,
    status_of: impl FnOnce(&RepoNode, &AgentNode) -> Status,
    send_keys: impl FnOnce(&str) -> crate::Result<()>,
    has_session: impl FnOnce(&str) -> crate::Result<bool>,
    load_ledger: impl FnOnce() -> ToolResult<Ledger>,
    enqueue: impl FnOnce(RuntimeRequest) -> ToolResult<()>,
) -> ToolResult<Value> {
    if let Ok((repo, agent)) = find_agent(registry, &args.agent_id) {
        let status = status_of(repo, agent);
        if !matches!(status, Status::Dead | Status::BranchOnly) {
            match send_keys(&agent.tmux_session) {
                Ok(()) => return Ok(json!({ "ok": true, "mode": "session" })),
                Err(err) => {
                    // The status snapshot above can be stale by now (see the
                    // module doc): only treat this as "no live session" once
                    // the session is confirmed gone for real. Any other
                    // send-keys failure is a genuine error and must not be
                    // swallowed into the fallback path.
                    if !matches!(has_session(&agent.tmux_session), Ok(false)) {
                        return Err(exec_err(err));
                    }
                }
            }
        }
    }
    if !args.confirm {
        return Err(invalid_params(
            "no live session for agent_id; confirm must be true to request a merge for \
             its pull request",
        ));
    }
    grant_operator_override(&args.agent_id, load_ledger, enqueue)
}

/// The session-less fallback: `target` reaches no live worker, so the
/// decision is handed to the merge pass instead. `load_ledger` and `enqueue`
/// are injected so a test can exercise the validation logic without touching
/// the real ledger file or the runtime-request queue — see `policy::policy`
/// for the same shape.
///
/// Validated against the ledger up front, read-only — the ledger itself is
/// written only inside the daemon's own pass, never from an MCP call — so a
/// typo in `target` fails the call immediately rather than silently
/// enqueueing a request nothing will ever match.
fn grant_operator_override(
    target: &str,
    load_ledger: impl FnOnce() -> ToolResult<Ledger>,
    enqueue: impl FnOnce(RuntimeRequest) -> ToolResult<()>,
) -> ToolResult<Value> {
    let ledger = load_ledger()?;
    if !ledger
        .entries
        .iter()
        .any(|entry| entry.agent_id == target || entry.display_id == target)
    {
        return Err(exec_err(format!(
            "no live session and no ledger entry for {target}"
        )));
    }
    enqueue(RuntimeRequest::OperatorMergeOverride {
        source: "mcp".into(),
        target: target.to_owned(),
        at: Utc::now(),
    })?;
    Ok(json!({
        "ok": true,
        "mode": "operator_override",
        "target": target
    }))
}

#[cfg(test)]
mod tests;
