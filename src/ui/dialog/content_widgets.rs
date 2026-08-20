//! Small pieces of dialog body every `Mode::Confirm*` / `Mode::Prompt*` arm in
//! `content.rs` reaches for — split out to keep that file under the size
//! limit.

use ratatui::text::{Line, Span};

use crate::locale::{Locale, t};
use crate::ui::{input_wrap, text_input::TextInput, theme::DEFAULT as THEME};

/// What a merge-now land and a merged-PR cleanup both do afterward — the
/// same `git::post_merge::Cleanup`, so both dialogs describe it identically.
pub(super) const CLEANUP_FOLLOWS: &str = "pulls main, removes the worktree, deletes the branch here and \
     on GitHub, and ends the running session — anything left uncommitted in \
     the worktree is discarded";

pub(super) fn confirm_lines(
    locale: Locale,
    subject: String,
    hint: &'static str,
) -> Vec<Line<'static>> {
    vec![Line::from(subject), hint_line(locale, hint)]
}

/// One-line labelled input, paired with the display column its caret sits at.
pub(super) fn input_line(label: &str, input: &TextInput) -> (Line<'static>, usize) {
    let prefix = format!(" {label}: ");
    let column =
        input_wrap::display_width(&prefix) + input_wrap::text_width(input.text(), input.cursor());
    let mut spans = vec![Span::styled(prefix, THEME.dialog_label_style())];
    spans.extend(input_wrap::input_spans(input.text(), Some(input.cursor())));
    (Line::from(spans), column)
}

pub(super) fn hint_line(locale: Locale, text: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        t(locale, text).to_string(),
        THEME.hint_style(),
    ))
}
