use super::*;

#[test]
fn initialize_returns_server_info() {
    let response = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).unwrap();
    assert_eq!(response["result"]["serverInfo"]["name"], "robco");
    assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
}

#[test]
fn initialize_with_no_version_negotiates_the_legacy_default() {
    let response = handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).unwrap();
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn initialize_requesting_2024_11_05_sees_no_behavioural_change() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05"}}"#,
    )
    .unwrap();
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn initialize_requesting_the_2026_revision_negotiates_it() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2026-07-28"}}"#,
    )
    .unwrap();
    assert_eq!(response["result"]["protocolVersion"], "2026-07-28");
}

#[test]
fn initialize_requesting_an_unknown_version_falls_back_to_the_default() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"1900-01-01"}}"#,
    )
    .unwrap();
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
}

#[test]
fn tools_list_returns_every_tool() {
    let response = handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 18);
}

#[test]
fn tools_list_carries_ttl_and_cache_scope() {
    let response = handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    assert_eq!(response["result"]["ttlMs"], TOOLS_LIST_TTL_MS);
    assert_eq!(response["result"]["cacheScope"], "private");
}

#[test]
fn tools_list_ordering_is_stable_across_calls() {
    let first = handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    let second = handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
    let names = |response: &Value| {
        response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&first), names(&second));
    assert_eq!(names(&first)[0], "robco_whoami");
}

#[test]
fn every_success_result_carries_result_type() {
    for line in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"server/discover"}"#,
    ] {
        let response = handle_line(line).unwrap();
        assert_eq!(response["result"]["resultType"], "complete", "for {line}");
    }
}

#[test]
fn initialized_notification_is_ignored() {
    assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
}

#[test]
fn tools_call_unknown_tool_is_invalid_params() {
    let response =
        handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#)
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

#[test]
fn stateless_request_with_2026_meta_and_no_handshake_is_served() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/list",
            "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
    )
    .unwrap();
    assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 18);
}

#[test]
fn stateless_request_with_client_identity_in_meta_is_served() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/list",
            "params":{"_meta":{
                "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                "io.modelcontextprotocol/clientInfo":{"name":"claude","version":"1.0"},
                "io.modelcontextprotocol/clientCapabilities":{}
            }}}"#,
    )
    .unwrap();
    assert_eq!(response["result"]["resultType"], "complete");
}

#[test]
fn unsupported_meta_version_is_refused_with_supported_list() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/list",
            "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}"#,
    )
    .unwrap();
    assert_eq!(response["error"]["code"], -32022);
    assert_eq!(response["error"]["data"]["requested"], "1900-01-01");
    assert_eq!(
        response["error"]["data"]["supported"],
        json!(SUPPORTED_PROTOCOL_VERSIONS)
    );
    assert!(response.get("result").is_none());
}

#[test]
fn discover_answers_without_any_prior_handshake() {
    let response = handle_line(r#"{"jsonrpc":"2.0","id":6,"method":"server/discover"}"#).unwrap();
    assert_eq!(
        response["result"]["protocolVersions"],
        json!(SUPPORTED_PROTOCOL_VERSIONS)
    );
    assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
    assert_eq!(response["result"]["serverInfo"]["name"], "robco");
}

#[test]
fn discover_answers_even_with_an_unsupported_meta_version() {
    let response = handle_line(
        r#"{"jsonrpc":"2.0","id":6,"method":"server/discover",
            "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}"#,
    )
    .unwrap();
    assert_eq!(response["result"]["serverInfo"]["name"], "robco");
    assert!(response.get("error").is_none());
}

#[test]
fn ping_is_still_answered() {
    let response = handle_line(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#).unwrap();
    assert_eq!(response["result"]["resultType"], "complete");
}
