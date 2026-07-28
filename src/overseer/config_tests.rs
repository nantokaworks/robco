use super::*;

#[test]
fn loads_a_config_that_still_carries_the_removed_enabled_key() {
    let raw = r#"{"enabled": false, "dispatch_enabled": true, "max_workers": 5}"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    assert!(config.dispatch_enabled);
    assert_eq!(config.max_workers, 5);
}

#[test]
fn a_config_written_before_retention_existed_loads_with_a_bounded_window() {
    let raw = r#"{"dispatch_enabled": true, "max_workers": 5}"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(
        config.terminal_retention_per_repo,
        OverseerConfig::default().terminal_retention_per_repo
    );
    assert_ne!(config.terminal_retention_per_repo, 0);
}

#[test]
fn a_discord_config_written_before_task_lifecycle_events_existed_defaults_them_on() {
    let raw = r#"{"enabled": true, "channel_id": "1", "allowed_user_ids": ["9"]}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert!(config.notify_task_started);
    assert!(config.notify_task_finished);
    assert!(config.notify_localize);
    assert!(config.chat_category_ids.is_empty());
    assert_eq!(config.chat_concurrency_cap, 3);
    assert_eq!(config.channel_id.as_deref(), Some("1"));
}

#[test]
fn serialized_config_carries_no_enabled_key() {
    let value = serde_json::to_value(OverseerConfig::default()).unwrap();
    assert!(value.get("enabled").is_none());
    assert!(value.get("dispatch_enabled").is_some());
}
