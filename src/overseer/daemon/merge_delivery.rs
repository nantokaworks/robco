//! Typing a prompt into a live tmux session and confirming it actually landed.
//!
//! `tmux send-keys` reports success once it has typed keys into a pane; it has
//! no idea whether the receiving program acted on them. That gap is what let a
//! merge-recovery handback sit unsent in a worker's input box while
//! `overseer::merge_recovery` recorded it as delivered: a long prompt sent as
//! one literal paste, immediately followed by `Enter`, can have the `Enter`
//! consumed by the receiving TUI's paste handling instead of submitting.
//!
//! This module owns both halves of the fix: a settle window between the paste
//! and the submit key, and a post-send confirmation that the session actually
//! started a turn. Callers decide what to do when confirmation fails —
//! `merge_recovery::dispatch` un-charges its budget and records its own
//! reason — this module only reports whether delivery could be confirmed.

use crate::{Result, tmux};

/// Delay between the literal send and the submitting `Enter`.
///
/// tmux wraps a literal `send-keys -l` this size in bracketed-paste markers
/// when the destination pane has requested bracketed paste — Claude Code's TUI
/// does. Sending `Enter` in the same instant lands inside that paste handling
/// instead of submitting, which is how a long judge verdict (the
/// merge-recovery template embeds it verbatim, often ~2,500 characters) ended
/// up sitting unsent in the input box. This settle window gives the paste
/// handling time to finish before the submit key follows; `confirm_delivered`
/// is the safety net for whatever this guess does not cover.
const SUBMIT_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// Types `prompt` into `session` and submits it, the way triage drives a live
/// worker through `TriageAction::RobcoAnswer`.
pub(super) fn send(session: &str, prompt: &str) -> Result<()> {
    tmux::send_literal_text(session, &tmux::single_line(prompt))?;
    std::thread::sleep(SUBMIT_SETTLE);
    tmux::send_keys(session, &["Enter"])
}

/// How long to wait for a session to show it started a turn before treating a
/// send as unconfirmed.
const CONFIRM_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);
const CONFIRM_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(150);

/// Whether `session` shows it started a turn after a send, polling until
/// `CONFIRM_TIMEOUT` elapses. A capture failure never confirms — a probe that
/// cannot see the pane must not be read as a successful delivery.
pub(super) fn confirm_delivered(session: &str) -> bool {
    let deadline = std::time::Instant::now() + CONFIRM_TIMEOUT;
    loop {
        if tmux::capture_plain(session).is_ok_and(|capture| looks_working(&capture)) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(CONFIRM_POLL_INTERVAL);
    }
}

/// Claude Code's persistent working marker, present whenever a turn is
/// in flight. The same signal `status::classify` uses to report `Running`.
fn looks_working(capture: &str) -> bool {
    capture.to_ascii_lowercase().contains("esc to interrupt")
}

#[cfg(test)]
#[path = "merge_delivery_tests.rs"]
mod tests;
