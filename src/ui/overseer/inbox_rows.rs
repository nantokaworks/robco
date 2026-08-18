use ratatui::text::{Line, Span, Text};

use crate::locale::{fmt, t};
use crate::model::Selection;
use crate::overseer::discord::humanize;
use crate::overseer::remedy::Move;
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
        return vec![Line::from(Span::styled(
            t(app.locale, "none"),
            THEME.muted_style(),
        ))];
    }
    app.overseer_inbox
        .iter()
        .enumerate()
        .map(|(index, item)| item_line(item, selected == Some(index)))
        .collect()
}

/// Why an item cannot be answered, said where its target session would go.
const DISPLAY_ONLY: &str = "display-only";

/// A row is `[ESC] REVIEW #296`: the kind code stays (it is the dismissal
/// identity), and the remedy's tag replaces the raw reason and the
/// `=> {session} | display-only` suffix — at the 24-column sidebar minimum
/// the row was clipped well ahead of that suffix, and `display-only` is now
/// said positively by the tag itself (a `Watch` row needs no live session any
/// more than a `Merge` one does). Two spans so a `WATCH` tag renders muted
/// while the rest of the row keeps its normal selection/accent style.
fn item_line(item: &InboxItem, selected: bool) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    let remedy = item.remedy();
    let base_style = if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    };
    let tag_style = if remedy.step == Move::Watch {
        THEME.muted_style()
    } else {
        base_style
    };
    Line::from(vec![
        Span::styled(format!("{marker} [{}] ", item.kind.code()), base_style),
        Span::styled(format!("{} {}", remedy.tag(), item.target_id), tag_style),
    ])
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
                t(app.locale, "item is no longer listed"),
                THEME.muted_style(),
            ))]
            .into(),
        );
    };

    let remedy = item.remedy();
    let mut lines = vec![
        field("kind", item.kind.label().to_string()),
        field("remedy", remedy.tag().to_string()),
        field("target", item.target_id.clone()),
        match &item.target_session {
            Some(session) => field("session", session.clone()),
            // Say why the two keys bound to this row will not act, rather than
            // leaving the operator to press them and find out.
            None => field(
                "session",
                fmt(
                    app.locale,
                    "{} — no live session to answer or approve",
                    &[DISPLAY_ONLY],
                ),
            ),
        },
        // With the year, unlike the Decisions detail's `%m-%d %H:%M`: a stale
        // escalation can sit here for months, and the row is exactly the one
        // whose age the operator needs in order to judge it.
        field("at", item.at.format("%Y-%m-%d %H:%M UTC").to_string()),
        Line::from(""),
        Line::from(Span::styled(
            t(app.locale, "what this means"),
            THEME.muted_style(),
        )),
        Line::from(Span::styled(
            t(app.locale, remedy.means),
            THEME.accent_style(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            t(app.locale, "next step"),
            THEME.muted_style(),
        )),
        Line::from(Span::styled(
            t(app.locale, remedy.next),
            THEME.accent_style(),
        )),
        Line::from(""),
        Line::from(Span::styled(t(app.locale, "reason"), THEME.muted_style())),
    ];
    // A known code-shaped reason gets one readable, localized sentence ahead
    // of the raw code — reusing the same vocabulary table
    // `discord::notifications` already builds Discord descriptions from, so
    // the two surfaces read the same reason the same way. The raw code
    // always stays, verbatim and untranslated, beneath it — accent-styled
    // when it is the only line (an unrecognised reason, or wrapped prose
    // that is already a sentence — see `humanize::sentence`), muted once
    // the sentence above it carries the primary reading.
    let known_sentence = humanize::static_sentence(&item.detail);
    if let Some(known) = known_sentence {
        lines.push(Line::from(Span::styled(
            t(app.locale, known),
            THEME.accent_style(),
        )));
    }
    let detail_style = if known_sentence.is_some() {
        THEME.muted_style()
    } else {
        THEME.accent_style()
    };
    lines.extend(
        item.detail
            .lines()
            .map(|line| Line::from(Span::styled(line.to_string(), detail_style))),
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
