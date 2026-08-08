use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::{
    config::Config, model::OverseerCategory, overseer::discord_channels::ChannelAgentStatus,
    registry::Registry,
};

fn channels_path() -> std::path::PathBuf {
    overseer::discord_ops_dir().unwrap().join("channels.json")
}

/// The state file lives under the process-wide fake test home
/// (`config::paths::test_home`), so concurrent tests writing it race on the
/// same path. Serialize every test in this file and start each one from a
/// clean slate.
fn lock_overseer_home() -> std::sync::MutexGuard<'static, ()> {
    static STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::remove_file(channels_path());
    guard
}

fn app_with_discord_row_selected() -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(Registry::default(), Config::default(), temp.path().into());
    app.overseer_visible = true;
    app.overseer_snapshot.discord_channels = DiscordChannels::load(&channels_path()).unwrap();
    app.set_overseer_category_expanded(OverseerCategory::Discord, true);
    app.selected = app
        .visible()
        .iter()
        .position(|row| matches!(row, Selection::DiscordChannel(0)))
        .expect("no discord channel row");
    app
}

fn press(app: &mut App, code: KeyCode) {
    assert!(
        !app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
            .unwrap(),
        "key {code:?} quit the app"
    );
}

#[test]
fn retry_resets_a_failed_channel_to_idle_and_clears_the_error() {
    let _guard = lock_overseer_home();
    let path = channels_path();
    let mut channels = DiscordChannels::load(&path).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    channels
        .end_turn(&path, "c1", "hi", Err("session timed out"))
        .unwrap();

    let mut app = app_with_discord_row_selected();
    press(&mut app, KeyCode::Char('r'));

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("reset c1 to idle")
    );
    let reloaded = DiscordChannels::load(&path).unwrap();
    let record = reloaded.channels.get("c1").unwrap();
    assert_eq!(record.status, ChannelAgentStatus::Idle);
    assert_eq!(record.last_error, None);
}

#[test]
fn retry_on_a_channel_that_is_not_failed_reports_instead_of_touching_it() {
    let _guard = lock_overseer_home();
    let path = channels_path();
    let mut channels = DiscordChannels::load(&path).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    channels.end_turn(&path, "c1", "hi", Ok("hello")).unwrap();

    let mut app = app_with_discord_row_selected();
    press(&mut app, KeyCode::Char('r'));

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("c1 is not in a failed state")
    );
}

#[test]
fn remove_key_opens_a_confirmation_before_deleting_anything() {
    let _guard = lock_overseer_home();
    let path = channels_path();
    let mut channels = DiscordChannels::load(&path).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    channels.end_turn(&path, "c1", "hi", Ok("hello")).unwrap();

    let mut app = app_with_discord_row_selected();
    press(&mut app, KeyCode::Char('x'));

    assert!(matches!(app.mode, Mode::ConfirmRemoveDiscordChannel { .. }));
    // Nothing removed yet — the confirm dialog is still open.
    let reloaded = DiscordChannels::load(&path).unwrap();
    assert!(reloaded.channels.contains_key("c1"));
}

#[test]
fn confirming_remove_deletes_the_retained_record() {
    let _guard = lock_overseer_home();
    let path = channels_path();
    let mut channels = DiscordChannels::load(&path).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    channels.end_turn(&path, "c1", "hi", Ok("hello")).unwrap();

    let mut app = app_with_discord_row_selected();
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('y'));

    assert_eq!(
        app.message.as_ref().map(|(message, _)| message.as_str()),
        Some("removed channel c1")
    );
    let reloaded = DiscordChannels::load(&path).unwrap();
    assert!(!reloaded.channels.contains_key("c1"));
    assert!(!matches!(
        app.selected_item(),
        Some(Selection::DiscordChannel(_))
    ));
}

#[test]
fn cancelling_remove_keeps_the_record() {
    let _guard = lock_overseer_home();
    let path = channels_path();
    let mut channels = DiscordChannels::load(&path).unwrap();
    channels.begin_turn(&path, "c1").unwrap();
    channels.end_turn(&path, "c1", "hi", Ok("hello")).unwrap();

    let mut app = app_with_discord_row_selected();
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('n'));

    assert!(matches!(app.mode, Mode::Normal));
    let reloaded = DiscordChannels::load(&path).unwrap();
    assert!(reloaded.channels.contains_key("c1"));
}
