use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::model::Selection;

use super::{App, Mode, layout, theme::DEFAULT as THEME};

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection]) {
    let (title, lines): (&str, Vec<Line<'static>>) = match &app.mode {
        Mode::PromptAgent {
            with_prompt, input, ..
        } => {
            let (title, description, label) = if *with_prompt {
                (
                    "new agent + prompt",
                    "Create a new agent and send an initial prompt.",
                    "agent",
                )
            } else {
                (
                    "new agent",
                    "Create a new agent under the selected repo.",
                    "agent title",
                )
            };

            let mut lines = vec![
                Line::from(Span::styled(description, THEME.accent_style())),
                Line::from(""),
                input_line(label, input),
            ];

            if *with_prompt {
                lines.push(hint_line("format: title | initial prompt"));
            }

            lines.extend([Line::from(""), hint_line("enter create   esc cancel")]);

            (title, lines)
        }
        Mode::PromptRepo { input } => (
            "add repo",
            vec![
                input_line("repo path", input),
                hint_line("enter add   esc cancel"),
            ],
        ),
        Mode::ConfirmKill { repo, agent } => (
            "delete worktree?",
            confirm_lines(
                app.registry.repos[*repo].agents[*agent].title.clone(),
                "y delete   n/esc cancel",
            ),
        ),
        Mode::ConfirmMerge { repo, agent } => (
            "merge / land?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(format!("strategy: {:?}", app.config.merge_strategy).to_lowercase()),
                hint_line("y merge   n/esc cancel"),
            ],
        ),
        Mode::ConfirmDeleteBranch { repo, agent } => (
            "delete branch?",
            confirm_lines(
                app.registry.repos[*repo].agents[*agent].branch.clone(),
                "y delete   n/esc keep",
            ),
        ),
        Mode::ConfirmKillOrphan { session } => (
            "kill session?",
            confirm_lines(session.clone(), "y kill   n/esc cancel"),
        ),
        Mode::Help => (
            "help",
            vec![
                Line::from("Navigation"),
                Line::from("  j/k or arrows  move selection"),
                Line::from("  h/l            collapse or expand repo"),
                Line::from("  tab            cycle claude / diff / terminal view"),
                Line::from(""),
                Line::from("Sessions"),
                Line::from("  n              new agent under selected repo"),
                Line::from("  N              new agent with initial prompt: title | prompt"),
                Line::from(
                    "  enter          attach: claude, or terminal (agent shell / repo main worktree)",
                ),
                Line::from("  ctrl-q         return from attached tmux session"),
                Line::from("  r              restart selected agent"),
                Line::from(
                    "  x              remove selected agent worktree, then optionally delete branch",
                ),
                Line::from(""),
                Line::from("Repo"),
                Line::from("  a              add repository by path"),
                Line::from(
                    "  m              merge/land selected agent: merge PR + pull main (needs commit + open PR)",
                ),
                Line::from(""),
                Line::from("General"),
                Line::from("  ,              edit settings (config.json) in $EDITOR"),
                Line::from("  ?              show this help"),
                Line::from("  q              quit without stopping agents"),
                Line::from(""),
                hint_line("press any key to close"),
            ],
        ),
        Mode::Message(_) | Mode::Normal => return,
    };

    let width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title.len()) as u16
        + 4;
    let height = lines.len() as u16 + 2;
    let area = if matches!(&app.mode, Mode::Help) {
        layout::centered_area(frame, width, height)
    } else {
        layout::popup_area(frame, app, visible, width, height)
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(THEME.dialog_border_style());
    let dialog = Paragraph::new(lines)
        .block(block)
        .style(THEME.accent_style());
    let body = layout::root(frame.area()).body;
    frame.render_widget(Block::default().style(THEME.backdrop_style()), body);

    // Blank the full-width band of rows the popup occupies before drawing it.
    // `Clear` only resets cells *inside* the popup rect, so a full-width (CJK)
    // glyph in the dimmed background that straddles the popup's left/right border
    // would leave a stray half-cell that corrupts the border. Wiping the whole
    // row-band removes any such glyph; rows above and below stay dimmed.
    let band = Rect {
        x: body.x,
        y: area.y,
        width: body.width,
        height: area.height,
    };
    frame.render_widget(Clear, band);
    frame.render_widget(Block::default().style(THEME.backdrop_style()), band);
    frame.render_widget(dialog, area);
}

fn confirm_lines(subject: String, hint: &str) -> Vec<Line<'static>> {
    vec![Line::from(subject), hint_line(hint)]
}

fn input_line(label: &str, input: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label}: "), THEME.dialog_label_style()),
        Span::styled(input.to_string(), THEME.input_style()),
        Span::styled("_", THEME.accent_style()),
    ])
}

fn hint_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), THEME.hint_style()))
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        text::Line,
        widgets::{Block, Borders, Clear, Paragraph},
    };

    /// A popup drawn over a full-width (CJK) background must keep clean vertical
    /// borders. `Clear` alone leaves the outside half of a wide glyph that
    /// straddles the popup edge, corrupting the border — `draw` blanks the
    /// popup's full-width row-band first to prevent it. This guards that fix.
    #[test]
    fn popup_border_survives_wide_char_background() {
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let bg = Paragraph::new(vec![
                    Line::from("あいうえおかきくけこ"),
                    Line::from("あいうえおかきくけこ"),
                    Line::from("あいうえおかきくけこ"),
                    Line::from("あいうえおかきくけこ"),
                    Line::from("あいうえおかきくけこ"),
                ]);
                frame.render_widget(bg, area);

                // Odd x so the left border lands on the right half of a wide glyph.
                let popup = Rect {
                    x: 3,
                    y: 1,
                    width: 10,
                    height: 3,
                };
                let band = Rect {
                    x: area.x,
                    y: popup.y,
                    width: area.width,
                    height: popup.height,
                };
                frame.render_widget(Clear, band);
                frame.render_widget(Block::default().borders(Borders::ALL), popup);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        // Both vertical borders render as box-drawing, not CJK remnants.
        assert_eq!(buf.cell((3, 2)).unwrap().symbol(), "│");
        assert_eq!(buf.cell((12, 2)).unwrap().symbol(), "│");
        // The cell just outside the left border is blanked (no stray wide half).
        assert_eq!(buf.cell((2, 2)).unwrap().symbol(), " ");
    }
}
