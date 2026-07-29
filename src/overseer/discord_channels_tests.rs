use super::*;

#[test]
fn a_first_turn_creates_a_running_record_and_a_success_appends_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("channels.json");
    let mut channels = DiscordChannels::load(&path).unwrap();

    channels.begin_turn(&path, "c1").unwrap();
    let record = channels.channels.get("c1").unwrap();
    assert_eq!(record.status, ChannelAgentStatus::Running);
    assert_eq!(record.turn_count, 0);
    let first_seen = record.first_seen_at;

    channels
        .end_turn(&path, "c1", "hello", Ok("hi there"))
        .unwrap();
    let record = channels.channels.get("c1").unwrap();
    assert_eq!(record.status, ChannelAgentStatus::Idle);
    assert_eq!(record.turn_count, 1);
    assert_eq!(record.first_seen_at, first_seen);
    assert_eq!(
        channels.history("c1"),
        [ChannelTurn {
            user_message: "hello".into(),
            reply: "hi there".into(),
        }]
    );

    let reloaded = DiscordChannels::load(&path).unwrap();
    assert_eq!(reloaded, channels);
}

#[test]
fn a_failed_turn_records_the_error_without_touching_history() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("channels.json");
    let mut channels = DiscordChannels::load(&path).unwrap();

    channels.begin_turn(&path, "c1").unwrap();
    channels.end_turn(&path, "c1", "hello", Ok("hi")).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    channels
        .end_turn(&path, "c1", "again", Err("session timed out"))
        .unwrap();

    let record = channels.channels.get("c1").unwrap();
    assert_eq!(record.status, ChannelAgentStatus::Failed);
    assert_eq!(record.last_error.as_deref(), Some("session timed out"));
    assert_eq!(record.turn_count, 2);
    assert_eq!(channels.history("c1").len(), 1);
}

#[test]
fn history_is_capped_at_the_limit_oldest_first() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("channels.json");
    let mut channels = DiscordChannels::load(&path).unwrap();

    for turn in 0..(HISTORY_LIMIT + 3) {
        channels.begin_turn(&path, "c1").unwrap();
        channels
            .end_turn(
                &path,
                "c1",
                &format!("msg-{turn}"),
                Ok(&format!("reply-{turn}")),
            )
            .unwrap();
    }

    let history = channels.history("c1");
    assert_eq!(history.len(), HISTORY_LIMIT);
    assert_eq!(history.first().unwrap().user_message, "msg-3");
    assert_eq!(history.last().unwrap().user_message, "msg-8");
}

#[test]
fn end_turn_without_a_matching_begin_is_a_no_op() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("channels.json");
    let mut channels = DiscordChannels::load(&path).unwrap();

    channels
        .end_turn(&path, "unknown", "hi", Ok("reply"))
        .unwrap();

    assert!(channels.channels.is_empty());
    assert!(!path.exists());
}

#[test]
fn reconcile_restart_clears_a_running_status_left_by_a_dead_daemon() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("channels.json");
    let mut channels = DiscordChannels::load(&path).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    assert_eq!(
        channels.channels.get("c1").unwrap().status,
        ChannelAgentStatus::Running
    );

    let mut reloaded = DiscordChannels::load(&path).unwrap();
    reloaded.reconcile_restart(&path).unwrap();
    assert_eq!(
        reloaded.channels.get("c1").unwrap().status,
        ChannelAgentStatus::Idle
    );

    let persisted = DiscordChannels::load(&path).unwrap();
    assert_eq!(
        persisted.channels.get("c1").unwrap().status,
        ChannelAgentStatus::Idle
    );
}

#[test]
fn a_missing_file_loads_as_empty() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("channels.json");
    assert_eq!(
        DiscordChannels::load(&path).unwrap(),
        DiscordChannels::default()
    );
}

#[test]
fn an_overlong_snippet_is_truncated() {
    let long = "x".repeat(SNIPPET_LIMIT + 50);
    let truncated = truncate(&long);
    assert!(truncated.chars().count() <= SNIPPET_LIMIT + 1);
    assert!(truncated.ends_with('…'));
}
