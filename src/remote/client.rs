use std::{process::Command, time::Duration};

use serde_json::{Value, json};

use super::{RemoteError, transport::Transport};

const REQUIRED_TOOLS: [&str; 17] = [
    "robco_agent_list",
    "robco_agent_kill",
    "robco_agent_restart",
    "robco_agent_land",
    "robco_answer",
    "robco_daemon_panic_stop",
    "robco_daemon_start",
    "robco_daemon_stop",
    "robco_discovery_snapshot",
    "robco_inbox_dismiss",
    "robco_inbox_dismiss_all",
    "robco_instruct",
    "robco_overseer_snapshot",
    "robco_pane_capture",
    "robco_repo_checkout_main",
    "robco_repo_clear_chat",
    "robco_repo_rename",
];

#[derive(Clone)]
pub(crate) struct RemoteClient {
    transport: Transport,
}

impl RemoteClient {
    pub(crate) fn ssh(host: &str) -> Result<Self, RemoteError> {
        let mut command = Command::new("ssh");
        command.args([host, "robco", "mcp-stdio"]);
        Self::from_command(command, Duration::from_secs(5))
    }

    #[cfg(test)]
    pub(crate) fn test_command(command: Command, timeout: Duration) -> Result<Self, RemoteError> {
        Self::from_command(command, timeout)
    }

    fn from_command(command: Command, timeout: Duration) -> Result<Self, RemoteError> {
        let transport = Transport::from_command(command, timeout)?;
        if let Err(error) = transport.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "robco-remote-tui", "version": env!("CARGO_PKG_VERSION")}
            }),
        ) {
            transport.terminate();
            return Err(error);
        }
        let tools = match transport.request("tools/list", json!({})) {
            Ok(tools) => tools,
            Err(error) => {
                transport.terminate();
                return Err(error);
            }
        };
        let names = tools
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        let missing = REQUIRED_TOOLS
            .iter()
            .filter(|required| !names.contains(required))
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            transport.terminate();
            return Err(RemoteError::MissingTools(missing));
        }
        transport.mark_connected();
        Ok(Self { transport })
    }

    pub(crate) fn call(&self, tool: &str, arguments: Value) -> Result<Value, RemoteError> {
        let result = self
            .transport
            .request("tools/call", json!({"name": tool, "arguments": arguments}))?;
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| RemoteError::Protocol("tool result has no text content".into()))?;
        let value: Value = serde_json::from_str(text)
            .map_err(|error| RemoteError::Protocol(format!("invalid tool JSON: {error}")))?;
        if is_error {
            let message = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or(text)
                .to_string();
            Err(RemoteError::Tool {
                tool: tool.into(),
                message,
            })
        } else {
            Ok(value)
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
