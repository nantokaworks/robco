//! Thin typed calls for actions a future remote TUI row can dispatch.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{RemoteClient, RemoteError};

#[derive(Debug, Deserialize)]
pub(crate) struct SimpleOutcome {
    pub(crate) ok: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct KillOutcome {
    pub(crate) ok: bool,
    pub(crate) branch_remains: bool,
    pub(crate) branch_deleted: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CheckoutOutcome {
    pub(crate) ok: bool,
    pub(crate) branch: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UnrepairedWorktree {
    pub(crate) path: String,
    pub(crate) error: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameOutcome {
    pub(crate) ok: bool,
    pub(crate) new_path: String,
    pub(crate) unrepaired_worktrees: Vec<UnrepairedWorktree>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LandOutcome {
    pub(crate) ok: bool,
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) failed_checks: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonOutcome {
    pub(crate) ok: bool,
    pub(crate) outcome: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DismissAllOutcome {
    pub(crate) ok: bool,
    pub(crate) dismissed_count: usize,
}

#[allow(dead_code)]
impl RemoteClient {
    pub(crate) fn kill_agent(
        &self,
        agent_id: &str,
        force: bool,
        delete_branch: bool,
    ) -> Result<KillOutcome, RemoteError> {
        self.action(
            "robco_agent_kill",
            json!({
                "agent_id": agent_id,
                "force": force,
                "delete_branch": delete_branch,
                "confirm": true
            }),
        )
    }

    pub(crate) fn restart_agent(&self, agent_id: &str) -> Result<SimpleOutcome, RemoteError> {
        self.action("robco_agent_restart", json!({ "agent_id": agent_id }))
    }

    pub(crate) fn checkout_main(&self, repo_path: &str) -> Result<CheckoutOutcome, RemoteError> {
        self.action(
            "robco_repo_checkout_main",
            json!({ "repo_path": repo_path }),
        )
    }

    pub(crate) fn clear_chat(&self, repo_path: &str) -> Result<SimpleOutcome, RemoteError> {
        self.action(
            "robco_repo_clear_chat",
            json!({ "repo_path": repo_path, "confirm": true }),
        )
    }

    pub(crate) fn rename_repo(
        &self,
        repo_path: &str,
        new_name: &str,
    ) -> Result<RenameOutcome, RemoteError> {
        self.action(
            "robco_repo_rename",
            json!({ "repo_path": repo_path, "new_name": new_name }),
        )
    }

    pub(crate) fn land_agent(&self, agent_id: &str) -> Result<LandOutcome, RemoteError> {
        self.action(
            "robco_agent_land",
            json!({ "agent_id": agent_id, "confirm": true }),
        )
    }

    pub(crate) fn start_daemon(&self) -> Result<DaemonOutcome, RemoteError> {
        self.action("robco_daemon_start", json!({}))
    }

    pub(crate) fn stop_daemon(&self) -> Result<DaemonOutcome, RemoteError> {
        self.action("robco_daemon_stop", json!({}))
    }

    pub(crate) fn panic_stop_daemon(&self) -> Result<DaemonOutcome, RemoteError> {
        self.action("robco_daemon_panic_stop", json!({ "confirm": true }))
    }

    pub(crate) fn dismiss_inbox(
        &self,
        kind: &str,
        target_id: &str,
    ) -> Result<SimpleOutcome, RemoteError> {
        self.action(
            "robco_inbox_dismiss",
            json!({ "kind": kind, "target_id": target_id }),
        )
    }

    pub(crate) fn dismiss_all_inbox(&self) -> Result<DismissAllOutcome, RemoteError> {
        self.action("robco_inbox_dismiss_all", json!({ "confirm": true }))
    }

    pub(crate) fn answer_agent(
        &self,
        agent_id: &str,
        text: &str,
    ) -> Result<SimpleOutcome, RemoteError> {
        self.action(
            "robco_answer",
            json!({ "agent_id": agent_id, "text": text }),
        )
    }

    pub(crate) fn instruct_session(
        &self,
        session: &str,
        text: &str,
    ) -> Result<SimpleOutcome, RemoteError> {
        self.action(
            "robco_instruct",
            json!({ "session": session, "text": text }),
        )
    }

    fn action<T: DeserializeOwned>(&self, tool: &str, arguments: Value) -> Result<T, RemoteError> {
        let value = self.call(tool, arguments)?;
        serde_json::from_value(value)
            .map_err(|error| RemoteError::Protocol(format!("invalid {tool} result: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_outcomes_require_their_contract_fields() {
        assert!(serde_json::from_value::<SimpleOutcome>(json!({})).is_err());
        assert!(serde_json::from_value::<DaemonOutcome>(json!({"ok":true})).is_err());
    }
}
