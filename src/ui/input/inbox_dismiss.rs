//! Clearing Overseer Inbox rows.
//!
//! The Inbox is derived, not stored, so dismissing cannot delete anything: it
//! records a suppression that [`crate::ui::inbox::aggregate`] applies as a
//! filter. See [`crate::overseer::dismissals`] for the persisted shape and why
//! a suppression is bounded by the dismissed item's timestamp.

use chrono::{DateTime, Utc};

use crate::locale::{fmt, t};

use super::super::App;

/// One row to suppress: its `(kind, target_id)` identity and the timestamp it
/// was carrying when the operator cleared it.
pub(super) type Row = (&'static str, String, DateTime<Utc>);

impl App {
    /// Hide the selected row. The escalation and ledger records it was derived
    /// from are untouched — only the derived row is suppressed, and only up to
    /// the timestamp it carries, so a later escalation for the same target is
    /// listed again.
    pub(in crate::ui) fn dismiss_inbox_item(&mut self, index: usize) {
        let rows = self.inbox_dismissal_rows(index);
        let Some((kind, target_id, _)) = rows.first() else {
            self.show_message(t(self.locale, "inbox item is no longer listed"));
            return;
        };
        let message = fmt(self.locale, "dismissed [{}] {}", &[kind, target_id]);
        self.apply_dismissals(&rows, message);
    }

    /// Reads one row from the displayed list rather than re-deriving, so what is
    /// suppressed is exactly what the operator was looking at — including the
    /// timestamps, which is what keeps a newer escalation out of the window.
    fn inbox_dismissal_rows(&self, index: usize) -> Vec<Row> {
        let row = |item: &crate::ui::inbox::InboxItem| {
            (item.kind.code(), item.target_id.clone(), item.at)
        };
        self.overseer_inbox
            .get(index)
            .map(row)
            .into_iter()
            .collect()
    }

    /// Shared by dismiss and by the approval acknowledgement: suppressing
    /// is the one mechanism that both hides the row now and keeps a newer
    /// escalation for the same target visible later.
    pub(super) fn apply_dismissals(&mut self, rows: &[Row], success: String) {
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
