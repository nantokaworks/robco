//! Small pieces of dialog body every `Mode::Confirm*` / `Mode::Prompt*` arm in
//! `content.rs` reaches for — split out to keep that file under the size
//! limit.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
};

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

const MIN_INSTRUCTION_ROWS: usize = 5;
const MAX_INSTRUCTION_ROWS: usize = 10;

/// Body shared by `Mode::PromptOverseer` and `Mode::PromptSession`.
pub(super) fn instruction_prompt_body(
    locale: Locale,
    body: Rect,
    content_width: usize,
    input: &TextInput,
) -> (Vec<Line<'static>>, (usize, usize)) {
    let available_rows = body.height.saturating_sub(3) as usize;
    let max_rows = available_rows.clamp(1, MAX_INSTRUCTION_ROWS);
    let mut wrapped =
        input_wrap::input_lines(t(locale, "instruction"), input, content_width, max_rows);
    let rows = instruction_input_rows(wrapped.lines.len(), available_rows);
    wrapped.lines.resize_with(rows, Line::default);
    let caret = wrapped.caret;
    let mut lines = wrapped.lines;
    lines.push(hint_line(
        locale,
        "enter send   alt+enter/ctrl+j newline   esc cancel",
    ));
    (lines, caret)
}

fn instruction_input_rows(content_rows: usize, available_rows: usize) -> usize {
    content_rows
        .clamp(MIN_INSTRUCTION_ROWS, MAX_INSTRUCTION_ROWS)
        .min(available_rows.max(1))
}

#[cfg(test)]
mod tests {
    use super::instruction_input_rows;

    #[test]
    fn instruction_rows_start_tall_grow_and_clamp() {
        assert_eq!(instruction_input_rows(1, 20), 5);
        assert_eq!(instruction_input_rows(7, 20), 7);
        assert_eq!(instruction_input_rows(20, 20), 10);
        assert_eq!(instruction_input_rows(7, 3), 3);
    }
}
