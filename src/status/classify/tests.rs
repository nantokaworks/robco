use super::*;
use chrono::TimeZone;

fn fixed_now() -> chrono::DateTime<Local> {
    Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
}

#[test]
fn detects_common_confirmation_prompts() {
    assert!(looks_waiting("Allow edit src/main.rs? (y/n)"));
    assert!(looks_waiting("Do you want to continue?"));
    assert!(looks_waiting("  │ ❯ 1. Yes │\n  │   2. No  │"));
    assert!(looks_waiting("Enter API token?"));
    assert!(!looks_waiting("No tests failed"));
    assert!(!looks_waiting("Yes, the change is complete"));
    assert!(!looks_waiting("running cargo test"));
}

#[test]
fn claude_squad_permission_marker_waits() {
    let capture = "╭────╮\n│ Edit file? │\n│ ❯ 1. Yes │\n│   2. Yes, and don't ask again │\n│   3. No, and tell Claude what to do differently │\n╰────╯\n  ? for shortcuts";
    let mut state = WatchStatusState::default();
    let report = classify_capture(capture, &mut state, fixed_now());
    assert_eq!(report.status, Status::Waiting);
    assert!(report.awaiting_confirmation);
}

#[test]
fn stopped_with_live_chrome_goes_done() {
    // A settled `❯` prompt with only live chrome changing (cursor blink,
    // token counter) is a finished turn: `Done`, not `wait`, not `run`.
    let mut state = WatchStatusState::default();
    let first = "Done\n  Tokens: 1 █\n❯ \n  ? for shortcuts";
    let second = "Done\n  Tokens: 2 ▌\n❯ \n  ? for shortcuts";
    assert_eq!(
        classify_capture(first, &mut state, fixed_now()).status,
        Status::Done
    );
    assert_eq!(
        classify_capture(second, &mut state, fixed_now() + Duration::seconds(1)).status,
        Status::Done
    );
}

#[test]
fn boxed_permission_prompt_waits_despite_footer() {
    let mut state = WatchStatusState::default();
    let capture =
        "╭────╮\n│ Do you want to proceed? │\n│ ❯ 1. Yes │\n│   2. No │\n╰────╯\n  ? for shortcuts";
    let report = classify_capture(capture, &mut state, fixed_now());
    assert_eq!(report.status, Status::Waiting);
    assert!(report.awaiting_confirmation);
}

#[test]
fn working_marker_forces_running() {
    let mut state = WatchStatusState::default();
    assert_eq!(
        classify_capture(
            "Generating response (esc to interrupt)",
            &mut state,
            fixed_now()
        )
        .status,
        Status::Running
    );
}

#[test]
fn finished_at_input_prompt_is_done_not_waiting() {
    // Real Claude "finished, awaiting input" shape: a `❯` prompt above the
    // persistent mode bar, no `esc to interrupt`. This is `Done`, and it is
    // NOT a confirmation prompt, so auto-accept must never fire.
    let capture = "⏺ 実装完了だす。\n\n────── branch-name ──\n❯ \n──────\n  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents";
    let mut state = WatchStatusState::default();
    let report = classify_capture(capture, &mut state, fixed_now());
    assert_eq!(report.status, Status::Done);
    assert!(!report.awaiting_confirmation);
}

#[test]
fn done_with_trailing_question_stays_done() {
    // A finished turn whose last prose line ends in `?` (Claude asking the
    // user something in chat) must NOT be misread as a `wait` prompt: the
    // `❯` caret demotes weak-waiting.
    let capture = "⏺ このまま進めるダスか？\n❯ \n  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents";
    let mut state = WatchStatusState::default();
    let report = classify_capture(capture, &mut state, fixed_now());
    assert_eq!(report.status, Status::Done);
    assert!(!report.awaiting_confirmation);
}

#[test]
fn non_claude_question_without_caret_weak_waits() {
    // A program with no `❯` caret still uses the weak-waiting fallback.
    let capture = "Enter your API token?";
    let mut state = WatchStatusState::default();
    let report = classify_capture(capture, &mut state, fixed_now());
    assert_eq!(report.status, Status::Waiting);
    assert!(!report.awaiting_confirmation);
}

#[test]
fn working_beats_input_prompt_even_with_prompt_visible() {
    // While working the `❯` box is still on screen; `esc to interrupt` in the
    // mode bar must keep it Running, not Done.
    let capture = "✻ Working…\n❯ \n  ⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt · ← for agents";
    let mut state = WatchStatusState::default();
    assert_eq!(
        classify_capture(capture, &mut state, fixed_now()).status,
        Status::Running
    );
}

#[test]
fn mode_bar_is_treated_as_footer_chrome() {
    assert!(is_footer_line(
        "⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents"
    ));
    assert!(is_footer_line("⏵⏵ accept edits on (shift+tab to cycle)"));
    assert!(!is_footer_line("❯ cargo run で確認して"));
}

#[test]
fn status_signature_ignores_trailing_whitespace() {
    assert_eq!(
        status_signature("hello   \nworld\t\n\n\n"),
        status_signature("hello\nworld")
    );
}
