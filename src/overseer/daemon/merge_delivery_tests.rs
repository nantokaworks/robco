use super::*;

#[test]
fn looks_working_recognizes_claudes_active_turn_marker() {
    assert!(looks_working(
        "⏵⏵ bypass permissions on (shift+tab to cycle) · esc to interrupt · ← for agents"
    ));
    // Case-insensitive: the marker's exact casing is an implementation detail
    // of the TUI, not a contract this check should be brittle against.
    assert!(looks_working("ESC TO INTERRUPT"));
    assert!(!looks_working("❯ "));
    assert!(!looks_working("… [Pasted text #1]"));
    assert!(!looks_working(""));
}
