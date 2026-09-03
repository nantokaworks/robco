use std::{process::Command, time::Duration};

use serde_json::json;

use super::*;

const TOOLS: &str = concat!(
    "[{\"name\":\"robco_agent_list\"},{\"name\":\"robco_agent_kill\"},",
    "{\"name\":\"robco_agent_restart\"},{\"name\":\"robco_agent_land\"},",
    "{\"name\":\"robco_answer\"},{\"name\":\"robco_daemon_panic_stop\"},",
    "{\"name\":\"robco_daemon_start\"},{\"name\":\"robco_daemon_stop\"},",
    "{\"name\":\"robco_discovery_snapshot\"},{\"name\":\"robco_inbox_dismiss\"},",
    "{\"name\":\"robco_inbox_dismiss_all\"},{\"name\":\"robco_instruct\"},",
    "{\"name\":\"robco_overseer_snapshot\"},{\"name\":\"robco_pane_capture\"},",
    "{\"name\":\"robco_repo_checkout_main\"},{\"name\":\"robco_repo_clear_chat\"},",
    "{\"name\":\"robco_repo_rename\"}]"
);

fn shell(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", script]);
    command
}

fn server(after_handshake: &str) -> Command {
    shell(&format!(
        r#"read a
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"serverInfo":{{"version":"test"}}}}}}'
read b
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"tools":{TOOLS}}}}}'
{after_handshake}"#
    ))
}

#[test]
fn happy_path_reuses_the_handshake_process_for_tool_calls() {
    let command = server(
        r#"read c
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{\"ok\":true}"}],"isError":false}}'"#,
    );
    let client = RemoteClient::test_command(command, Duration::from_secs(1)).unwrap();
    assert_eq!(
        client.call("robco_agent_list", json!({})).unwrap(),
        json!({"ok": true})
    );
}

#[test]
fn connection_failure_is_distinct() {
    let error = RemoteClient::test_command(
        shell("echo 'ssh: connect to host bad: Connection refused' >&2; exit 255"),
        Duration::from_millis(200),
    )
    .err()
    .unwrap();
    assert!(matches!(error, RemoteError::Connect(_)));
}

#[test]
fn connection_timeout_is_bounded() {
    let started = std::time::Instant::now();
    let error = RemoteClient::test_command(shell("sleep 2"), Duration::from_millis(30))
        .err()
        .unwrap();
    assert!(matches!(error, RemoteError::Connect(_)));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn missing_remote_binary_is_distinct() {
    let error = RemoteClient::test_command(
        shell("echo 'sh: robco: command not found' >&2; exit 127"),
        Duration::from_millis(200),
    )
    .err()
    .unwrap();
    assert!(matches!(error, RemoteError::BinaryMissing(_)));
}

#[test]
fn missing_binary_classification_is_stable() {
    for _ in 0..25 {
        let error = RemoteClient::test_command(
            shell("echo 'sh: robco: command not found' >&2; exit 127"),
            Duration::from_millis(200),
        )
        .err()
        .unwrap();
        assert!(matches!(error, RemoteError::BinaryMissing(_)));
    }
}

#[test]
fn old_server_reports_missing_tools() {
    let command = shell(
        r#"read a
echo '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"version":"old"}}}'
read b
echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"robco_agent_list"}]}}'"#,
    );
    let error = RemoteClient::test_command(command, Duration::from_secs(1))
        .err()
        .unwrap();
    assert!(matches!(error, RemoteError::MissingTools(_)));
}

#[test]
fn mid_session_drop_is_distinct() {
    let client =
        RemoteClient::test_command(server("read c; exit 42"), Duration::from_secs(1)).unwrap();
    let error = client.call("robco_agent_list", json!({})).unwrap_err();
    assert!(matches!(error, RemoteError::Dropped(_)));
}
