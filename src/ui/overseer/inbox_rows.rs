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

/// A row is `[ESC] REVIEW robco #296 — the branch has conflicts with its
/// base.`: the kind code stays (it is the dismissal identity), and the
/// remedy's tag replaces the raw reason and the `=> {session} |
/// display-only` suffix — at the 24-column sidebar minimum the row was
/// clipped well ahead of that suffix, and `display-only` is now said
/// positively by the tag itself (a `Watch` row needs no live session any more
/// than a `Merge` one does). The repository sits between the tag and the
/// target id — with four repositories registered here, the tag and the
/// repository are what tell an operator whether a row is theirs, so the
/// target id is the reasonable thing to trim first when the row runs out of
/// width; a decision-sourced row with no matching ledger entry omits it
/// rather than rendering a blank gap.
///
/// The reason ([`row_reason`]) trails the target id, so it is the *next*
/// thing to give way, and only ever renders past the width the tag,
/// repository, and target id already claimed — this is what says *what
/// happened*, where everything ahead of it says *whose row this is* and
/// *what to do about it*. Three spans so a `WATCH` tag renders muted while
/// the rest of the row keeps its normal selection/accent style, and the
/// reason renders muted on its own so it reads as detail rather than as
/// urgent as the tag.
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
    let repo = item
        .repo
        .as_deref()
        .map(|repo| format!("{repo} "))
        .unwrap_or_default();
    let reason = row_reason(&item.detail)
        .map(|reason| format!(" — {reason}"))
        .unwrap_or_default();
    Line::from(vec![
        // Marker + one gap space = `super::ROW_LEFT_EDGE` columns, the same
        // left edge every other detail row in this frame keeps to.
        Span::styled(format!("{marker} [{}] ", item.kind.code()), base_style),
        Span::styled(
            format!("{} {repo}{}", remedy.tag(), item.target_id),
            tag_style,
        ),
        Span::styled(reason, THEME.muted_style()),
    ])
}

/// A short, English, untranslated fragment of `item.detail` for the row: the
/// known sentence when `discord::humanize` recognises the reason as one of
/// its table entries, otherwise the reason's own first line, trimmed. `None`
/// when the first line is empty.
///
/// Row content stays English regardless of locale (see the module's own
/// `t`-free preview title and the workspace localization policy: labels and
/// row content are English, prose in a pane or dialog follows the locale) —
/// unlike [`item_preview`], which localizes both the humanized sentence and
/// the "what this means" / "next step" guidance, this never calls `t`.
/// `item.sentence`, the board reviewer's one-sentence summary, is excluded
/// for the same reason: it is written in the operator's configured language,
/// so putting it on an English row would mix scripts inside one line.
fn row_reason(detail: &str) -> Option<&str> {
    let first_line = detail.lines().next().unwrap_or(detail).trim();
    if first_line.is_empty() {
        return None;
    }
    Some(humanize::static_sentence(first_line).unwrap_or(first_line))
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
    ];
    if let Some(repo) = &item.repo {
        lines.push(field("repo", repo.clone()));
    }
    lines.extend([
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
    ]);
    // Per-case facts (dropr:461) — absent when the row has no matching pull
    // request, or the daemon has not read one yet. The row renders the same
    // without them; this only ever adds to it.
    if let Some(url) = &item.pr_url {
        lines.push(field("pull request", pr_label(url)));
    }
    if let Some(facts) = &item.pr_facts {
        if !facts.title.is_empty() {
            lines.push(field("title", facts.title.clone()));
        }
        lines.push(field(
            "size",
            format!(
                "{} files, {} lines",
                facts.files_changed, facts.lines_changed
            ),
        ));
        if !facts.failed_checks.is_empty() {
            lines.push(field("failed check", facts.failed_checks.join(", ")));
        }
    }
    // The board reviewer's own one-sentence description of this case
    // (dropr:462) — model-written, already in the operator's configured
    // language, and never itself localized. Absent whenever `review_profile`
    // is unset, the session failed, or the case has since changed; the row
    // renders the same without it.
    if let Some(sentence) = &item.sentence {
        lines.push(field("summary", sentence.clone()));
    }
    lines.extend([
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
    ]);
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

/// `#123` when `url` has the usual `/pull/123` shape, or the raw url
/// otherwise — the same reading `discord::notifications::pr_link` gives the
/// same shape of url, kept as its own copy since that one also wraps the
/// number in a markdown link this plain-text pane has no use for.
fn pr_label(url: &str) -> String {
    url.rsplit_once("/pull/")
        .map(|(_, number)| format!("#{number}"))
        .unwrap_or_else(|| url.to_string())
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
