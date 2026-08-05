use std::io::Cursor;

use crate::config::Config;

use super::steps_discord as steps;

#[test]
fn discord_validators_reject_secrets_and_bad_ids() {
    assert!(steps::digits("123"));
    assert!(!steps::digits("12x"));
    assert!(steps::clear_or_digits(""));
    assert!(steps::clear_or_digits("-"));
    assert!(steps::clear_or_digits("123"));
    assert!(!steps::clear_or_digits("12x"));
    assert!(steps::clear_or_id_list(""));
    assert!(steps::clear_or_id_list("-"));
    assert!(steps::clear_or_id_list("1, 2"));
    assert!(!steps::clear_or_id_list("1,"));
    assert!(steps::env_name("ROBCO_DISCORD_TOKEN"));
    assert!(!steps::env_name("token-value!"));
}

#[test]
fn discord_answers_are_applied_after_retries() {
    let mut config = Config::default();
    let mut input = Cursor::new(
        b"y\n\nbad\n123\n45x\n456\n1, nope\n10,20\n1,\n30,40\nsecret-value!\nMY_DISCORD_TOKEN\n\n",
    );
    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(config.overseer.discord.channel_id.as_deref(), Some("123"));
    assert_eq!(
        config.overseer.discord.notify_channel_id.as_deref(),
        Some("456")
    );
    assert_eq!(config.overseer.discord.allowed_user_ids, ["10", "20"]);
    assert_eq!(config.overseer.discord.chat_category_ids, ["30", "40"]);
    assert_eq!(config.overseer.discord.token_env, "MY_DISCORD_TOKEN");
}

/// Populates every optional Discord field so keep/clear tests can show which
/// answers change them.
fn configured() -> Config {
    let mut config = Config::default();
    let discord = &mut config.overseer.discord;
    discord.enabled = true;
    discord.channel_id = Some("123".into());
    discord.notify_channel_id = Some("456".into());
    discord.allowed_user_ids = vec!["10".into(), "20".into()];
    discord.chat_category_ids = vec!["30".into()];
    config
}

#[test]
fn discord_blank_answers_keep_current_values() {
    let mut config = configured();
    let before = serde_json::to_vec(&config).unwrap();
    let mut input = Cursor::new(b"\n\n\n\n\n\n\n\n");
    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(serde_json::to_vec(&config).unwrap(), before);
}

#[test]
fn discord_dash_clears_optional_values_and_warns() {
    let mut config = configured();
    let mut input = Cursor::new(b"\n\n-\n-\n-\n-\n\n\n");
    let mut output = Vec::new();
    steps::discord(&mut input, &mut output, &mut config).unwrap();
    let discord = &config.overseer.discord;
    assert_eq!(discord.channel_id, None);
    assert_eq!(discord.notify_channel_id, None);
    assert!(discord.allowed_user_ids.is_empty());
    assert!(discord.chat_category_ids.is_empty());
    let shown = String::from_utf8(output).unwrap();
    assert!(shown.contains("nothing to serve"));
    // The operator must be able to read how to keep and how to clear.
    assert!(shown.contains("leave blank to keep the shown value, enter '-' to clear"));
    assert!(shown.contains("Discord channel ID (leave blank to keep, enter '-' to clear)"));
}

#[test]
fn discord_no_warning_while_a_chat_category_remains() {
    let mut config = configured();
    let mut input = Cursor::new(b"\n\n-\n\n\n\n\n\n");
    let mut output = Vec::new();
    steps::discord(&mut input, &mut output, &mut config).unwrap();
    assert_eq!(config.overseer.discord.channel_id, None);
    assert_eq!(config.overseer.discord.chat_category_ids, ["30"]);
    assert!(
        !String::from_utf8(output)
            .unwrap()
            .contains("nothing to serve")
    );
}

#[test]
fn discord_notify_level_defaults_to_summary_and_can_be_changed() {
    use crate::overseer::config::NotifyLevel;

    let mut config = Config::default();
    let mut input = Cursor::new(b"y\n\n123\n\n10,20\n\nMY_DISCORD_TOKEN\n\n");
    let mut output = Vec::new();
    steps::discord(&mut input, &mut output, &mut config).unwrap();
    assert_eq!(config.overseer.discord.notify_level, NotifyLevel::Summary);
    // The operator choosing a level must be able to read what each tier
    // means — in particular that summary is milestones + problems.
    let shown = String::from_utf8(output).unwrap();
    assert!(shown.contains("summary: milestones + problems"), "{shown}");

    let mut config = Config::default();
    // 2 = "errors", the second listed choice.
    let mut input = Cursor::new(b"y\n2\n123\n\n10,20\n\nMY_DISCORD_TOKEN\n\n");
    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert_eq!(config.overseer.discord.notify_level, NotifyLevel::Errors);
}

#[test]
fn discord_blank_chat_category_answer_stays_disabled() {
    let mut config = Config::default();
    let mut input = Cursor::new(b"y\n\n123\n\n10,20\n\nMY_DISCORD_TOKEN\n\n");
    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();
    assert!(config.overseer.discord.chat_category_ids.is_empty());
}

/// A per-test temp file, kept out of the process-wide fake home
/// (`config::paths::test_home`) so concurrent tests never race on one path.
fn isolated_env_file(config: &mut Config) -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("env");
    config.overseer.session_env_file = Some(path.clone());
    (temp, path)
}

#[test]
fn discord_blank_token_answer_leaves_the_env_file_untouched() {
    let mut config = Config::default();
    let (_temp, env_file) = isolated_env_file(&mut config);
    std::fs::write(&env_file, "UNRELATED=kept\n").unwrap();
    let mut input = Cursor::new(b"y\n\n123\n\n10,20\n\nMY_DISCORD_TOKEN\n\n");

    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();

    assert_eq!(
        std::fs::read_to_string(&env_file).unwrap(),
        "UNRELATED=kept\n"
    );
}

#[test]
fn discord_typed_token_is_written_to_the_session_env_file() {
    let mut config = Config::default();
    let (_temp, env_file) = isolated_env_file(&mut config);
    let mut input = Cursor::new(b"y\n\n123\n\n10,20\n\nMY_DISCORD_TOKEN\nthe-bot-token\n");

    steps::discord(&mut input, &mut Vec::new(), &mut config).unwrap();

    assert_eq!(
        std::fs::read_to_string(&env_file).unwrap(),
        "MY_DISCORD_TOKEN=the-bot-token\n"
    );
    let saved = serde_json::to_string(&config).unwrap();
    assert!(!saved.contains("the-bot-token"));
}
