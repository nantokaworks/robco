use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use crate::Result;

use super::tools;

pub fn run_stdio() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_line(line: &str) -> Option<Value> {
    let request = match serde_json::from_str::<Value>(line) {
        Ok(request) => request,
        Err(err) => {
            return Some(error_response(
                Value::Null,
                -32700,
                format!("parse error: {err}"),
            ));
        }
    };
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str);

    match (id, method) {
        (None, Some("notifications/initialized")) => None,
        (None, _) => None,
        (Some(id), Some("initialize")) => Some(success_response(id, initialize_result())),
        (Some(id), Some("tools/list")) => Some(success_response(
            id,
            json!({
                "tools": tools::list_tools()
            }),
        )),
        (Some(id), Some("tools/call")) => Some(call_response(id, &request)),
        (Some(id), Some("ping")) => Some(success_response(id, json!({}))),
        (Some(id), Some(method)) => Some(error_response(
            id,
            -32601,
            format!("method not found: {method}"),
        )),
        (Some(id), None) => Some(error_response(id, -32600, "missing method")),
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "robco",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn call_response(id: Value, request: &Value) -> Value {
    match call_result(request) {
        Ok(result) => success_response(id, result),
        Err(message) => error_response(id, -32602, message),
    }
}

fn call_result(request: &Value) -> std::result::Result<Value, String> {
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err("missing tool name".to_string());
    };
    if name.trim().is_empty() {
        return Err("missing tool name".to_string());
    }
    let arguments = params.get("arguments").cloned();

    match tools::call_tool(name, arguments) {
        Ok(result) => Ok(tool_text(result, false)),
        Err(tools::ToolError::InvalidParams(message)) => Err(message),
        Err(tools::ToolError::Execution(message)) => {
            Ok(tool_text(json!({ "error": message }), true))
        }
    }
}

fn tool_text(value: Value, is_error: bool) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
        }],
        "isError": is_error
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_returns_server_info() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).unwrap();
        assert_eq!(response["result"]["serverInfo"]["name"], "robco");
        assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
    }

    #[test]
    fn tools_list_returns_seven_tools() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 7);
    }

    #[test]
    fn initialized_notification_is_ignored() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn tools_call_unknown_tool_is_invalid_params() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#,
        )
        .unwrap();
        assert_eq!(response["error"]["code"], -32602);
        assert!(response.get("result").is_none());
    }

    #[test]
    fn tools_call_blank_tool_name_is_invalid_params() {
        let response =
            handle_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":" "}}"#)
                .unwrap();
        assert_eq!(response["error"]["code"], -32602);
        assert!(response.get("result").is_none());
    }
}
