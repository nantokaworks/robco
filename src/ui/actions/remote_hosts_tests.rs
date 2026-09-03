use super::*;
use std::{process::Command, time::Duration};

use crate::{
    model::Selection, registry::Registry, remote::RemoteClient, ui::backend::RemoteBackend,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

fn test_backend(marker: &std::path::Path) -> Arc<RemoteBackend> {
    let script = format!(
        r#"read a
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"serverInfo":{{"version":"test"}}}}}}'
read b
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"tools":{TOOLS}}}}}'
read c
printf '%s' "$c" > '{}'
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"content":[{{"type":"text","text":"{{\"ok\":true}}"}}],"isError":false}}}}'"#,
        marker.display()
    );
    let mut command = Command::new("sh");
    command.args(["-c", &script]);
    let client = RemoteClient::test_command(command, Duration::from_secs(1)).unwrap();
    Arc::new(RemoteBackend::test(client))
}

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = false;
    app
}

fn channels(first_active: &str, second_active: &str) -> DiscordChannels {
    serde_json::from_value(serde_json::json!({"channels": {
        "first": {"first_seen_at":"2025-01-01T00:00:00Z","last_active_at":first_active,
            "turn_count":1,"status":"idle","last_error":null,"channel_name":"first"},
        "second": {"first_seen_at":"2025-01-01T00:00:00Z","last_active_at":second_active,
            "turn_count":1,"status":"idle","last_error":null,"channel_name":"second"}
    }}))
    .unwrap()
}

#[test]
fn publish_tags_repos_and_preserves_last_success_on_error() {
    let cell = Mutex::new(HostSnapshot::default());
    let label = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let repo = serde_json::from_value(serde_json::json!({
        "path": "/srv/repo", "name": "repo", "remote_url": null
    }))
    .unwrap();
    let mut channels = DiscordChannels::default();
    channels.channels.insert(
        "42".into(),
        serde_json::from_value(serde_json::json!({
            "first_seen_at": "2025-01-01T00:00:00Z",
            "last_active_at": "2025-01-01T00:00:00Z",
            "turn_count": 1, "status": "idle", "last_error": null
        }))
        .unwrap(),
    );
    publish(
        &cell,
        &label,
        vec![repo],
        Some(Status::Waiting),
        channels.clone(),
        false,
        None,
    );
    publish_error(&cell, "offline".into());
    let snapshot = cell.lock().unwrap();
    assert_eq!(snapshot.repos[0].host.as_ref(), Some(&label));
    assert_eq!(snapshot.repos.len(), 1);
    assert_eq!(snapshot.error.as_deref(), Some("offline"));
    assert_eq!(snapshot.control_status, Some(Status::Waiting));
    assert_eq!(snapshot.discord_channels, channels);
    assert!(!snapshot.daemon_alive);
}

#[test]
fn connection_tracks_first_success_and_failures() {
    let label = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let slot = HostSlot::idle(label.clone());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Connecting, None)
    );

    publish(
        &slot.snapshot,
        &label,
        Vec::new(),
        None,
        DiscordChannels::default(),
        true,
        None,
    );
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Connected, None)
    );

    publish_error(&slot.snapshot, "offline".into());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Failed, Some("offline".into()))
    );
}

#[test]
fn failed_first_publish_is_failed_despite_advanced_generation() {
    let slot = HostSlot::idle(HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    });
    publish_error(&slot.snapshot, "offline".into());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Failed, Some("offline".into()))
    );
}

#[test]
fn poisoned_snapshot_remains_readable_and_writable() {
    let label = HostLabel {
        name: "Prod".into(),
        ssh: "prod".into(),
    };
    let slot = HostSlot::idle(label.clone());
    let snapshot = Arc::clone(&slot.snapshot);
    let _ = std::panic::catch_unwind(|| {
        let _guard = snapshot.lock().unwrap();
        panic!("poison snapshot");
    });

    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Connecting, None)
    );
    assert!(slot.backend().is_none());
    publish(
        &slot.snapshot,
        &label,
        Vec::new(),
        None,
        DiscordChannels::default(),
        true,
        None,
    );
    publish_error(&slot.snapshot, "offline".into());
    assert_eq!(
        slot.connection_and_error(),
        (HostConnection::Failed, Some("offline".into()))
    );
}

#[test]
fn remote_control_prompt_keeps_the_host_that_opened_it() {
    let temp = tempfile::tempdir().unwrap();
    let first_marker = temp.path().join("first");
    let second_marker = temp.path().join("second");
    let mut app = test_app();
    app.hosts = [("first", &first_marker), ("second", &second_marker)]
        .into_iter()
        .map(|(name, marker)| {
            HostSlot::with_backend(
                HostLabel {
                    name: name.into(),
                    ssh: name.into(),
                },
                test_backend(marker),
            )
        })
        .collect();
    app.sync_remote_host_views();
    let rows = app.visible();
    app.selected = rows
        .iter()
        .position(|row| *row == Selection::RemoteControlAi(1))
        .unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
        .unwrap();
    app.selected = rows
        .iter()
        .position(|row| *row == Selection::RemoteControlAi(0))
        .unwrap();

    for code in [KeyCode::Char('g'), KeyCode::Char('o'), KeyCode::Enter] {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap();
    }

    assert!(!first_marker.exists());
    let request = std::fs::read_to_string(second_marker).unwrap();
    assert!(request.contains("robco_instruct"));
    assert!(request.contains("go"));
}

#[test]
fn remote_channel_actions_use_the_ingested_order_until_the_next_ingest() {
    let mut app = test_app();
    app.hosts = vec![HostSlot::connected_with_chats(
        HostLabel {
            name: "host".into(),
            ssh: "host".into(),
        },
        None,
        channels("2025-01-03T00:00:00Z", "2025-01-02T00:00:00Z"),
        true,
    )];
    app.sync_remote_host_views();
    let row = Selection::RemoteDiscordChannel {
        host: 0,
        channel: 0,
    };
    app.selected = app.visible().iter().position(|item| *item == row).unwrap();
    let key = app.item_key(row);

    app.hosts[0].replace_chats(channels("2025-01-03T00:00:00Z", "2025-01-04T00:00:00Z"));
    let (_, session) = crate::ui::input::remote_chat_target(&app, row).unwrap();
    assert!(session.ends_with("discord-first"));
    assert_eq!(app.item_key(app.selected_item().unwrap()), key);

    app.ingest_remote_hosts();
    assert_eq!(app.item_key(app.selected_item().unwrap()), key);
    assert_eq!(
        app.selected_item(),
        Some(Selection::RemoteDiscordChannel {
            host: 0,
            channel: 1
        })
    );
}
