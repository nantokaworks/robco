use super::*;

#[test]
fn loads_a_config_that_still_carries_the_removed_enabled_key() {
    let raw = r#"{"enabled": false, "max_workers": 5}"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    assert!(!config.auto_merge);
}

/// dropr:476 — the polling dispatcher's own scheduling keys are retired: the
/// operator now picks work by name, so there is nothing left to schedule
/// (a daily cap, an author allowlist, a per-task retry cap, or per-repository
/// parallel slots) or a toggle to disable that scheduling with. A config file
/// written before this retirement still loads without error, and the next
/// save drops every retired key rather than round-tripping them forever.
#[test]
fn a_config_still_carrying_the_retired_dispatch_scheduling_keys_loads_and_drops_them() {
    let raw = r#"{
        "dispatch_enabled": true,
        "daily_dispatch_limit": 20,
        "dispatch_task_authors": ["someone"],
        "max_retries_per_task": 3,
        "parallel_limit": 2
    }"#;
    let config: OverseerConfig = serde_json::from_str(raw).unwrap();
    let serialized = serde_json::to_value(&config).unwrap();
    for key in [
        "dispatch_enabled",
        "daily_dispatch_limit",
        "dispatch_task_authors",
        "max_retries_per_task",
        "parallel_limit",
    ] {
        assert!(serialized.get(key).is_none(), "{key} must not round-trip");
    }
}

#[test]
fn a_config_written_before_retention_existed_loads_with_a_bounded_window() {
    let raw = r#"{"max_workers": 5}"#;
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
    assert!(value.get("auto_merge").is_some());
}
