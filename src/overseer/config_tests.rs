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
    // No per-event booleans were ever written, so every one of them defers
    // to `notify_level`, which itself defaults to `summary` — the same
    // effective "on" outcome the old hardcoded `true` default gave these two
    // events, just resolved through the level now instead of a literal bool.
    assert_eq!(config.notify_task_started, None);
    assert!(notify_admits(
        config.notify_task_started,
        config.notify_level,
        NotifyTier::Summary
    ));
    assert_eq!(config.notify_task_finished, None);
    assert!(notify_admits(
        config.notify_task_finished,
        config.notify_level,
        NotifyTier::Errors
    ));
    assert!(config.notify_localize);
    assert!(config.chat_category_ids.is_empty());
    assert_eq!(config.chat_concurrency_cap, 3);
    assert_eq!(config.channel_id.as_deref(), Some("1"));
}

#[test]
fn a_discord_config_missing_every_notify_key_falls_back_to_the_default_level() {
    let raw = r#"{"enabled": true, "channel_id": "1", "allowed_user_ids": ["9"]}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Summary);
}

#[test]
fn a_discord_config_that_explicitly_set_pr_opened_keeps_it_on_under_the_quieter_default_level() {
    let raw = r#"{"enabled": true, "notify_pr_opened": true}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Summary);
    assert!(notify_admits(
        config.notify_pr_opened,
        config.notify_level,
        NotifyTier::All
    ));
}

#[test]
fn a_fresh_discord_config_at_the_default_level_silences_pr_opened() {
    let config = DiscordConfig::default();
    assert_eq!(config.notify_pr_opened, None);
    assert!(!notify_admits(
        config.notify_pr_opened,
        config.notify_level,
        NotifyTier::All
    ));
}

#[test]
fn discord_config_round_trips_an_explicit_level() {
    let raw = r#"{"enabled": true, "notify_level": "errors"}"#;
    let config: DiscordConfig = serde_json::from_str(raw).unwrap();
    assert_eq!(config.notify_level, NotifyLevel::Errors);
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["notify_level"], "errors");
    // Unset booleans stay out of the serialized form entirely, so a fresh
    // level-only config does not resurrect the seven legacy keys.
    assert!(value.get("notify_pr_opened").is_none());
}

#[test]
fn serialized_config_carries_no_enabled_key() {
    let value = serde_json::to_value(OverseerConfig::default()).unwrap();
    assert!(value.get("enabled").is_none());
    assert!(value.get("dispatch_enabled").is_some());
}
