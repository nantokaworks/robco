use std::{
    collections::HashMap,
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{Value, json};

/// JSON-RPC id of the first `tools/call` request; the initialize handshake owns
/// id 1 and each further call in the batch takes the next id.
const FIRST_CALL_ID: u64 = 2;

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
    call_tools(&[(tool, arguments)], timeout)?
        .into_iter()
        .next()
}

/// Calls several MCP tools over one `dropr mcp-stdio` session.
///
/// A session per call pays the spawn and the initialize handshake again for
/// every question, and `task_list` answers about one level of the task
/// hierarchy at a time — so a caller walking subtrees has several questions to
/// ask at once. Answers are matched by request id, so the returned vector lines
/// up with `calls` however the server orders its replies.
///
/// `None` carries the same meaning as in [`call_tool`], and covers the session
/// answering fewer calls than it was given: a batch is all-or-nothing, because
/// a caller cannot tell which answer went missing.
pub(super) fn call_tools(calls: &[(&str, Value)], timeout: Duration) -> Option<Vec<ToolOutcome>> {
    if calls.is_empty() {
        return Some(Vec::new());
    }
    let program = crate::config::resolve_program("dropr")?;
    let mut child = Command::new(program)
        .args(["mcp-stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut requests = vec![
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
    ];
    requests.extend(calls.iter().enumerate().map(|(index, (tool, arguments))| {
        json!({
            "jsonrpc": "2.0",
            "id": FIRST_CALL_ID + index as u64,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": arguments,
            },
        })
    }));

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
    parse_tool_responses(output.as_bytes(), calls.len())
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Pulls the `count` batched tool answers out of a session's stdout, in the
/// order they were asked rather than the order they arrived.
fn parse_tool_responses(stdout: &[u8], count: usize) -> Option<Vec<ToolOutcome>> {
    let stdout = std::str::from_utf8(stdout).ok()?;
    let mut answers: HashMap<u64, ToolOutcome> = HashMap::new();
    let call_ids = FIRST_CALL_ID..FIRST_CALL_ID + count as u64;
    for line in stdout.lines() {
        let response: Value = serde_json::from_str(line).ok()?;
        let Some(id) = response.get("id").and_then(Value::as_u64) else {
            continue;
        };
        if !call_ids.contains(&id) {
            continue;
        }
        answers.insert(id, response_outcome(&response)?);
    }
    call_ids.map(|id| answers.remove(&id)).collect()
}

/// The verdict one JSON-RPC response carries.
///
/// A refused call arrives as a JSON-RPC error; a tool that ran and failed
/// arrives as a result carrying `isError`. Both are verdicts. `None` is for a
/// response shaped like neither.
fn response_outcome(response: &Value) -> Option<ToolOutcome> {
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
    Some(ToolOutcome::Ok(payload))
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
#[path = "mcp_tests.rs"]
mod tests;
