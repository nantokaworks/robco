use ratatui::text::{Line, Span, Text};

use crate::model::Selection;
use crate::ui::{App, inbox::InboxItem, theme::DEFAULT as THEME};

/// The Inbox category's detail: one row per aggregated item, and nothing else.
/// The count the rows used to be nested under is already on the category row —
/// `category_summary` renders it as `N/M actionable` — so a header here would
/// read as an Inbox inside an Inbox and cost a row of a 24-column sidebar.
///
/// The item rows are real tree rows — the marker is the row head and the caller
/// owns the indent — so the same builder serves the OVERSEER frame and the
/// frame's height arithmetic counts what it draws. Item `n` is detail row `n`.
pub(in crate::ui) fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let selected = match app.selected_item() {
        Some(Selection::OverseerInbox(index)) => Some(index),
        _ => None,
    };
    if app.overseer_inbox.is_empty() {
        return vec![Line::from(Span::styled("none", THEME.muted_style()))];
    }
    app.overseer_inbox
        .iter()
        .enumerate()
        .map(|(index, item)| item_line(item, selected == Some(index)))
        .collect()
}

/// Why an item cannot be answered, said where its target session would go.
const DISPLAY_ONLY: &str = "display-only";

fn item_line(item: &InboxItem, selected: bool) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    // A display-only item names why it cannot be answered where its target
    // session would otherwise be: there is no live session to answer into.
    let target = item.target_session.as_deref().unwrap_or(DISPLAY_ONLY);
    Line::from(Span::styled(
        format!("{marker} [{}] {} => {target}", item.kind.code(), item.label),
        if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        },
    ))
}

/// The preview for a selected item row: what the row is, who it is about, and
/// its reason in full.
///
/// The row itself is already on screen in the left frame, so re-listing the
/// other items here would repeat what the operator can see while saying nothing
/// about the one under the cursor. The sidebar trims `label` to its width, which
/// makes this the only place the whole reason fits.
pub(in crate::ui) fn item_preview(app: &App, index: usize) -> (String, Text<'static>) {
    let Some(item) = app.overseer_inbox.get(index) else {
        return (
            "OVERSEER / Inbox".to_string(),
            vec![Line::from(Span::styled(
                "item is no longer listed",
                THEME.muted_style(),
            ))]
            .into(),
        );
    };

    let mut lines = vec![
        field("kind", item.kind.label().to_string()),
        field("target", item.target_id.clone()),
        match &item.target_session {
            Some(session) => field("session", session.clone()),
            // Say why the two keys bound to this row will not act, rather than
            // leaving the operator to press them and find out.
            None => field(
                "session",
                format!("{DISPLAY_ONLY} — no live session to answer or approve"),
            ),
        },
        // With the year, unlike the Decisions detail's `%m-%d %H:%M`: a stale
        // escalation can sit here for months, and the row is exactly the one
        // whose age the operator needs in order to judge it.
        field("at", item.at.format("%Y-%m-%d %H:%M UTC").to_string()),
        Line::from(""),
        Line::from(Span::styled("reason", THEME.muted_style())),
    ];
    lines.extend(
        item.detail
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), THEME.accent_style()))),
    );

    (
        format!("OVERSEER / Inbox / {}", item.target_id),
        lines.into(),
    )
}

fn field(name: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{name}: "), THEME.muted_style()),
        Span::styled(value, THEME.accent_style()),
    ])
}

#[cfg(test)]
#[path = "inbox_rows_tests.rs"]
mod tests;
