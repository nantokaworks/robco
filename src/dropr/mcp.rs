use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use serde_json::{Value, json};

use super::{DroprTaskCandidate, parse_tasks};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) fn fetch_in_progress_tasks(workspace_id: &str) -> Option<Vec<DroprTaskCandidate>> {
    let mut child = Command::new("dropr")
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
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "task_list",
                "arguments": {
                    "workspace_id": workspace_id,
                    "status": "in_progress",
                    "limit": 3,
                },
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

    let output = match receiver.recv_timeout(RESPONSE_TIMEOUT) {
        Ok(output) => output,
        Err(_) => {
            terminate_child(&mut child);
            let _ = reader.join();
            return None;
        }
    };
    terminate_child(&mut child);
    let _ = reader.join();
    parse_task_list_response(output.as_bytes())
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn parse_task_list_response(stdout: &[u8]) -> Option<Vec<DroprTaskCandidate>> {
    let stdout = std::str::from_utf8(stdout).ok()?;
    for line in stdout.lines() {
        let response: Value = serde_json::from_str(line).ok()?;
        if response.get("id").and_then(Value::as_u64) != Some(2) {
            continue;
        }
        if response
            .get("result")
            .and_then(|result| result.get("isError"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return None;
        }

        let result = response.get("result")?;
        let payload = if let Some(structured) = result.get("structuredContent") {
            structured.clone()
        } else {
            let text = result.get("content")?.get(0)?.get("text")?.as_str()?;
            serde_json::from_str(text).ok()?
        };
        return parse_tasks(&serde_json::to_vec(&payload).ok()?);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;

    #[test]
    fn parses_structured_content() {
        let stdout = format!(
            "{INITIALIZED}\n{}\n",
            r##"{"jsonrpc":"2.0","id":2,"result":{"content":[],"isError":false,"structuredContent":{"next_cursor":null,"tasks":[{"global_display_id":"#124","title":"Fix task fetch","priority":"high","status":"in_progress"}]}}}"##
        );

        let tasks = parse_task_list_response(stdout.as_bytes()).unwrap();
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

        let tasks = parse_task_list_response(stdout.as_bytes()).unwrap();
        assert_eq!(tasks[0].display_id, "#7");
        assert_eq!(tasks[0].title, "Fallback");
    }

    #[test]
    fn rejects_tool_errors() {
        let stdout = format!(
            "{INITIALIZED}\n{}\n",
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[],"isError":true}}"#
        );
        assert!(parse_task_list_response(stdout.as_bytes()).is_none());
    }

    #[test]
    fn rejects_garbage_and_missing_response() {
        assert!(parse_task_list_response(b"not json\n").is_none());
        assert!(parse_task_list_response(format!("{INITIALIZED}\n").as_bytes()).is_none());
    }
}
