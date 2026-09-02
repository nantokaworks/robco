use super::*;
use std::{process::Command, sync::Arc, time::Duration};

use crate::{
    config::Config,
    model::HostLabel,
    registry::Registry,
    remote::RemoteClient,
    ui::{actions::remote_hosts::HostSlot, backend::RemoteBackend},
};

#[test]
fn cached_preview_only_matches_the_last_completed_target() {
    let mut capture = PreviewCapture::new();
    capture.current = Some((
        CaptureTarget::Tmux {
            session: "worker".into(),
            width: 80,
            height: 24,
            offset: 0,
        },
        Some(Text::raw("pane")),
    ));

    assert_eq!(cached_tmux(&capture, "worker"), Some(Text::raw("pane")));
    assert_eq!(cached_tmux(&capture, "other"), None);
    assert_eq!(cached_diff(&capture, std::path::Path::new("repo")), None);
}

#[test]
fn remote_selection_never_queues_a_local_capture() {
    let temp = tempfile::tempdir().unwrap();
    let repo = serde_json::from_value(serde_json::json!({
        "path": "/remote/repo", "name": "remote", "remote_url": null,
        "pinned": true
    }))
    .unwrap();
    let mut app = App::new(
        Registry {
            version: 1,
            repos: vec![repo],
        },
        Config::default(),
        temp.path().into(),
    );
    let host = HostLabel {
        name: "Remote".into(),
        ssh: "remote".into(),
    };
    app.registry.repos[0].host = Some(host.clone());
    let tools = [
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
    ]
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
    let client = RemoteClient::test_command(command, Duration::from_millis(100)).unwrap();
    let backend = Arc::new(RemoteBackend::test(client));
    app.hosts = vec![HostSlot::with_backend(host, Arc::clone(&backend))];
    app.overseer_visible = false;
    app.preview = PreviewPane::Claude;

    app.schedule_preview_capture(Rect::new(0, 0, 120, 40));

    assert!(app.preview_capture.last_target.is_none());
    let (session, width, height, offset) = backend.test_pane_target().unwrap();
    assert_eq!(session, "robco_remote_main");
    assert!(width > 0);
    assert!(height > 0);
    assert_eq!(offset, 0);
}
