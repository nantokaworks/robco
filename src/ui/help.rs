use ratatui::text::{Line, Span};

use super::theme::DEFAULT as THEME;
use crate::locale::{Locale, t};

pub(crate) const CONTENT_LINE_COUNT: u16 = 67;

pub(crate) fn lines(locale: Locale) -> Vec<Line<'static>> {
    let l = |text: &'static str| Line::from(t(locale, text));
    vec![
        l("Navigation"),
        l("  j/k or arrows  move selection; OVERSEER rows open local control info"),
        l("  h/l            collapse or expand repo or OVERSEER category"),
        l("  shift-up/down  on a repo row: move it among its sibling repos"),
        l("  tab/shift-tab  cycle claude / diff / terminal view"),
        Line::from(""),
        l("Sessions"),
        l("  n              new agent under selected repo (title | initial prompt)"),
        l("  enter          attach Claude/terminal (agent shell or main worktree)"),
        l("                 on OVERSEER: attach the control AI, creating it if absent"),
        l("  i              on the OVERSEER control AI row: send it an instruction"),
        l("  ctrl-q         return from attached tmux session"),
        l("  r              on a repo row: reload dropr tasks; else restart agent"),
        l("  x              remove selected agent worktree or pinned repo"),
        l("  g              cycle selected worktree: unmanaged -> Auto -> Manual"),
        l("                 on a repo row: any Auto -> all Manual, else all Auto"),
        l("  G              on a repo row: toggle Overseer opt-out for the whole repo"),
        l("                 (opted out: no dispatch/merge; running workers untouched)"),
        l("  S              stop dispatch (kill workers); off: start dispatch"),
        l("  R              reset dispatch circuit (open) or start the daemon"),
        l("                 (not running)"),
        l("  K              stop the overseer daemon process (running)"),
        l("Overseer inbox"),
        l("  l              expand OVERSEER > Inbox to reach its item rows"),
        l("  enter          on an item row: answer the waiting worker"),
        l("  y              on an item row: approve it (sends y + enter)"),
        l("  d              on an item row: dismiss it (hides the row only)"),
        l("  D              on an item or Inbox row: clear the inbox (confirms)"),
        l("                 also: robco overseer clear-inbox"),
        l("Overseer discord"),
        l("  l              expand OVERSEER > Discord to reach its channel rows"),
        l("  enter          on a channel row: attach its tmux session (live"),
        l("                 only while a turn is running for that channel)"),
        l("Repo"),
        l("  a              clone <git-url> [branch], or add local repo path"),
        l("  m              merge agent: merge PR + pull main (commit + PR needed)"),
        l("                 already-merged PR: clean up without merging again"),
        l("  p              edit and request PR from selected running agent"),
        Line::from(""),
        l("Text prompts"),
        l("  left/right     move the cursor within the text being typed"),
        l("  home/ctrl-a    jump to the start; end/ctrl-e jumps to the end"),
        l("  backspace/del  delete before the cursor / at the cursor"),
        l("  ctrl-w/ctrl-u  delete the previous word / back to the line start"),
        Line::from(""),
        l("Indicators"),
        l("  One primary per row: dead > running > waiting > TERM activity"),
        l("    > subagents > dropr reload > static status"),
        l("  ⠋… animated agent running   ? waiting   ✗ dead"),
        l("  ⌦ worktree missing (appended after primary; alone if no primary)"),
        l("  merge-failed native merge failed (appended after primary)"),
        l("  ▖… animated TERM activity   ✻N active subagents"),
        l("  ⟳ manual dropr reload (r key)"),
        l("  ✓ done   · idle   ⎇ branch only (static fallback)"),
        l("  ● overseer Auto   ○ overseer Manual   blank unmanaged (rides indent)"),
        l("  nerdfont project_icon swaps in a bolt/hand pictograph pair instead"),
        l("  Repo row always shows its own marker; Auto agent rows always show"),
        l("  theirs too; a Manual agent row blanks only when its repo is Manual"),
        l("  Opted-out repo (○): name and its agent rows render dimmed"),
        l("  Collapsed repos: N ⠿ is running; status glyphs/N ⌦ are child counts"),
        l("  Child rows: * uncommitted changes   ⌁ tmux session"),
        l("General"),
        l("  ,              edit settings (config.json) in $EDITOR"),
        l("  ?              show this help"),
        l("  q              quit without stopping agents"),
        Line::from(""),
        Line::from(Span::styled(
            t(locale, "press any key to close"),
            THEME.hint_style(),
        )),
    ]
}

/// Rows the frame loses around the help content: the 1-row top margin and
/// 1-row footer from `layout::root`, plus the popup's two border rows.
const FRAME_OVERHEAD_ROWS: u16 = 4;

pub(crate) fn max_scroll(frame_height: u16) -> u16 {
    let visible_rows = frame_height.saturating_sub(FRAME_OVERHEAD_ROWS);
    CONTENT_LINE_COUNT.saturating_sub(visible_rows)
}

pub(crate) fn clamp_scroll(scroll: u16, frame_height: u16) -> u16 {
    scroll.min(max_scroll(frame_height))
}

pub(crate) fn scroll_up(scroll: u16, frame_height: u16) -> u16 {
    clamp_scroll(scroll, frame_height).saturating_sub(1)
}

pub(crate) fn scroll_down(scroll: u16, frame_height: u16) -> u16 {
    clamp_scroll(scroll.saturating_add(1), frame_height)
}

pub(crate) fn terminal_height() -> u16 {
    crossterm::terminal::size()
        .map(|(_, height)| height)
        .unwrap_or(CONTENT_LINE_COUNT + FRAME_OVERHEAD_ROWS)
}

pub(crate) fn scroll_title(scroll: u16, frame_height: u16, locale: Locale) -> Option<String> {
    let max = max_scroll(frame_height);
    (max > 0).then(|| {
        crate::locale::fmt(
            locale,
            "help [j/k scroll {}/{}]",
            &[
                &clamp_scroll(scroll, frame_height).to_string(),
                &max.to_string(),
            ],
        )
    })
}

#[cfg(test)]
mod tests {
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
        assert_eq!(lines(Locale::En).len(), CONTENT_LINE_COUNT as usize);
        let rendered = rendered_help(24, max_scroll(24));
        assert!(rendered.contains("press any key to close"));
        assert!(rendered.contains("j/k scroll"));
    }

    #[test]
    fn tall_terminal_keeps_original_help_title_and_content() {
        // CONTENT_LINE_COUNT rows plus FRAME_OVERHEAD_ROWS is the height at which
        // the help fits without a scroll indicator.
        let rendered = rendered_help(CONTENT_LINE_COUNT + FRAME_OVERHEAD_ROWS, 0);
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

    #[test]
    fn scrolling_up_clamps_before_moving() {
        assert_eq!(scroll_up(u16::MAX, 24), max_scroll(24) - 1);
    }

    #[test]
    fn an_absent_language_renders_english_help_unchanged() {
        let rendered =
            rendered_help_with_language(CONTENT_LINE_COUNT + FRAME_OVERHEAD_ROWS, 0, None);
        assert!(rendered.contains("press any key to close"));
        assert!(rendered.contains("Navigation"));
    }

    #[test]
    fn an_unrecognized_language_falls_back_to_english_help() {
        let rendered = rendered_help_with_language(
            CONTENT_LINE_COUNT + FRAME_OVERHEAD_ROWS,
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
}
