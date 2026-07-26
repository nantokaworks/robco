//! Clearing Overseer Inbox rows.
//!
//! The Inbox is derived, not stored, so dismissing cannot delete anything: it
//! records a suppression that [`crate::ui::inbox::aggregate`] applies as a
//! filter. See [`crate::overseer::dismissals`] for the persisted shape and why
//! a suppression is bounded by the dismissed item's timestamp.

use chrono::{DateTime, Utc};

use super::super::{App, Mode};

/// One row to suppress: its `(kind, target_id)` identity and the timestamp it
/// was carrying when the operator cleared it.
type Row = (&'static str, String, DateTime<Utc>);

impl App {
    /// Hide the selected row. The escalation and ledger records it was derived
    /// from are untouched — only the derived row is suppressed, and only up to
    /// the timestamp it carries, so a later escalation for the same target is
    /// listed again.
    pub(in crate::ui) fn dismiss_inbox_item(&mut self, index: usize) {
        let rows = self.inbox_dismissal_rows(Some(index));
        let Some((kind, target_id, _)) = rows.first() else {
            self.show_message("inbox item is no longer listed");
            return;
        };
        let message = format!("dismissed [{kind}] {target_id}");
        self.apply_dismissals(&rows, message);
    }

    /// Open the confirmation for clearing every listed row. Dismissing one row
    /// acts immediately; clearing the list is the bulk action, so it is gated
    /// the same way the other bulk overseer actions are.
    pub(in crate::ui) fn confirm_dismiss_inbox(&mut self) {
        if self.overseer_inbox.is_empty() {
            self.show_message("inbox is already empty");
            return;
        }
        self.mode = Mode::ConfirmInboxDismissAll {
            count: self.overseer_inbox.len(),
        };
    }

    pub(in crate::ui) fn dismiss_inbox_all(&mut self) {
        let rows = self.inbox_dismissal_rows(None);
        let message = format!("dismissed {} inbox item(s)", rows.len());
        self.apply_dismissals(&rows, message);
    }

    /// The rows a dismiss covers: one item by index, or every listed item.
    ///
    /// Reads from the displayed list rather than re-deriving, so what is
    /// suppressed is exactly what the operator was looking at — including the
    /// timestamps, which is what keeps a newer escalation out of the window.
    fn inbox_dismissal_rows(&self, index: Option<usize>) -> Vec<Row> {
        let row = |item: &crate::ui::inbox::InboxItem| {
            (item.kind.code(), item.target_id.clone(), item.at)
        };
        match index {
            Some(index) => self
                .overseer_inbox
                .get(index)
                .map(row)
                .into_iter()
                .collect(),
            None => self.overseer_inbox.iter().map(row).collect(),
        }
    }

    fn apply_dismissals(&mut self, rows: &[Row], success: String) {
        let targets = rows
            .iter()
            .map(|(kind, target_id, at)| (*kind, target_id.as_str(), *at))
            .collect::<Vec<_>>();
        match crate::overseer::dismissals::dismiss(&targets, &self.overseer_inbox_targets) {
            Ok(()) => {
                // Re-derive immediately so the rows leave on this frame rather
                // than on the next background refresh.
                self.refresh_overseer_snapshot();
                self.show_message(success);
            }
            Err(error) => self.show_message(error.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "inbox_dismiss_tests.rs"]
mod tests;
