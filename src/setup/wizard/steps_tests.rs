use std::io::Cursor;

use crate::config::Config;

use super::steps;

#[test]
fn overseer_all_enter_leaves_configuration_unchanged() {
    let mut config = Config::default();
    let before = serde_json::to_vec(&config).unwrap();
    let mut input = Cursor::new(b"\n\n\n\n\n\n\n");
    steps::overseer(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(serde_json::to_vec(&config).unwrap(), before);
}

#[test]
fn unavailable_current_profile_is_preserved_by_default() {
    let mut config = Config::default();
    config.overseer.worker_profile = Some("custom-missing".into());
    let mut input = Cursor::new(b"\n\n\n\n\n\n\n");
    steps::overseer(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(
        config.overseer.worker_profile.as_deref(),
        Some("custom-missing")
    );
}

#[test]
fn overseer_answers_are_applied() {
    let mut config = Config::default();
    let mut input = Cursor::new(b"n\ny\n2\n3\n5\n2\n42\n");
    let mut output = Vec::new();
    steps::overseer(&mut input, &mut output, &mut config).unwrap();
    assert!(!config.overseer.dispatch_enabled);
    assert!(config.overseer.auto_merge);
    assert_eq!(config.overseer.worker_profile.as_deref(), Some("claude"));
    assert_eq!(config.overseer.triage_profile.as_deref(), Some("codex"));
    assert_eq!(config.overseer.max_workers, 5);
    assert_eq!(config.overseer.per_repo_limit, 2);
    assert_eq!(config.overseer.daily_dispatch_limit, 42);
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("branch protection")
    );
}

#[test]
fn discord_validators_reject_secrets_and_bad_ids() {
    assert!(steps::digits("123"));
    assert!(!steps::digits("12x"));
    assert!(steps::valid_user_ids("1, 2"));
    assert!(!steps::valid_user_ids("1,"));
    assert!(steps::env_name("ROBCO_DISCORD_TOKEN"));
    assert!(!steps::env_name("token-value!"));
}

#[test]
fn discord_answers_are_applied_after_retries() {
    let mut config = Config::default();
    let mut input =
        Cursor::new(b"y\nbad\n123\n\n1, nope\n10,20\nsecret-value!\nMY_DISCORD_TOKEN\n");
    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(config.overseer.discord.channel_id.as_deref(), Some("123"));
    assert_eq!(config.overseer.discord.allowed_user_ids, ["10", "20"]);
    assert_eq!(config.overseer.discord.token_env, "MY_DISCORD_TOKEN");
}
