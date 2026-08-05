//! Answering and approving Overseer Inbox rows.
//!
//! Both actions send a response into the worker's tmux session and, on
//! success, immediately mark the row handled by reusing the dismiss
//! suppression (see [`crate::overseer::dismissals`]). The Inbox is derived,
//! not stored, so without the suppression the row would keep demanding
//! attention until a later snapshot stopped deriving it.

use crate::{Result, locale::t, ui::inbox::InboxItem};

use super::super::{App, Mode, text_input::TextInput};

/// Why an inbox item cannot be answered: the escalation is real, but the worker
/// it came from is gone, so there is no session to send an answer into.
pub(super) const DISPLAY_ONLY: &str = "display-only inbox item: no live session to answer";

pub(super) enum InboxResponse<'a> {
    Answer(&'a str),
    Approve,
}

pub(super) fn send_response(
    session: &str,
    response: InboxResponse<'_>,
    mut literal: impl FnMut(&str, &str) -> Result<()>,
    mut keys: impl FnMut(&str, &[&str]) -> Result<()>,
) -> Result<()> {
    match response {
        InboxResponse::Answer(text) => {
            literal(session, text)?;
            keys(session, &["Enter"])
        }
        InboxResponse::Approve => keys(session, &["y", "Enter"]),
    }
}

impl App {
    /// Open the answer prompt for the selected Inbox row, or say why the row
    /// cannot be answered. Never falls through to an attach: an inbox row has no
    /// session of its own to attach to.
    pub(in crate::ui) fn answer_inbox_selected(&mut self, index: usize) {
        let Some(item) = self.overseer_inbox.get(index) else {
            self.show_message(t(self.locale, "inbox item is no longer listed"));
            return;
        };
        if item.target_session.is_none() {
            self.show_message(t(self.locale, DISPLAY_ONLY));
            return;
        }
        self.mode = Mode::PromptInbox {
            item: item.clone(),
            input: TextInput::new(),
        };
    }

    pub(super) fn answer_inbox(&mut self, item: &InboxItem, answer: &str) {
        self.answer_inbox_with(
            item,
            answer,
            crate::tmux::send_literal_text,
            crate::tmux::send_keys,
        );
    }

    fn answer_inbox_with(
        &mut self,
        item: &InboxItem,
        answer: &str,
        literal: impl FnMut(&str, &str) -> Result<()>,
        keys: impl FnMut(&str, &[&str]) -> Result<()>,
    ) {
        let Some(session) = item.target_session.clone() else {
            self.show_message(t(self.locale, DISPLAY_ONLY));
            return;
        };
        let result = send_response(&session, InboxResponse::Answer(answer), literal, keys);
        self.acknowledge_inbox(item, result, "answer sent");
    }

    pub(super) fn approve_inbox(&mut self, index: usize) {
        self.approve_inbox_with(
            index,
            crate::tmux::send_literal_text,
            crate::tmux::send_keys,
        );
    }

    fn approve_inbox_with(
        &mut self,
        index: usize,
        literal: impl FnMut(&str, &str) -> Result<()>,
        keys: impl FnMut(&str, &[&str]) -> Result<()>,
    ) {
        let Some(item) = self.overseer_inbox.get(index).cloned() else {
            self.show_message(t(self.locale, "inbox item is no longer listed"));
            return;
        };
        let Some(session) = item.target_session.clone() else {
            self.show_message(t(self.locale, DISPLAY_ONLY));
            return;
        };
        let result = send_response(&session, InboxResponse::Approve, literal, keys);
        self.acknowledge_inbox(&item, result, "approval sent");
    }

    /// A successful approve or answer means the row is handled: suppress it the
    /// same way a dismiss does, so it leaves the list — and the actionable
    /// count — now instead of whenever the next snapshot stops deriving it.
    /// The suppression is bounded by the row's own timestamp, so a fresh
    /// escalation for the same target still appears. A failed send changes
    /// nothing: the worker never got the response, so the row must keep
    /// demanding attention.
    fn acknowledge_inbox(&mut self, item: &InboxItem, result: Result<()>, success: &'static str) {
        match result {
            Ok(()) => self.apply_dismissals(
                &[(item.kind.code(), item.target_id.clone(), item.at)],
                t(self.locale, success).to_string(),
            ),
            Err(error) => self.show_message(error.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "inbox_respond_tests.rs"]
mod tests;
