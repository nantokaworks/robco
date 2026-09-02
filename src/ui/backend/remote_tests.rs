use super::*;
use std::process::Command;

const TOOLS: [&str; 17] = [
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

fn idle_backend() -> RemoteBackend {
    let tools = TOOLS
        .into_iter()
        .map(|name| serde_json::json!({ "name": name }))
        .collect::<Vec<_>>();
    let script = format!(
        r#"read a
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"serverInfo":{{"version":"test"}}}}}}'
read b
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"tools":{}}}}}'
read c
sleep 1"#,
        serde_json::to_string(&tools).unwrap()
    );
    let mut command = Command::new("sh");
    command.args(["-c", &script]);
    RemoteBackend {
        client: RemoteClient::test_command(command, Duration::from_millis(100)).unwrap(),
        pane: Arc::new(Mutex::new(PaneCache::default())),
        error: Arc::new(Mutex::new(None)),
    }
}

#[test]
fn pane_cache_reads_match_sessions_without_requiring_equal_dimensions() {
    let pane = PaneCache {
        target: Some(("worker".into(), 120, 40, 9)),
        text: Some(Text::raw("pane")),
        ..PaneCache::default()
    };

    assert_eq!(cached_pane(&pane, "worker"), Some(Text::raw("pane")));
    assert_eq!(cached_pane(&pane, "other"), None);
}

#[test]
fn scheduling_state_marks_a_remote_capture_in_flight() {
    let backend = idle_backend();

    backend.schedule_remote_pane("worker", 120, 40, 9);

    let pane = backend.pane.lock().unwrap();
    assert!(pane.in_flight);
    assert_eq!(pane.target, Some(("worker".into(), 120, 40, 9)));
}

#[test]
fn same_target_refresh_keeps_previous_text_while_capture_runs() {
    let backend = idle_backend();
    {
        let mut pane = backend.pane.lock().unwrap();
        pane.target = Some(("worker".into(), 120, 40, 9));
        pane.text = Some(Text::raw("previous"));
    }

    backend.schedule_remote_pane("worker", 120, 40, 9);

    let pane = backend.pane.lock().unwrap();
    assert!(pane.in_flight);
    assert_eq!(pane.text, Some(Text::raw("previous")));
}
