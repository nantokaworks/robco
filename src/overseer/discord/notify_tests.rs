use super::*;

fn config(channel_id: Option<&str>, notify_channel_id: Option<&str>) -> DiscordConfig {
    DiscordConfig {
        channel_id: channel_id.map(str::to_owned),
        notify_channel_id: notify_channel_id.map(str::to_owned),
        ..DiscordConfig::default()
    }
}

#[test]
fn reports_go_to_the_notify_channel_when_it_is_set() {
    let config = config(Some("100"), Some("200"));
    assert_eq!(report_channel_id(&config), Some(Id::new(200)));
    // Chat routing must not move with it.
    assert_eq!(channel_id(&config), Some(Id::new(100)));
}

#[test]
fn reports_fall_back_to_the_chat_channel_when_the_notify_channel_is_unset() {
    // The pre-field behavior: one channel serves chat and reports alike.
    let config = config(Some("100"), None);
    assert_eq!(report_channel_id(&config), Some(Id::new(100)));
}

#[test]
fn an_unparseable_notify_channel_degrades_to_the_old_routing() {
    // A typo hand-edited into config.json must not silence reports; the
    // fallback keeps them flowing where they always went.
    for bad in ["not-a-number", "0", ""] {
        let config = config(Some("100"), Some(bad));
        assert_eq!(
            report_channel_id(&config),
            Some(Id::new(100)),
            "for {bad:?}"
        );
    }
}

#[test]
fn no_channel_at_all_delivers_nowhere() {
    assert_eq!(report_channel_id(&config(None, None)), None);
}
