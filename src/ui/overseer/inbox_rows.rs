use ratatui::text::{Line, Span};

use crate::model::Selection;
use crate::ui::{App, inbox::InboxItem, theme::DEFAULT as THEME};

/// Rows the header contributes ahead of the item rows — the `inbox (N)` count.
/// The OVERSEER frame adds this to a category's first detail row to find the
/// screen row of item `n`, so the two cannot drift apart silently.
pub(in crate::ui) const ITEM_ROW_OFFSET: usize = 1;

/// The Inbox category's detail: the count header, then one row per aggregated
/// item. The item rows are real tree rows — the marker is the row head and the
/// caller owns the indent — so the same builder serves the OVERSEER frame and
/// the Info preview, and the frame's height arithmetic counts what it draws.
pub(in crate::ui) fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let selected = match app.selected_item() {
        Some(Selection::OverseerInbox(index)) => Some(index),
        _ => None,
    };
    let mut lines = vec![Line::from(Span::styled(
        format!("inbox ({})", app.overseer_inbox.len()),
        THEME.accent_bold_style(),
    ))];
    if app.overseer_inbox.is_empty() {
        lines.push(Line::from(Span::styled("none", THEME.muted_style())));
    }
    lines.extend(
        app.overseer_inbox
            .iter()
            .enumerate()
            .map(|(index, item)| item_line(item, selected == Some(index))),
    );
    lines
}

fn item_line(item: &InboxItem, selected: bool) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    // A display-only item names why it cannot be answered where its target
    // session would otherwise be: there is no live session to answer into.
    let target = item.target_session.as_deref().unwrap_or("display-only");
    Line::from(Span::styled(
        format!("{marker} [{}] {} => {target}", item.kind.code(), item.label),
        if selected {
            THEME.selection_style()
        } else {
            THEME.accent_style()
        },
    ))
}
