//! Approving Overseer escalations.
//!
//! Approval sends a response into the worker's tmux session and, on success,
//! immediately marks the row handled by reusing the dismiss
//! suppression (see [`crate::overseer::dismissals`]). The Inbox is derived,
//! not stored, so without the suppression the row would keep demanding
//! attention until a later snapshot stopped deriving it.

use crate::{Result, locale::t, ui::inbox::InboxItem};

use super::super::App;

/// Why an inbox item cannot be answered: the escalation is real, but the worker
/// it came from is gone, so there is no session to send an answer into.
pub(super) const DISPLAY_ONLY: &str = "display-only inbox item: no live session to answer";

pub(super) enum InboxResponse {
    Approve,
}

pub(super) fn send_response(
    session: &str,
    response: InboxResponse,
    mut keys: impl FnMut(&str, &[&str]) -> Result<()>,
) -> Result<()> {
    match response {
        InboxResponse::Approve => keys(session, &["y", "Enter"]),
    }
}

impl App {
    pub(super) fn approve_inbox(&mut self, index: usize) {
        let server = self.config.tmux_server.clone();
        self.approve_inbox_with(index, |session, keys| {
            crate::tmux::send_keys(&server, session, keys)
        });
    }

    pub(super) fn approve_inbox_with(
        &mut self,
        index: usize,
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
        let result = send_response(&session, InboxResponse::Approve, keys);
        self.acknowledge_inbox(&item, result, "approval sent");
    }

    /// A successful approve means the row is handled: suppress it the
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
