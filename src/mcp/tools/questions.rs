//! `robco_question_list`: agents currently waiting on a confirmation prompt.
//!
//! Split out of `tools.rs` into its own module so `Command::QuestionList`
//! (Discord's shared copy of this action, see `discord::agent_actions`) can
//! call it without reaching into `tools.rs`'s private free functions.

use serde_json::{Value, json};

use crate::{model::Status, registry::Registry};

use super::{ToolResult, exec_err, live_status, prompt_tail};

pub(super) fn list() -> ToolResult<Value> {
    let registry = Registry::load().map_err(exec_err)?;
    list_in(&registry)
}

fn list_in(registry: &Registry) -> ToolResult<Value> {
    let questions = registry
        .repos
        .iter()
        .flat_map(|repo| {
            repo.agents.iter().filter_map(move |agent| {
                let report = live_status(repo, agent);
                (report.status == Status::Waiting && report.awaiting_confirmation).then(|| {
                    json!({
                        "agent_id": agent.id,
                        "title": agent.title,
                        "tmux_session": agent.tmux_session,
                        "worktree_missing": report.worktree_missing,
                        "prompt": prompt_tail(&agent.tmux_session)
                    })
                })
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({ "questions": questions }))
}
