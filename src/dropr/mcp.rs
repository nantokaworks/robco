use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{Value, json};

use super::{DroprTaskCandidate, IN_PROGRESS_FETCH_LIMIT, parse_tasks};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
/// JSON-RPC id of the `tools/call` request; the initialize handshake owns id 1.
const CALL_ID: u64 = 2;

/// Result of a single `dropr mcp-stdio` tool call. `Refused` means the server
/// answered and declined — a claim another agent holds, a closed task — which
/// is a decision the caller must act on, not a transport fault.
pub(super) enum ToolOutcome {
    Ok(Value),
    Refused(String),
}

/// Calls one MCP tool over a throwaway `dropr mcp-stdio` session.
///
/// `None` means the call never reached a verdict: the binary is missing, the
/// process died, or it outlived `timeout`. Callers that gate a side effect on
/// the answer must treat that as unknown rather than as permission.
pub(super) fn call_tool(tool: &str, arguments: Value, timeout: Duration) -> Option<ToolOutcome> {
    let program = crate::config::resolve_program("dropr")?;
    let mut child = Command::new(program)
        .args(["mcp-stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "robco",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            },
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }),
        json!({
            "jsonrpc": "2.0",
            "id": CALL_ID,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": arguments,
            },
        }),
    ];

    let requests_written = (|| {
        let stdin = child.stdin.as_mut()?;
        for request in requests {
            serde_json::to_writer(&mut *stdin, &request).ok()?;
            stdin.write_all(b"\n").ok()?;
        }
        Some(())
    })();
    if requests_written.is_none() {
        terminate_child(&mut child);
        return None;
    }
    drop(child.stdin.take());

    let Some(stdout) = child.stdout.take() else {
        terminate_child(&mut child);
        return None;
    };
    let (sender, receiver) = mpsc::channel();
    let reader = match thread::Builder::new().spawn(move || {
        let mut stdout = stdout;
        let mut output = String::new();
        if stdout.read_to_string(&mut output).is_ok() {
            let _ = sender.send(output);
        }
    }) {
        Ok(reader) => reader,
        Err(_) => {
            terminate_child(&mut child);
            return None;
        }
    };

    let output = match receiver.recv_timeout(timeout) {
        Ok(output) => output,
        Err(_) => {
            terminate_child(&mut child);
            let _ = reader.join();
            return None;
        }
    };
    terminate_child(&mut child);
    let _ = reader.join();
    parse_tool_response(output.as_bytes())
}

pub(super) fn fetch_in_progress_tasks(workspace_id: &str) -> Option<Vec<DroprTaskCandidate>> {
    let payload = match call_tool(
        "task_list",
        json!({
            "workspace_id": workspace_id,
            "status": "in_progress",
            "limit": IN_PROGRESS_FETCH_LIMIT,
        }),
        RESPONSE_TIMEOUT,
    )? {
        ToolOutcome::Ok(payload) => payload,
        ToolOutcome::Refused(_) => return None,
    };
    parse_tasks(&serde_json::to_vec(&payload).ok()?)
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn parse_tool_response(stdout: &[u8]) -> Option<ToolOutcome> {
    let stdout = std::str::from_utf8(stdout).ok()?;
    for line in stdout.lines() {
        let response: Value = serde_json::from_str(line).ok()?;
        if response.get("id").and_then(Value::as_u64) != Some(CALL_ID) {
            continue;
        }
        // A refused call arrives as a JSON-RPC error; a tool that ran and failed
        // arrives as a result carrying `isError`. Both are verdicts.
        if let Some(error) = response.get("error") {
            return Some(ToolOutcome::Refused(refusal_text(error)));
        }
        let result = response.get("result")?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Some(ToolOutcome::Refused(refusal_text(result)));
        }

        let payload = if let Some(structured) = result.get("structuredContent") {
            structured.clone()
        } else {
            let text = result.get("content")?.get(0)?.get("text")?.as_str()?;
            serde_json::from_str(text).ok()?
        };
        return Some(ToolOutcome::Ok(payload));
    }
    None
}

/// Best-effort human-readable text for a refusal, preferring the server's
/// `message` field and falling back to the raw JSON.
fn refusal_text(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .get("content")?
                .get(0)?
                .get("text")?
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;

    fn tasks(stdout: &str) -> Option<Vec<DroprTaskCandidate>> {
        match parse_tool_response(stdout.as_bytes())? {
            ToolOutcome::Ok(payload) => parse_tasks(&serde_json::to_vec(&payload).ok()?),
            ToolOutcome::Refused(_) => None,
        }
    }

    #[test]
    fn parses_structured_content() {
        let stdout = format!(
            "{INITIALIZED}\n{}\n",
            r##"{"jsonrpc":"2.0","id":2,"result":{"content":[],"isError":false,"structuredContent":{"next_cursor":null,"tasks":[{"global_display_id":"#124","title":"Fix task fetch","priority":"high","status":"in_progress"}]}}}"##
        );

        let tasks = tasks(&stdout).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].display_id, "#124");
        assert_eq!(tasks[0].status, "in_progress");
    }

    #[test]
    fn parses_text_content_fallback() {
        let stdout = format!(
            "{INITIALIZED}\n{}\n",
            r##"{"jsonrpc":"2.0","id":2,"result":{"content":[{"text":"{\"next_cursor\":null,\"tasks\":[{\"global_display_id\":\"#7\",\"title\":\"Fallback\",\"priority\":\"medium\",\"status\":\"in_progress\"}]}","type":"text"}],"isError":false}}"##
        );

        let tasks = tasks(&stdout).unwrap();
        assert_eq!(tasks[0].display_id, "#7");
        assert_eq!(tasks[0].title, "Fallback");
    }

    #[test]
    fn rejects_tool_errors() {
        let stdout = format!(
            "{INITIALIZED}\n{}\n",
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[],"isError":true}}"#
        );
        assert!(tasks(&stdout).is_none());
    }

    #[test]
    fn rejects_garbage_and_missing_response() {
        assert!(parse_tool_response(b"not json\n").is_none());
        assert!(parse_tool_response(format!("{INITIALIZED}\n").as_bytes()).is_none());
    }

    #[test]
    fn a_refused_call_is_a_verdict_not_a_transport_fault() {
        // dropr answers a targeted claim it will not grant with a JSON-RPC error
        // whose message carries the machine-readable refusal.
        let stdout = format!(
            "{INITIALIZED}\n{}\n",
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"{\"code\":\"task_not_claimable\",\"reason\":\"claimed\"}"}}"#
        );
        let ToolOutcome::Refused(message) = parse_tool_response(stdout.as_bytes()).unwrap() else {
            panic!("expected a refusal");
        };
        assert!(message.contains("task_not_claimable"));
    }
}
