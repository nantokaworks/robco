//! Overseer daemon lifecycle actions exposed without any UI dependency.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::overseer::command::{self, StartAttempt, StopAttempt};

use super::super::{ToolResult, exec_err, invalid_params};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyArgs {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PanicStopArgs {
    #[serde(default)]
    confirm: bool,
}

pub(super) fn start(_args: EmptyArgs) -> ToolResult<Value> {
    let outcome = match command::start_daemon().map_err(exec_err)? {
        StartAttempt::NotInstalled => "not_installed",
        StartAttempt::Unsupported => "unsupported",
        StartAttempt::AlreadyRunning => "already_running",
        StartAttempt::Started => "started",
    };
    Ok(json!({ "ok": true, "outcome": outcome }))
}

pub(super) fn stop(_args: EmptyArgs) -> ToolResult<Value> {
    let outcome = match command::stop_daemon().map_err(exec_err)? {
        StopAttempt::NotRunning => "not_running",
        StopAttempt::Stopped => "stopped",
        StopAttempt::StillShuttingDown => "still_shutting_down",
    };
    Ok(json!({ "ok": true, "outcome": outcome }))
}

pub(super) fn panic_stop(args: PanicStopArgs) -> ToolResult<Value> {
    if !args.confirm {
        return Err(invalid_params(
            "confirm must be true: panic stop terminates every Overseer worker",
        ));
    }
    command::panic_stop_attributed("mcp", None).map_err(exec_err)?;
    Ok(json!({ "ok": true, "outcome": "panic_stopped" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::{ToolError, parse_args};

    #[test]
    fn panic_stop_requires_confirmation() {
        let args = parse_args(Some(json!({}))).unwrap();
        let error = panic_stop(args).unwrap_err();
        assert!(matches!(error, ToolError::InvalidParams(_)));
    }

    #[test]
    fn empty_daemon_args_are_strict() {
        assert!(parse_args::<EmptyArgs>(Some(json!({"extra": true}))).is_err());
    }
}
