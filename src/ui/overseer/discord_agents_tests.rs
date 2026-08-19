use super::*;
use crate::config::Config;
use crate::registry::Registry;
use chrono::Utc;

fn agent(status: ChannelAgentStatus, turns: u64, minutes_ago: i64) -> ChannelAgent {
    let now = Utc::now();
    ChannelAgent {
        first_seen_at: now,
        last_active_at: now - chrono::Duration::minutes(minutes_ago),
        turn_count: turns,
        status,
        last_error: None,
        history: Vec::new(),
        channel_name: None,
    }
}

fn app_with_channels(channels: DiscordChannels) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_snapshot.discord_channels = channels;
    app
}

#[test]
fn no_channels_renders_an_explicit_empty_state() {
    let app = app_with_channels(DiscordChannels::default());
    let lines = detail_lines(&app);
    assert_eq!(lines.len(), 1);
    assert!(text_of(&lines[0]).contains("no retained channel agents"));
}

#[test]
fn channels_are_ordered_newest_activity_first() {
    let mut channels = DiscordChannels::default();
    channels
        .channels
        .insert("older".into(), agent(ChannelAgentStatus::Idle, 3, 30));
    channels
        .channels
        .insert("newer".into(), agent(ChannelAgentStatus::Idle, 1, 1));

    assert_eq!(
        ordered_channel_ids(&channels),
        vec!["newer".to_string(), "older".to_string()]
    );
}

#[test]
fn the_selected_channel_row_carries_the_marker() {
    let mut channels = DiscordChannels::default();
    channels
        .channels
        .insert("c1".into(), agent(ChannelAgentStatus::Running, 1, 0));
    let mut app = app_with_channels(channels);
    app.selected = 0;
    app.set_overseer_category_expanded(crate::model::OverseerCategory::Discord, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::DiscordChannel(0)))
        .expect("no discord channel row");

    let lines = detail_lines(&app);
    assert!(text_of(&lines[0]).starts_with('>'));
}

#[test]
fn a_named_channel_renders_its_name_and_an_unnamed_one_falls_back_to_the_id() {
    let mut channels = DiscordChannels::default();
    let mut named = agent(ChannelAgentStatus::Idle, 1, 1);
    named.channel_name = Some("general".into());
    channels.channels.insert("111222333".into(), named);
    channels
        .channels
        .insert("999888777".into(), agent(ChannelAgentStatus::Idle, 1, 30));

    let app = app_with_channels(channels);
    let lines = detail_lines(&app);
    assert!(text_of(&lines[0]).contains("#general"));
    assert!(!text_of(&lines[0]).contains("111222333"));
    assert!(text_of(&lines[1]).contains("999888777"));
}

#[test]
fn a_failed_channel_renders_its_error_on_the_next_line() {
    let mut channels = DiscordChannels::default();
    let mut failing = agent(ChannelAgentStatus::Failed, 2, 5);
    failing.last_error = Some("session timed out".into());
    channels.channels.insert("c1".into(), failing);

    let app = app_with_channels(channels);
    let lines = detail_lines(&app);
    assert_eq!(lines.len(), 2);
    assert!(text_of(&lines[0]).contains(Status::Dead.glyph()));
    assert!(text_of(&lines[1]).contains("session timed out"));
}

#[test]
fn a_running_channel_shows_the_same_animated_glyph_an_agent_row_shows() {
    // The animated spinner frames `crate::ui::spinner::frame` cycles through
    // for `Indicator::Running` — the same set an agent row's running glyph
    // draws from. Listed here rather than read via elapsed-time equality so
    // the test is not racing the clock against `detail_lines`' own call to
    // `app.started.elapsed()`.
    const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    let mut channels = DiscordChannels::default();
    channels
        .channels
        .insert("c1".into(), agent(ChannelAgentStatus::Running, 4, 0));

    let app = app_with_channels(channels);
    let lines = detail_lines(&app);
    let glyph = lines[0].spans[1].content.trim().to_string();
    assert!(
        SPINNER_FRAMES.contains(&glyph.as_str()),
        "expected an animated spinner frame, got {glyph:?}"
    );
    // The animated frame, never the static glyph a non-animated fallback
    // would show for the same status.
    assert_ne!(glyph, Status::Running.glyph());
}

#[test]
fn turn_count_and_last_activity_age_are_still_visible() {
    let mut channels = DiscordChannels::default();
    channels
        .channels
        .insert("c1".into(), agent(ChannelAgentStatus::Idle, 7, 5));

    let app = app_with_channels(channels);
    let lines = detail_lines(&app);
    let text = text_of(&lines[0]);
    assert!(text.contains("7t"));
    assert!(text.contains("5m ago"));
}

#[test]
fn a_discord_row_and_an_agent_row_in_the_same_state_produce_the_same_primary_span() {
    // What an unadorned PROJECTS agent row computes for a plain-status row
    // (`tree.rs`'s `Selection::Agent` arm, before its own optional flags —
    // merging, worktree_missing, and the like — are layered on top): build
    // an `IndicatorState` from the status alone, then hand it to
    // `select`/`primary_span`. A channel row must resolve to the identical
    // glyph and style for the identical status.
    let elapsed = std::time::Duration::ZERO;
    let expected = indicator::primary_span(
        select(IndicatorState::with_status(Some(Status::Idle))),
        false,
        elapsed,
        INDICATOR_WIDTH,
    );

    let mut channels = DiscordChannels::default();
    channels
        .channels
        .insert("c1".into(), agent(ChannelAgentStatus::Idle, 1, 1));
    let app = app_with_channels(channels);
    let lines = detail_lines(&app);
    let actual = &lines[0].spans[1];

    assert_eq!(actual.content.trim(), expected.content.trim());
    assert_eq!(actual.style, expected.style);
}

fn text_of(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn joined(text: &Text<'static>) -> String {
    text.lines
        .iter()
        .map(text_of)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn selecting_a_channel_with_turns_shows_its_conversation() {
    use crate::overseer::discord_channels::ChannelTurn;

    let mut channels = DiscordChannels::default();
    let mut agent = agent(ChannelAgentStatus::Idle, 1, 1);
    agent.history.push(ChannelTurn {
        user_message: "status".into(),
        reply: "all quiet".into(),
    });
    channels.channels.insert("c1".into(), agent);
    let app = app_with_channels(channels);

    let (title, text) = channel_preview(&app, 0);
    assert!(title.contains("c1"));
    let rendered = joined(&text);
    assert!(rendered.contains("status"));
    assert!(rendered.contains("all quiet"));
}

#[test]
fn a_channel_with_no_turns_shows_an_empty_state() {
    let mut channels = DiscordChannels::default();
    channels
        .channels
        .insert("c1".into(), agent(ChannelAgentStatus::Idle, 0, 1));
    let app = app_with_channels(channels);

    let (_, text) = channel_preview(&app, 0);
    assert!(joined(&text).contains("no turns yet"));
}

#[test]
fn an_out_of_range_index_does_not_panic() {
    let app = app_with_channels(DiscordChannels::default());
    let (_, text) = channel_preview(&app, 0);
    assert!(joined(&text).contains("no longer listed"));
}

#[test]
fn a_very_long_reply_stays_one_line_per_source_line() {
    use crate::overseer::discord_channels::ChannelTurn;

    let mut channels = DiscordChannels::default();
    let mut agent = agent(ChannelAgentStatus::Idle, 1, 1);
    let long_reply = "x".repeat(2000);
    agent.history.push(ChannelTurn {
        user_message: "status".into(),
        reply: long_reply.clone(),
    });
    channels.channels.insert("c1".into(), agent);
    let app = app_with_channels(channels);

    let (_, text) = channel_preview(&app, 0);
    // Wrapping is the Paragraph widget's job at render time (dropr:451
    // reuses it rather than pre-wrapping here); this only checks the
    // long reply survives intact as a single logical line.
    assert!(joined(&text).contains(&long_reply));
}
