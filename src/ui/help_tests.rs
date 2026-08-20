//! Tests for the `?` help screen, split out of `ui::help` to keep that file
//! under this project's source file size limit.

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{config::Config, registry::Registry};

fn rendered_help_with_language(height: u16, scroll: u16, language: Option<&str>) -> String {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        language: language.map(str::to_string),
        ..Config::default()
    };
    let mut app = super::super::App::new(Registry::default(), config, temp.path().into());
    app.mode = super::super::Mode::Help { scroll };
    let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
    terminal
        .draw(|frame| {
            super::super::dialog::draw(frame, &app);
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .fold(String::new(), |mut output, cell| {
            output.push_str(cell.symbol());
            output
        })
}

fn rendered_help(height: u16, scroll: u16) -> String {
    rendered_help_with_language(height, scroll, None)
}

#[test]
fn short_terminal_can_render_last_help_line_at_max_scroll() {
    let rendered = rendered_help(24, max_scroll(24));
    assert!(rendered.contains("press any key to close"));
    assert!(rendered.contains("j/k scroll"));
}

#[test]
fn tall_terminal_keeps_original_help_title_and_content() {
    // The content rows plus FRAME_OVERHEAD_ROWS is the height at which the
    // help fits without a scroll indicator.
    let rendered = rendered_help(content_line_count() + FRAME_OVERHEAD_ROWS, 0);
    assert!(rendered.contains("press any key to close"));
    assert!(!rendered.contains("j/k scroll"));
}

#[test]
fn every_line_fits_an_80_column_terminal() {
    for locale in [Locale::En, Locale::Ja] {
        for (index, line) in lines(locale).iter().enumerate() {
            assert!(
                line.width() <= 76,
                "{locale:?} help line {} is {} columns wide",
                index + 1,
                line.width()
            );
        }
    }
}

/// The scroll ceiling has to be the content's own height. It used to be a
/// hand-written constant two hundred lines from the list it described
/// (dropr:509), so this pins the derivation instead: one row per entry,
/// and the last row reachable at `max_scroll` on a terminal one row too
/// short to show everything.
#[test]
fn the_scroll_ceiling_follows_the_content_it_scrolls() {
    let height = content_line_count() + FRAME_OVERHEAD_ROWS - 1;
    assert_eq!(max_scroll(height), 1);
    assert_eq!(max_scroll(content_line_count() + FRAME_OVERHEAD_ROWS), 0);
}

/// `content_line_count` reads the English list, so a locale that emitted a
/// different number of rows would scroll to the wrong ceiling. The Ja
/// table translates lines; it must never add or drop one.
#[test]
fn every_locale_emits_the_same_number_of_rows() {
    assert_eq!(lines(Locale::Ja).len(), lines(Locale::En).len());
    assert_eq!(lines(Locale::En).len(), content_line_count() as usize);
}

#[test]
fn scrolling_up_clamps_before_moving() {
    assert_eq!(scroll_up(u16::MAX, 24), max_scroll(24) - 1);
}

#[test]
fn an_absent_language_renders_english_help_unchanged() {
    let rendered = rendered_help_with_language(content_line_count() + FRAME_OVERHEAD_ROWS, 0, None);
    assert!(rendered.contains("press any key to close"));
    assert!(rendered.contains("Navigation"));
}

#[test]
fn an_unrecognized_language_falls_back_to_english_help() {
    let rendered = rendered_help_with_language(
        content_line_count() + FRAME_OVERHEAD_ROWS,
        0,
        Some("Brazilian Portuguese"),
    );
    assert!(rendered.contains("press any key to close"));
    assert!(rendered.contains("Navigation"));
}

// Asserted directly against `lines()` rather than through a rendered
// terminal buffer: a double-width CJK glyph occupies two buffer cells (the
// glyph, then a leftover blank continuation cell), so flattening the
// buffer cell-by-cell — fine for the single-width English fixtures above —
// does not reconstruct contiguous Japanese substrings.
#[test]
fn a_recognized_language_renders_localized_help() {
    let localized = lines(Locale::Ja);
    assert!(
        localized
            .iter()
            .any(|line| line.to_string().contains("何かキーを押すと閉じます"))
    );
    assert!(
        !localized
            .iter()
            .any(|line| line.to_string().contains("press any key to close"))
    );
}
