use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

use crate::Result;

use super::tools;

/// Protocol revisions this server answers. Order carries no meaning beyond
/// display; negotiation and the mismatch check both search the whole slice.
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] = ["2026-07-28", "2024-11-05"];

/// What `initialize` answered before per-request negotiation existed. Kept as
/// the fallback for a handshake that names no version, or one this server
/// does not support, so pre-2026-07-28 clients see no behavioural change.
const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";

/// `tools/list` reflects the binary's compiled-in tool set, so it only goes
/// stale on upgrade; a long TTL lets a client cache it for a whole session.
const TOOLS_LIST_TTL_MS: u64 = 3_600_000;

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

    // `server/discover` is the version probe itself, so it must answer even
    // when `_meta` names a version this server does not support.
    if method != Some("server/discover")
        && let Some(id) = id.clone()
        && let Some(error) = meta_version_error(&request, id)
    {
        return Some(error);
    }

    match (id, method) {
        (None, Some("notifications/initialized")) => None,
        (None, _) => None,
        (Some(id), Some("initialize")) => Some(success_response(id, initialize_result(&request))),
        (Some(id), Some("server/discover")) => Some(success_response(id, discover_result())),
        (Some(id), Some("tools/list")) => Some(success_response(id, tools_list_result())),
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

/// Checks a request's stateless `_meta` envelope for a protocol version this
/// server cannot serve. Absent `_meta`, or `_meta` naming no version, is not
/// an error — the legacy `initialize` handshake covers that case instead.
fn meta_version_error(request: &Value, id: Value) -> Option<Value> {
    let meta = request.get("params")?.get("_meta")?;
    // Client identity travels alongside the protocol version under the
    // stateless request shape. robco has no per-client capability gating
    // today, so it is read but not consulted further — mirroring how the
    // legacy `initialize` handshake's own `clientInfo` goes unused already.
    let _client_info = meta.get("io.modelcontextprotocol/clientInfo");
    let _client_capabilities = meta.get("io.modelcontextprotocol/clientCapabilities");

    let requested = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)?;
    if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
        None
    } else {
        Some(unsupported_version_response(id, requested))
    }
}

fn unsupported_version_response(id: Value, requested: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32022,
            "message": format!("unsupported protocol version: {requested}"),
            "data": {
                "supported": SUPPORTED_PROTOCOL_VERSIONS,
                "requested": requested
            }
        }
    })
}

fn initialize_result(request: &Value) -> Value {
    let requested = request
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str);
    json!({
        "protocolVersion": negotiate_version(requested),
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "robco",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// Picks the version to answer `initialize` with: the client's requested
/// version when this server supports it, otherwise the legacy default —
/// never an error, since the handshake predates per-request negotiation.
fn negotiate_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|version| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|supported| *supported == version)
        })
        .unwrap_or(DEFAULT_PROTOCOL_VERSION)
}

fn discover_result() -> Value {
    json!({
        "protocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
        "capabilities": {
            "tools": {},
            "extensions": {}
        },
        "serverInfo": {
            "name": "robco",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": tools::list_tools(),
        "ttlMs": TOOLS_LIST_TTL_MS,
        "cacheScope": "private"
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

/// Wraps a result in the JSON-RPC envelope, stamping `resultType: "complete"`
/// centrally so no method can forget it. robco issues no server-initiated
/// requests, so every result it returns is complete — the `"input_required"`
/// half of the revision's `resultType` values does not apply here.
fn success_response(id: Value, mut result: Value) -> Value {
    if let Value::Object(map) = &mut result {
        map.insert("resultType".to_string(), json!("complete"));
    }
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
#[path = "server_tests.rs"]
mod tests;
