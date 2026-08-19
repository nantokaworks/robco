//! Display-width measurement for user text (dropr:481).
//!
//! [`UnicodeWidthStr::width`] already answers correctly for a whole string:
//! given `"♻️"` (`U+267B` `RECYCLING SYMBOL` plus `U+FE0F`
//! `VARIATION SELECTOR-16`) it scans ahead, sees the pair forms an emoji
//! presentation sequence, and answers 2 — the two columns a terminal actually
//! draws — even though `U+267B` alone measures 1. The bug this module exists
//! to prevent is not in that crate; it is in code that measures one `char` at
//! a time (`UnicodeWidthChar::width`), which throws that context away and
//! always answers for the base scalar alone (1 for `♻`, 0 for `U+FE0F`
//! itself), quietly undercounting the pair by one column. Any code that needs
//! an incremental or truncating scan over `text` — not just its total width —
//! must walk [`next_cluster`] rather than `char`s, so a base-plus-`U+FE0F`
//! pair is never split or measured as half of itself.
use unicode_width::UnicodeWidthStr;

const VARIATION_SELECTOR_16: char = '\u{FE0F}';

/// Total display width of `text`, in terminal columns.
pub(in crate::ui) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// The byte length and display width of `text`'s first display cluster — a
/// lone scalar, or a scalar immediately followed by `U+FE0F`. `None` when
/// `text` is empty.
pub(in crate::ui) fn next_cluster(text: &str) -> Option<(usize, usize)> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    let mut end = first.len_utf8();
    if let Some((index, selector)) = chars.next()
        && selector == VARIATION_SELECTOR_16
    {
        end = index + selector.len_utf8();
    }
    Some((end, UnicodeWidthStr::width(&text[..end])))
}

/// The longest byte prefix of `text` whose display width does not exceed
/// `max_width`. Walks [`next_cluster`] rather than `char`s so a
/// base-plus-`U+FE0F` pair is never split across the boundary.
pub(in crate::ui) fn prefix_within(text: &str, max_width: usize) -> &str {
    let mut width = 0;
    let mut end = 0;
    while let Some((cluster_len, cluster_width)) = next_cluster(&text[end..]) {
        if width + cluster_width > max_width {
            break;
        }
        width += cluster_width;
        end += cluster_len;
    }
    &text[..end]
}

/// The byte offset of the first cluster whose accumulated width reaches
/// `target`, or `text.len()` when `target` is never reached.
pub(in crate::ui) fn byte_at_or_after_width(text: &str, target: usize) -> usize {
    let mut width = 0;
    let mut offset = 0;
    while let Some((cluster_len, cluster_width)) = next_cluster(&text[offset..]) {
        if width >= target {
            return offset;
        }
        width += cluster_width;
        offset += cluster_len;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real board titles (dropr:481): `♻️` and `⚡️` are base-plus-U+FE0F
    // pairs that measure 1 column per-scalar but draw 2; `🐛` is a single
    // scalar that already measures 2 either way.
    const RECYCLE_TITLE: &str = "♻️ [api] Retire the polling dispatcher";
    const BOLT_TITLE: &str = "⚡️ [web] Add user invitation feature";
    const BUG_TITLE: &str = "🐛 [ui] Say which repository an Inbox row is about";
    const ASCII_TITLE: &str = "eb [ui] plain ascii title";

    #[test]
    fn variation_selector_pair_counts_as_two_columns() {
        assert_eq!(display_width("♻️"), 2);
        assert_eq!(display_width("⚡️"), 2);
        // The base scalar alone (no U+FE0F) stays whatever it already was.
        assert_eq!(display_width("♻"), 1);
    }

    #[test]
    fn single_codepoint_emoji_still_counts_as_two_columns() {
        assert_eq!(display_width("🐛"), 2);
        assert_eq!(display_width("✨"), 2);
    }

    #[test]
    fn ascii_counts_one_column_per_character() {
        assert_eq!(display_width("abc"), 3);
    }

    #[test]
    fn board_titles_report_the_terminal_drawn_width() {
        // "♻️ " is 3 columns (2 for the pair, 1 for the space); "⚡️ " and "🐛 "
        // are the same shape. Every title here starts `{emoji} [scope] `, so
        // trimming each to its first 3 columns keeps only the emoji + space.
        assert_eq!(prefix_within(RECYCLE_TITLE, 3), "♻️ ");
        assert_eq!(prefix_within(BOLT_TITLE, 3), "⚡️ ");
        assert_eq!(prefix_within(BUG_TITLE, 3), "🐛 ");
    }

    #[test]
    fn rows_built_from_every_prefix_style_line_up() {
        // A row is `{prefix}{padding}` out to a fixed column: the same
        // budget spent on a variation-selector emoji, a single-codepoint
        // emoji, or plain ASCII must leave the same amount of padding once
        // widths are measured correctly.
        let column = 6;
        for title in [RECYCLE_TITLE, BOLT_TITLE, BUG_TITLE, ASCII_TITLE] {
            let prefix = prefix_within(title, column);
            let padded_width = display_width(prefix) + (column - display_width(prefix));
            assert_eq!(padded_width, column, "title {title:?} misaligns the row");
        }
    }

    #[test]
    fn prefix_within_never_splits_a_variation_selector_pair() {
        // A budget that lands mid-pair (width 1, when the pair needs 2) must
        // drop the whole pair rather than emit a lone base scalar that would
        // then draw as width 1 while the byte-accounting still called it 0
        // remaining — the exact off-by-one dropr:481 reports.
        assert_eq!(prefix_within("♻️x", 1), "");
        assert_eq!(prefix_within("♻️x", 2), "♻️");
        assert_eq!(prefix_within("♻️x", 3), "♻️x");
    }

    #[test]
    fn byte_at_or_after_width_keeps_a_variation_selector_pair_together() {
        let text = "♻️tail";
        // Width 1 lands inside the pair (it spends 2 columns): the pair
        // cannot be shown half-scrolled, so the offset skips it whole
        // rather than landing on the byte between the base scalar and
        // U+FE0F — which is exactly the split the old char-by-char scan
        // produced.
        assert_eq!(byte_at_or_after_width(text, 1), "♻️".len());
        assert_eq!(byte_at_or_after_width(text, 2), "♻️".len());
    }
}
