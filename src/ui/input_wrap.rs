use ratatui::text::{Line, Span};

use super::{text_input::TextInput, theme::DEFAULT as THEME};

/// Wrapped prompt input plus where its caret landed once wrapping and the
/// visible-window choice are known.
pub(in crate::ui) struct WrappedInput {
    pub(in crate::ui) lines: Vec<Line<'static>>,
    /// Row within `lines`, and display column within that row, of the caret.
    pub(in crate::ui) caret: (usize, usize),
}

pub(in crate::ui) fn input_lines(
    label: &str,
    input: &TextInput,
    max_width: usize,
    max_lines: usize,
) -> WrappedInput {
    if max_width == 0 {
        return WrappedInput {
            lines: vec![Line::default()],
            caret: (0, 0),
        };
    }

    let text = input.text();
    let full_prefix = format!(" {label}: ");
    let widest_char = text
        .chars()
        .map(|ch| width(&ch.to_string()))
        .max()
        .unwrap_or(1);
    let prefix = if width(&full_prefix) + widest_char <= max_width {
        full_prefix
    } else {
        String::new()
    };
    let prefix_width = width(&prefix);
    let input_width = max_width - prefix_width;
    let mut wrapped = wrap_text(text, input_width);

    let cursor = input.cursor();
    // A caret past the last character needs a cell of its own, and a last line
    // that already fills the width has none left — so it gets a row. A caret
    // inside the text sits on an existing character and needs no such row.
    if cursor == input.len()
        && wrapped
            .last()
            .is_some_and(|(line, _)| width(line) >= input_width)
    {
        wrapped.push((String::new(), cursor));
    }

    let caret_row = wrapped
        .iter()
        .rposition(|(_, start)| *start <= cursor)
        .unwrap_or(0);
    let caret_offset = cursor.saturating_sub(wrapped[caret_row].1);

    let wrapped_len = wrapped.len();
    // The tail is the interesting end while typing, but an edit further back
    // pulls the window up so the caret cannot be scrolled out of sight.
    let visible_from = wrapped_len.saturating_sub(max_lines.max(1)).min(caret_row);
    let caret_column = prefix_width + text_width(&wrapped[caret_row].0, caret_offset);

    let lines = wrapped
        .into_iter()
        .skip(visible_from)
        .take(max_lines.max(1))
        .enumerate()
        .map(|(index, (text, _))| {
            let label = if index == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix_width)
            };
            let mut spans = vec![Span::styled(label, THEME.dialog_label_style())];
            let caret = (index + visible_from == caret_row).then_some(caret_offset);
            spans.extend(input_spans(&text, caret));
            Line::from(spans)
        })
        .collect();

    WrappedInput {
        lines,
        caret: (caret_row - visible_from, caret_column),
    }
}

/// Render `text`, marking the caret when it falls inside this segment.
///
/// The caret reverses the cell it sits on rather than inserting a glyph, so the
/// rest of the line keeps its columns. Past the last character there is nothing
/// to reverse, so a literal `_` stands in for the cell.
pub(in crate::ui) fn input_spans(text: &str, caret: Option<usize>) -> Vec<Span<'static>> {
    let Some(caret) = caret else {
        return vec![Span::styled(text.to_string(), THEME.input_style())];
    };
    let mut rest = text.chars();
    let before: String = rest.by_ref().take(caret).collect();
    let before = Span::styled(before, THEME.input_style());
    match rest.next() {
        Some(at) => vec![
            before,
            Span::styled(at.to_string(), THEME.caret_style()),
            Span::styled(rest.collect::<String>(), THEME.input_style()),
        ],
        None => vec![before, Span::styled("_", THEME.accent_style())],
    }
}

/// Display width of the first `chars` characters of `text`.
pub(in crate::ui) fn text_width(text: &str, chars: usize) -> usize {
    width(&text.chars().take(chars).collect::<String>())
}

/// Display width of `text`, in terminal columns rather than characters.
pub(in crate::ui) fn display_width(text: &str) -> usize {
    width(text)
}

/// Wrap `input` into lines no wider than `max_width`, pairing each with the
/// character offset in `input` where it starts. Offsets count every input
/// character, including one too wide to fit on any line, so a cursor offset
/// always maps onto exactly one line.
fn wrap_text(input: &str, max_width: usize) -> Vec<(String, usize)> {
    let mut lines: Vec<(String, usize)> = Vec::new();
    let mut start = 0;
    for paragraph in input.split('\n') {
        wrap_paragraph(paragraph, max_width, start, &mut lines);
        start += paragraph.chars().count() + 1;
    }
    lines
}

fn wrap_paragraph(input: &str, max_width: usize, base: usize, lines: &mut Vec<(String, usize)>) {
    let first_line = lines.len();
    let mut current = String::new();
    let mut start = base;
    let mut consumed = 0;
    let mut chars = input.chars().peekable();

    while let Some(first) = chars.next() {
        let whitespace = first.is_whitespace();
        let mut token = String::from(first);
        while chars
            .peek()
            .is_some_and(|ch| ch.is_whitespace() == whitespace)
        {
            token.push(chars.next().expect("peeked character must exist"));
        }

        if !whitespace && !current.is_empty() && width(&current) + width(&token) > max_width {
            lines.push((std::mem::take(&mut current), start));
            start = base + consumed;
        }

        for ch in token.chars() {
            let ch_width = width(&ch.to_string());
            if width(&current) + ch_width > max_width && !current.is_empty() {
                lines.push((std::mem::take(&mut current), start));
                start = base + consumed;
            }
            if ch_width <= max_width {
                current.push(ch);
            }
            consumed += 1;
        }
    }

    if !current.is_empty() || lines.len() == first_line {
        lines.push((current, start));
    }
}

fn width(text: &str) -> usize {
    Line::from(text).width()
}

#[cfg(test)]
#[path = "input_wrap_tests.rs"]
mod tests;
