use ratatui::text::{Line, Span};

use super::theme::DEFAULT as THEME;

pub(super) fn input_lines(
    label: &str,
    input: &str,
    max_width: usize,
    max_lines: usize,
) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

    let full_prefix = format!(" {label}: ");
    let widest_char = input
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
    let mut wrapped = wrap_text(input, input_width);

    if wrapped
        .last()
        .is_some_and(|line| width(line) >= input_width)
    {
        wrapped.push(String::new());
    }

    let wrapped_len = wrapped.len();
    let visible_from = wrapped_len.saturating_sub(max_lines.max(1));
    wrapped
        .into_iter()
        .skip(visible_from)
        .enumerate()
        .map(|(index, text)| {
            let label = if index == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix_width)
            };
            let mut spans = vec![
                Span::styled(label, THEME.dialog_label_style()),
                Span::styled(text, THEME.input_style()),
            ];
            if index + visible_from + 1 == wrapped_len {
                spans.push(Span::styled("_", THEME.accent_style()));
            }
            Line::from(spans)
        })
        .collect()
}

fn wrap_text(input: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
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
            lines.push(std::mem::take(&mut current));
        }

        for ch in token.chars() {
            let ch_width = width(&ch.to_string());
            if width(&current) + ch_width > max_width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if ch_width <= max_width {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn width(text: &str) -> usize {
    Line::from(text).width()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words_to_available_width() {
        let lines = input_lines("prompt", "one two three four", 16, 10);

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [" prompt: one two", "          three ", "         four_"]
        );
        assert!(lines.iter().all(|line| line.width() <= 16));
    }

    #[test]
    fn wraps_cjk_without_splitting_characters() {
        let lines = input_lines("prompt", "日本語入力", 14, 10);

        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [" prompt: 日本", "         語入", "         力_"]
        );
        assert!(lines.iter().all(|line| line.width() <= 14));
    }

    #[test]
    fn tail_scroll_keeps_cursor_visible() {
        let lines = input_lines("prompt", "one two three four five", 16, 2);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].to_string(), " prompt: four ");
        assert_eq!(lines[1].to_string(), "         five_");
    }

    #[test]
    fn narrow_widths_keep_lines_bounded_and_cursor_visible() {
        for max_width in 0..=9 {
            let lines = input_lines("prompt", "日本", max_width, 10);

            assert!(lines.iter().all(|line| line.width() <= max_width));
            if max_width > 0 {
                assert!(lines.last().unwrap().to_string().ends_with('_'));
            }
        }
    }

    #[test]
    fn preserves_whitespace_and_trailing_cursor_position() {
        let lines = input_lines("prompt", "  one  two  ", 30, 10);
        let rendered = lines.iter().map(ToString::to_string).collect::<Vec<_>>();

        assert_eq!(rendered, [" prompt:   one  two  _"]);
    }

    #[test]
    fn wrapping_does_not_normalize_whitespace() {
        let wrapped = wrap_text(" one   two ", 6);

        assert_eq!(wrapped.concat(), " one   two ");
        assert!(wrapped.iter().all(|line| width(line) <= 6));
    }
}
