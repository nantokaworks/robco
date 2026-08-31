//! Declarations for the PR and merge tools.
//!
//! They sit apart from the agent and coordination tools because they are the
//! only ones that reach GitHub or change the repository, and because their
//! descriptions have to carry the operational contract the schemas cannot: who
//! actually writes the pull request, and what a merge destroys.

use serde_json::{Value, json};

use super::{tool, tool_with_output};

pub(super) fn tools() -> Vec<Value> {
    vec![pr_status(), pr_request(), pr_update_branch(), merge()]
}

fn pr_status() -> Value {
    tool_with_output(
        "robco_pr_status",
        "Report the state of the pull requests opened from an agent's branch.",
        agent_id_schema(),
        json!({
            "type": "object",
            "properties": {
                "agent_id": { "type": "string" },
                "branch": { "type": "string" },
                "pr_state": {
                    "type": "string",
                    "enum": ["open", "merged", "closed_unmerged", "absent"],
                    "description":
                        "open: mergeable now. merged: already landed, so only cleanup is left. \
                         closed_unmerged: every PR was closed without merging, so the branch \
                         still holds work that is nowhere else. absent: the branch has never \
                         had a PR."
                }
            },
            "required": ["agent_id", "branch", "pr_state"],
            "additionalProperties": false
        }),
    )
}

fn pr_request() -> Value {
    tool(
        "robco_pr_request",
        "Ask an agent to open its pull request by sending the PR prompt to its session. \
         robco does not run `gh pr create` itself — the agent authors the PR, so the PR \
         appears only once that agent acts on the prompt. Refuses when the agent's session \
         is not running or its branch already has an open PR.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": { "type": "string" },
                "prompt": {
                    "type": "string",
                    "description":
                        "Text sent to the session. Omit to send the configured `pr_prompt`, \
                         which is what the TUI pre-fills its dialog with."
                }
            },
            "required": ["agent_id"],
            "additionalProperties": false
        }),
    )
}

fn pr_update_branch() -> Value {
    tool_with_output(
        "robco_pr_update_branch",
        "Bring an agent's pull request branch up to date with its base, the same action the \
         TUI's `u` key runs. Runs entirely on GitHub's own side (`gh pr update-branch`), so it \
         never touches the agent's worktree or the primary checkout, and unlike `robco_merge` \
         nothing here is destructive — there is no `confirm` gate. Refuses when the branch has \
         no open pull request. When the branch was actually behind, this also resets the \
         Overseer's own automated update budget for it, so a pull request the auto-merge gate \
         parked after spending that budget gets looked at again on the next pass.",
        agent_id_schema(),
        json!({
            "type": "object",
            "properties": {
                "agent_id": { "type": "string" },
                "branch": { "type": "string" },
                "outcome": {
                    "type": "string",
                    "enum": ["updated", "already_up_to_date"],
                    "description":
                        "updated: GitHub reported the branch behind its base and the update ran. \
                         already_up_to_date: nothing was sent."
                }
            },
            "required": ["agent_id", "branch", "outcome"],
            "additionalProperties": false
        }),
    )
}

fn merge() -> Value {
    tool_with_output(
        "robco_merge",
        "Merge an agent's pull request and clean up after it: merge, fast-forward the \
         primary worktree, remove the agent's worktree, delete the branch locally and on \
         the remote, kill its tmux sessions, and drop it from the registry. Destructive and \
         not undoable by robco, so `confirm` must be true. `merge_then_clean` requires an \
         open PR; `clean_only` requires an already-merged one and never calls `gh pr merge`. \
         A merge that fails returns `ok: false` with the step it stopped on rather than a \
         tool error. Refuses while another robco process is merging in the same repository.",
        json!({
            "type": "object",
            "properties": {
                "agent_id": { "type": "string" },
                "mode": {
                    "type": "string",
                    "enum": ["merge_then_clean", "clean_only"],
                    "default": "merge_then_clean",
                    "description":
                        "clean_only runs the post-merge sequence alone, for an agent whose \
                         PR was merged somewhere else."
                },
                "confirm": {
                    "type": "boolean",
                    "default": false,
                    "description": "Must be true. Acknowledges that the merge cannot be undone."
                }
            },
            "required": ["agent_id", "confirm"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "properties": {
                "ok": { "type": "boolean" },
                "agent_id": { "type": "string" },
                "branch": { "type": "string" },
                "mode": { "type": "string", "enum": ["merge_then_clean", "clean_only"] },
                "steps": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Steps that started, in order."
                },
                "failed_step": {
                    "type": ["string", "null"],
                    "description": "The step the sequence stopped on; null when it finished."
                },
                "error": { "type": ["string", "null"] }
            },
            "required": ["ok", "agent_id", "branch", "mode", "steps", "failed_step", "error"],
            "additionalProperties": false
        }),
    )
}

fn agent_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "agent_id": { "type": "string" } },
        "required": ["agent_id"],
        "additionalProperties": false
    })
}
