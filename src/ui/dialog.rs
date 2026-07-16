use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::model::Selection;

use super::{App, Mode, error_dialog, help, input_wrap, layout, theme::DEFAULT as THEME};

pub fn draw(frame: &mut Frame<'_>, app: &App, visible: &[Selection]) {
    let body = layout::root(frame.area()).body;
    let content_width = body.width.saturating_sub(4) as usize;
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
                input_line("git URL or path", input),
                hint_line("git format: <git-url> [branch]"),
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
        Mode::ConfirmRemoveRepo { path } => (
            "remove repo?",
            confirm_lines(path.display().to_string(), "y remove   n/esc cancel"),
        ),
        Mode::ConfirmMerge { repo, agent } => (
            "merge?",
            vec![
                Line::from(app.registry.repos[*repo].agents[*agent].branch.clone()),
                Line::from(format!("strategy: {:?}", app.config.merge_strategy).to_lowercase()),
                hint_line("y merge   n/esc cancel"),
            ],
        ),
        Mode::ConfirmPr { branch, input, .. } => {
            let max_input_height = body.height.saturating_sub(4).clamp(1, 10) as usize;
            let mut lines = vec![Line::from(format!("branch: {branch}"))];
            lines.extend(input_wrap::input_lines(
                "prompt",
                input,
                content_width,
                max_input_height,
            ));
            lines.push(hint_line("enter send   ctrl-s save only   esc cancel"));
            ("request PR from agent?", lines)
        }
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
        Mode::ErrorDialog {
            title,
            lines,
            force_kill,
        } => (title, error_dialog::content(lines, force_kill.is_some())),
        Mode::Help { .. } => ("help", help::lines()),
        Mode::Normal => return,
    };

    let width = (lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title.len()) as u16
        + 4)
    .min(body.width);
    let height = (lines.len() as u16 + 2).min(body.height);
    let area = if matches!(&app.mode, Mode::Help { .. }) {
        layout::centered_area(frame, width, height)
    } else {
        layout::popup_area(frame, app, visible, width, height)
    };

    let (title, scroll) = match app.mode {
        Mode::Help { scroll } => (
            help::scroll_title(scroll, frame.area().height).unwrap_or_else(|| title.to_string()),
            help::clamp_scroll(scroll, frame.area().height),
        ),
        Mode::ConfirmPr { .. } => {
            let cursor_row = lines.len().saturating_sub(2) as u16;
            let visible_rows = height.saturating_sub(2);
            (
                title.to_string(),
                cursor_row.saturating_add(1).saturating_sub(visible_rows),
            )
        }
        _ => (title.to_string(), 0),
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(THEME.dialog_border_style());
    let dialog = Paragraph::new(lines)
        .scroll((scroll, 0))
        .block(block)
        .style(THEME.accent_style());
    frame.render_widget(Block::default().style(THEME.backdrop_style()), body);

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
    // Painting the backdrop under the popup would leave its DIM modifier
    // (`set_style` only adds modifiers), rendering the dialog content dim.
    let right_x = area.x + area.width;
    for side in [
        Rect {
            x: band.x,
            y: band.y,
            width: area.x.saturating_sub(band.x),
            height: band.height,
        },
        Rect {
            x: right_x,
            y: band.y,
            width: (band.x + band.width).saturating_sub(right_x),
            height: band.height,
        },
    ] {
        frame.render_widget(Block::default().style(THEME.backdrop_style()), side);
    }
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
        style::{Modifier, Style},
        text::Line,
        widgets::{Block, Borders, Clear, Paragraph},
    };

    /// A full-width background must keep clean popup borders.
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

                // Odd x so the left border sits on the right half of a wide glyph.
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

    /// The backdrop's DIM must not bleed into the popup.
    #[test]
    fn popup_cells_escape_backdrop_dim() {
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let backdrop = Style::default().add_modifier(Modifier::DIM);
                frame.render_widget(Block::default().style(backdrop), area);

                let popup = Rect {
                    x: 5,
                    y: 0,
                    width: 10,
                    height: 3,
                };
                frame.render_widget(Clear, area);
                let right_x = popup.x + popup.width;
                for side in [
                    Rect {
                        x: area.x,
                        y: area.y,
                        width: popup.x.saturating_sub(area.x),
                        height: area.height,
                    },
                    Rect {
                        x: right_x,
                        y: area.y,
                        width: (area.x + area.width).saturating_sub(right_x),
                        height: area.height,
                    },
                ] {
                    frame.render_widget(Block::default().style(backdrop), side);
                }
                let dialog = Paragraph::new("input").block(Block::default().borders(Borders::ALL));
                frame.render_widget(dialog, popup);
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        // Dialog content and border cells carry no DIM.
        assert!(!buf.cell((6, 1)).unwrap().modifier.contains(Modifier::DIM));
        assert!(!buf.cell((5, 0)).unwrap().modifier.contains(Modifier::DIM));
        // The band beside the popup stays dimmed.
        assert!(buf.cell((2, 1)).unwrap().modifier.contains(Modifier::DIM));
        assert!(buf.cell((17, 1)).unwrap().modifier.contains(Modifier::DIM));
    }
}
