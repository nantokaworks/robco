use super::*;
use crate::{config::Config, registry::Registry};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.orphans.clear();
    app
}

fn escalation(target: &str) -> crate::ui::inbox::InboxItem {
    crate::ui::inbox::InboxItem {
        kind: crate::ui::inbox::InboxKind::Escalation,
        repo: None,
        agent_id: None,
        target_session: None,
        target_id: target.into(),
        label: target.into(),
        detail: "needs operator".into(),
        at: chrono::Utc::now(),
        pr_url: None,
        pr_facts: None,
        sentence: None,
    }
}

#[test]
fn frame_keeps_only_control_ai_and_discord_after_the_header() {
    let app = test_app();
    let lines = content_lines(&app)
        .lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("OVERSEER"));
    assert_eq!(lines[1], "⚠ STALE/OFFLINE");
    assert!(lines[2].contains("Control AI"));
    assert!(lines[3].contains("Discord"));
}

#[test]
fn warnings_and_global_alerts_stay_below_the_header() {
    let mut app = test_app();
    app.overseer_inbox.push(escalation("global"));
    let lines = content_lines(&app)
        .lines
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert!(lines[1].starts_with("⚠ "));
    assert!(lines[2].contains("global"));
    assert!(lines[3].contains("Control AI"));
}

#[test]
fn selected_alert_row_counts_header_warnings() {
    let mut app = test_app();
    app.overseer_inbox.push(escalation("global"));
    app.selected = 0;
    assert_eq!(app.selected_item(), Some(Selection::OverseerAlert(0)));
    assert_eq!(content_lines(&app).selected_row, 2);
}

#[test]
fn discord_is_the_only_expandable_category() {
    assert_eq!(OverseerCategory::ALL, [OverseerCategory::Discord]);
    assert!(OverseerCategory::Discord.has_children());
    let app = test_app();
    let discord = content_lines(&app)
        .lines
        .into_iter()
        .find(|line| line.to_string().contains("Discord"))
        .unwrap()
        .to_string();
    assert!(discord.contains('▸'));
}

#[test]
fn dead_status_glyph_stays_beside_the_header_label() {
    let app = test_app();
    let line = build_content(&app, Some(40)).lines[0].to_string();
    assert!(line.starts_with("OVERSEER  ✗"), "{line}");
}

#[test]
fn an_errored_channel_above_shifts_the_selected_discord_row_down() {
    use crate::overseer::discord_channels::{ChannelAgent, ChannelAgentStatus, DiscordChannels};
    let now = chrono::Utc::now();
    let agent = |active_at, last_error| ChannelAgent {
        first_seen_at: now,
        last_active_at: active_at,
        turn_count: 1,
        status: ChannelAgentStatus::Idle,
        last_error,
        history: Vec::new(),
        channel_name: None,
    };
    let mut channels = DiscordChannels::default();
    // Newest first in the rendered order; the newer channel's error line
    // pushes every later channel down one rendered row.
    channels
        .channels
        .insert("newer".into(), agent(now, Some("boom".into())));
    channels.channels.insert(
        "older".into(),
        agent(now - chrono::Duration::hours(1), None),
    );
    let mut app = test_app();
    app.overseer_snapshot.discord_channels = channels;
    app.set_overseer_category_expanded(OverseerCategory::Discord, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::DiscordChannel(1)))
        .expect("no second channel row");

    // header(0) ⚠(1) Control AI(2) Discord(3), details from 4: newer(4),
    // newer's error(5), older(6).
    assert_eq!(content_lines(&app).selected_row, 6);
}
