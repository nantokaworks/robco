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
//!
//! The confirmation itself is a timeout-bounded probe, not a certainty, and it
//! false-negatives: dropr task #455 caught a live case (nex PR #830, dropr
//! task #571) where a handback was typed, submitted, and acted on by the
//! worker — yet `confirm_delivered` exhausted `CONFIRM_TIMEOUT` without ever
//! seeing the marker, so the caller retried, and the retry alone reached
//! `undeliverable` and escalated the entry to an operator while the worker was
//! already fixing the exact failure it had been handed. Widening the windows
//! here does not fix that class of failure by itself — task #455's evidence
//! shows confirmed deliveries skew toward *larger* prompts than unconfirmed
//! ones, the opposite of what a slow-paste theory predicts. Task #457 tracks
//! making the caller's retry safe against this false negative (skip resending
//! into a session that is already busy) rather than tuning these constants
//! further.

use crate::{Result, tmux};

/// Delay between the literal send and the submitting `Enter`.
///
/// tmux wraps a literal `send-keys -l` this size in bracketed-paste markers
/// when the destination pane has requested bracketed paste — Claude Code's TUI
/// does. Sending `Enter` in the same instant lands inside that paste handling
/// instead of submitting, which is how a long judge verdict (the
/// merge-recovery template embeds it verbatim, often ~2,500 characters) ended
/// up sitting unsent in the input box. 120ms only covered a fraction of real
/// deliveries (`merge_recovery_dispatched` landed in about 5% of attempts
/// against a live worker); this window is widened well past that margin.
/// This settle window gives the paste handling time to finish before the
/// submit key follows; `confirm_delivered` is the safety net for whatever
/// this guess does not cover.
const SUBMIT_SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

/// Types `prompt` into `session` and submits it, the way triage drives a live
/// worker through `TriageAction::RobcoAnswer`.
pub(super) fn send(session: &str, prompt: &str) -> Result<()> {
    tmux::send_literal_text(session, &tmux::single_line(prompt))?;
    std::thread::sleep(SUBMIT_SETTLE);
    tmux::send_keys(session, &["Enter"])
}

/// How long to wait for a session to show it started a turn before treating a
/// send as unconfirmed. Widened alongside `SUBMIT_SETTLE`: 1500ms left too
/// little room after a slow paste settle for the working marker to appear at
/// all, which is consistent with confirmation failing on ~95% of real
/// deliveries.
const CONFIRM_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(5000);
const CONFIRM_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(150);

/// Outcome of [`confirm_delivered`]. A capture error and a session that
/// simply has not started a turn yet are different facts — one says the probe
/// itself is untrustworthy, the other says the send may genuinely not have
/// landed — so the caller (and the decision log) must be able to tell them
/// apart instead of both reading as an identical `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DeliveryConfirmation {
    /// The session showed it started a turn.
    Confirmed,
    /// The deadline elapsed; every capture that succeeded showed no working
    /// marker.
    NotConfirmed,
    /// The deadline elapsed and the most recent capture attempt itself
    /// failed — the probe never got a clean read on the pane, so nothing it
    /// saw can be read as evidence either way.
    CaptureFailed(String),
}

/// Whether `session` shows it started a turn after a send, polling until
/// `CONFIRM_TIMEOUT` elapses.
pub(super) fn confirm_delivered(session: &str) -> DeliveryConfirmation {
    let deadline = std::time::Instant::now() + CONFIRM_TIMEOUT;
    let mut last_capture_error: Option<String>;
    loop {
        match tmux::capture_plain(session) {
            Ok(capture) if looks_working(&capture) => return DeliveryConfirmation::Confirmed,
            Ok(_) => last_capture_error = None,
            Err(error) => last_capture_error = Some(error.to_string()),
        }
        if std::time::Instant::now() >= deadline {
            return confirmation_at_deadline(last_capture_error);
        }
        std::thread::sleep(CONFIRM_POLL_INTERVAL);
    }
}

/// Resolves what a bounded confirmation loop should report once its deadline
/// elapses without ever seeing the working marker, from the most recent
/// capture attempt's outcome. Split out of the poll loop above so this
/// decision — a capture failure is reported apart from a clean read that
/// simply never showed the marker — is testable without a real tmux session
/// or the wall-clock delay.
fn confirmation_at_deadline(last_capture_error: Option<String>) -> DeliveryConfirmation {
    match last_capture_error {
        Some(error) => DeliveryConfirmation::CaptureFailed(error),
        None => DeliveryConfirmation::NotConfirmed,
    }
}

/// Claude Code's persistent working marker, present whenever a turn is
/// in flight. The same signal `status::classify` uses to report `Running`.
fn looks_working(capture: &str) -> bool {
    capture.to_ascii_lowercase().contains("esc to interrupt")
}

/// A single, immediate read of whether `session` is mid-turn right now.
///
/// Unlike [`confirm_delivered`], this does not poll toward a deadline — it
/// exists for `merge_recovery::dispatch` to ask, right before retyping a
/// prompt that a previous attempt could not confirm, whether the session
/// might already be running that earlier attempt for real. A capture that
/// fails is reported as "not busy": it says nothing about whether the
/// session is working, and treating it as busy would let a session this
/// probe can never read stall a retry forever instead of falling through to
/// the existing undelivered/undeliverable bound.
pub(super) fn is_busy(session: &str) -> bool {
    tmux::capture_plain(session)
        .map(|capture| looks_working(&capture))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "merge_delivery_tests.rs"]
mod tests;
