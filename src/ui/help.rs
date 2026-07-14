use ratatui::text::{Line, Span};

use super::theme::DEFAULT as THEME;

pub(crate) const CONTENT_LINE_COUNT: u16 = 29;

pub(crate) fn lines() -> Vec<Line<'static>> {
    vec![
        Line::from("Navigation"),
        Line::from("  j/k or arrows  move selection"),
        Line::from("  h/l            collapse or expand repo"),
        Line::from("  tab/shift-tab  cycle claude / diff / terminal view"),
        Line::from(""),
        Line::from("Sessions"),
        Line::from("  n              new agent under selected repo"),
        Line::from("  N              new agent with initial prompt: title | prompt"),
        Line::from("  enter          attach Claude/terminal (agent shell or main worktree)"),
        Line::from("  ctrl-q         return from attached tmux session"),
        Line::from("  r              restart agent / reload dropr tasks (all workspaces)"),
        Line::from("  x              remove selected agent worktree or pinned repo"),
        Line::from(""),
        Line::from("Repo"),
        Line::from("  a              add repository by path"),
        Line::from("  m              merge/land agent: merge PR + pull main (commit + PR needed)"),
        Line::from(""),
        Line::from("Indicators"),
        Line::from("  ▶ running   ? waiting   ✓ done   · idle"),
        Line::from("  ✗ dead   ⎇ branch only   ⌦ worktree missing"),
        Line::from("  ✻N active subagents   ▖… TERM working (animated)"),
        Line::from("  * uncommitted changes   ⌁ tmux session (child rows)"),
        Line::from(""),
        Line::from("General"),
        Line::from("  ,              edit settings (config.json) in $EDITOR"),
        Line::from("  ?              show this help"),
        Line::from("  q              quit without stopping agents"),
        Line::from(""),
        Line::from(Span::styled("press any key to close", THEME.hint_style())),
    ]
}

pub(crate) fn max_scroll(frame_height: u16) -> u16 {
    let visible_rows = frame_height.saturating_sub(3);
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
        .unwrap_or(CONTENT_LINE_COUNT + 3)
}

pub(crate) fn scroll_title(scroll: u16, frame_height: u16) -> Option<String> {
    let max = max_scroll(frame_height);
    (max > 0).then(|| {
        format!(
            "help [j/k scroll {}/{}]",
            clamp_scroll(scroll, frame_height),
            max
        )
    })
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{config::Config, registry::Registry};

    fn rendered_help(height: u16, scroll: u16) -> String {
        let temp = tempfile::tempdir().unwrap();
        let mut app =
            super::super::App::new(Registry::default(), Config::default(), temp.path().into());
        app.mode = super::super::Mode::Help { scroll };
        let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
        terminal
            .draw(|frame| super::super::dialog::draw(frame, &app, &[]))
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

    #[test]
    fn short_terminal_can_render_last_help_line_at_max_scroll() {
        assert_eq!(lines().len(), CONTENT_LINE_COUNT as usize);
        let rendered = rendered_help(24, max_scroll(24));
        assert!(rendered.contains("press any key to close"));
        assert!(rendered.contains("j/k scroll"));
    }

    #[test]
    fn tall_terminal_keeps_original_help_title_and_content() {
        let rendered = rendered_help(40, 0);
        assert!(rendered.contains("press any key to close"));
        assert!(!rendered.contains("j/k scroll"));
    }

    #[test]
    fn every_line_fits_an_80_column_terminal() {
        for (index, line) in lines().iter().enumerate() {
            assert!(
                line.width() <= 76,
                "help line {} is {} columns wide",
                index + 1,
                line.width()
            );
        }
    }

    #[test]
    fn scrolling_up_clamps_before_moving() {
        assert_eq!(scroll_up(u16::MAX, 24), max_scroll(24) - 1);
    }
}
