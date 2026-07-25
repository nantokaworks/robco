use super::*;
use crate::dropr::parse_as;

const INITIALIZED: &str = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;

fn outcome(stdout: &str) -> Option<ToolOutcome> {
    parse_tool_responses(stdout.as_bytes(), 1)?
        .into_iter()
        .next()
}

fn tasks(stdout: &str) -> Option<Vec<crate::dropr::DroprTaskCandidate>> {
    match outcome(stdout)? {
        ToolOutcome::Ok(payload) => {
            parse_as::<crate::dropr::DroprTaskCandidate>(&serde_json::to_vec(&payload).ok()?)
        }
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
    assert!(parse_tool_responses(b"not json\n", 1).is_none());
    assert!(parse_tool_responses(format!("{INITIALIZED}\n").as_bytes(), 1).is_none());
}

#[test]
fn a_refused_call_is_a_verdict_not_a_transport_fault() {
    // dropr answers a targeted claim it will not grant with a JSON-RPC error
    // whose message carries the machine-readable refusal.
    let stdout = format!(
        "{INITIALIZED}\n{}\n",
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"{\"code\":\"task_not_claimable\",\"reason\":\"claimed\"}"}}"#
    );
    let ToolOutcome::Refused(message) = outcome(&stdout).unwrap() else {
        panic!("expected a refusal");
    };
    assert!(message.contains("task_not_claimable"));
}

/// A batch is only useful if answer 2 is distinguishable from answer 1, so the
/// order the server replies in must not decide which caller gets which answer.
#[test]
fn a_batch_matches_answers_to_calls_by_id_not_arrival_order() {
    let second = r##"{"jsonrpc":"2.0","id":3,"result":{"structuredContent":{"tasks":[{"global_display_id":"#2","title":"Second"}]}}}"##;
    let first = r##"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"tasks":[{"global_display_id":"#1","title":"First"}]}}}"##;
    let stdout = format!("{INITIALIZED}\n{second}\n{first}\n");

    let answers = parse_tool_responses(stdout.as_bytes(), 2).unwrap();
    let titles = answers
        .iter()
        .map(|answer| match answer {
            ToolOutcome::Ok(payload) => payload["tasks"][0]["title"].as_str().unwrap().to_owned(),
            ToolOutcome::Refused(message) => message.clone(),
        })
        .collect::<Vec<_>>();
    assert_eq!(titles, ["First", "Second"]);
}

/// Half a batch is worse than none: the caller cannot tell which question went
/// unanswered, so a short session is a session fault.
#[test]
fn a_batch_missing_one_answer_is_a_fault_not_a_partial_result() {
    let stdout = format!(
        "{INITIALIZED}\n{}\n",
        r##"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"tasks":[]}}}"##
    );
    assert!(parse_tool_responses(stdout.as_bytes(), 2).is_none());
}

#[test]
fn a_batch_keeps_a_per_call_refusal_beside_a_successful_call() {
    let ok = r##"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"tasks":[]}}}"##;
    let refused =
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"unknown workspace"}}"#;
    let stdout = format!("{INITIALIZED}\n{ok}\n{refused}\n");

    let answers = parse_tool_responses(stdout.as_bytes(), 2).unwrap();
    assert!(matches!(answers[0], ToolOutcome::Ok(_)));
    let ToolOutcome::Refused(message) = &answers[1] else {
        panic!("expected the second call to be refused");
    };
    assert_eq!(message, "unknown workspace");
}

#[test]
fn an_empty_batch_asks_nothing_and_answers_nothing() {
    assert!(call_tools(&[], Duration::from_secs(1)).unwrap().is_empty());
}
