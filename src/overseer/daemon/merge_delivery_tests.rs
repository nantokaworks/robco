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

/// A probe that never got a clean read on the pane proves nothing about
/// whether the worker got the prompt — it must not be reported the same way
/// as a pane that was read cleanly and simply showed no working marker.
#[test]
fn a_capture_failure_at_the_deadline_is_reported_as_its_own_state() {
    assert_eq!(
        confirmation_at_deadline(Some("no such session: robco-test".into())),
        DeliveryConfirmation::CaptureFailed("no such session: robco-test".into())
    );
}

#[test]
fn a_clean_read_with_no_marker_at_the_deadline_is_not_confirmed() {
    assert_eq!(
        confirmation_at_deadline(None),
        DeliveryConfirmation::NotConfirmed
    );
}

/// End-to-end against a real tmux pane (no live Claude Code needed): typing
/// the marker text into an idle shell's input line is enough for
/// `capture-pane` to read it back, which is what `confirm_delivered` actually
/// polls. Returns on the very first poll, so this stays fast despite the
/// widened `CONFIRM_TIMEOUT`.
#[test]
fn confirm_delivered_recognizes_a_real_sessions_marker_text() {
    let session = format!("robco-test-confirm-delivered-{}", std::process::id());
    if crate::tmux::new_session(&session, &std::env::temp_dir(), "sh", &[]).is_err() {
        // No usable tmux in this environment — the pure-logic tests above
        // already cover the branch this test would exercise.
        return;
    }
    let _ = crate::tmux::send_literal_text(&session, "esc to interrupt");
    let result = confirm_delivered(&session);
    let _ = crate::tmux::kill_session(&session);
    assert_eq!(result, DeliveryConfirmation::Confirmed);
}

#[test]
fn confirm_delivered_reports_a_capture_failure_for_a_session_that_does_not_exist() {
    let result = confirmation_at_deadline(
        crate::tmux::capture_plain("robco-test-confirm-delivered-missing-session")
            .err()
            .map(|error| error.to_string()),
    );
    assert!(matches!(result, DeliveryConfirmation::CaptureFailed(_)));
}

/// `is_busy` is the immediate read `merge_recovery::dispatch` uses to decide
/// whether retyping a prompt would land on top of a still-running turn. A
/// missing session must not read as busy — that would let an unreadable pane
/// stall a retry the undelivered/undeliverable bound is supposed to resolve.
#[test]
fn is_busy_is_false_for_a_session_that_does_not_exist() {
    assert!(!is_busy("robco-test-is-busy-missing-session"));
}

#[test]
fn is_busy_reflects_the_sessions_working_marker() {
    let session = format!("robco-test-is-busy-{}", std::process::id());
    if crate::tmux::new_session(&session, &std::env::temp_dir(), "sh", &[]).is_err() {
        // No usable tmux in this environment — the pure-logic tests above
        // already cover the branch this test would exercise.
        return;
    }
    assert!(!is_busy(&session));
    let _ = crate::tmux::send_literal_text(&session, "esc to interrupt");
    assert!(is_busy(&session));
    let _ = crate::tmux::kill_session(&session);
}
