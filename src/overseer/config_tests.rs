use super::*;

#[test]
fn loads_a_config_that_still_carries_the_removed_enabled_key() {
    let raw = r#"{"enabled": false, "dispatch_enabled": true, "max_workers": 5}"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    assert!(config.dispatch_enabled);
}

/// dropr:452 — `max_workers` / `per_repo_limit` retired in favor of
/// `parallel_limit`. A config file written before the slot model existed
/// still loads without error, and the next save drops both retired keys
/// rather than round-tripping them forever.
#[test]
fn a_config_still_carrying_the_retired_worker_caps_loads_and_drops_them() {
    let raw = r#"{"dispatch_enabled": true, "max_workers": 5, "per_repo_limit": 2}"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.parallel_limit, 0);
    let serialized = serde_json::to_value(&config).unwrap();
    assert!(serialized.get("max_workers").is_none());
    assert!(serialized.get("per_repo_limit").is_none());
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
    // No `notify_level` key was ever written, so it defaults to `summary`
    // — the same effective "on" outcome the old hardcoded `true` default
    // gave these two events, just resolved through the level now.
    assert!(config.notify_level.admits(NotifyTier::Summary));
    assert!(config.notify_level.admits(NotifyTier::Errors));
    assert!(config.notify_localize);
    assert!(config.chat_category_ids.is_empty());
    assert_eq!(config.chat_concurrency_cap, 3);
    assert_eq!(config.channel_id.as_deref(), Some("1"));
}

#[test]
fn a_discord_config_missing_notify_level_falls_back_to_the_default_level() {
    let raw = r#"{"enabled": true, "channel_id": "1", "allowed_user_ids": ["9"]}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Summary);
}

#[test]
fn a_legacy_discord_config_carrying_plain_boolean_notify_keys_still_loads() {
    let raw = r#"{"enabled": true, "notify_pr_opened": true, "notify_task_started": false}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Summary);
}

#[test]
fn a_post_334_discord_config_carrying_null_notify_keys_still_loads() {
    let raw = r#"{"enabled": true, "notify_pr_opened": null, "notify_task_started": null}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Summary);
}

#[test]
fn a_fresh_discord_config_at_the_default_level_silences_pr_opened() {
    let config = DiscordConfig::default();
    assert!(!config.notify_level.admits(NotifyTier::All));
}

#[test]
fn discord_config_round_trips_an_explicit_level() {
    let raw = r#"{"enabled": true, "notify_level": "errors"}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Errors);
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["notify_level"], "errors");
    // The seven legacy per-event keys no longer exist on the struct, so
    // they never resurface in the serialized form.
    assert!(value.get("notify_pr_opened").is_none());
}

#[test]
fn a_discord_config_written_before_the_notify_channel_existed_leaves_it_unset() {
    // Unset means reports keep falling back to `channel_id` — the exact
    // single-channel behavior every existing config was written under.
    let raw = r#"{"enabled": true, "channel_id": "1", "allowed_user_ids": ["9"]}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_channel_id, None);
}

#[test]
fn discord_config_round_trips_the_notify_channel() {
    let raw = r#"{"enabled": true, "channel_id": "1", "notify_channel_id": "2"}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_channel_id.as_deref(), Some("2"));
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["notify_channel_id"], "2");
}

#[test]
fn a_discord_config_written_before_channel_repo_bindings_existed_has_no_bindings() {
    let raw = r#"{"enabled": true, "channel_id": "1", "allowed_user_ids": ["9"]}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert!(config.channel_repo_bindings.is_empty());
}

#[test]
fn discord_config_round_trips_channel_repo_bindings() {
    let raw = r#"{"enabled": true, "channel_repo_bindings": {"111": "widgets"}}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(
        config.channel_repo_bindings.get("111").map(String::as_str),
        Some("widgets")
    );
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["channel_repo_bindings"]["111"], "widgets");
}

#[test]
fn serialized_config_carries_no_enabled_key() {
    let value = serde_json::to_value(OverseerConfig::default()).unwrap();
    assert!(value.get("enabled").is_none());
    assert!(value.get("dispatch_enabled").is_some());
}
