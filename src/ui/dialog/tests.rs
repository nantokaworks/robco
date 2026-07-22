use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{config::Config, registry::Registry};

use super::{App, Mode, draw};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn assert_cursor_on_trailing_caret(mode: Mode) {
    let mut app = test_app();
    app.mode = mode;
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let mut cursor = None;

    terminal
        .draw(|frame| cursor = draw(frame, &app, &[]))
        .unwrap();

    let cursor = cursor.expect("text input mode should return a cursor");
    assert_eq!(
        terminal.backend().buffer().cell(cursor).unwrap().symbol(),
        "_"
    );
}

#[test]
fn prompt_agent_cursor_uses_input_row_two() {
    assert_cursor_on_trailing_caret(Mode::PromptAgent {
        repo: 0,
        with_prompt: false,
        input: "worker".to_string(),
    });
}

#[test]
fn prompt_repo_cursor_uses_input_row_zero() {
    assert_cursor_on_trailing_caret(Mode::PromptRepo {
        input: "/repo".to_string(),
    });
}

#[test]
fn wrapped_prompt_cursor_uses_last_input_line() {
    assert_cursor_on_trailing_caret(Mode::PromptOverseer {
        input: "send this instruction".to_string(),
    });
}

#[test]
fn popup_border_survives_wide_char_background() {
    let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            let bg = Paragraph::new(vec![Line::from("あいうえおかきくけこ"); 5]);
            frame.render_widget(bg, area);
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
    assert_eq!(buf.cell((3, 2)).unwrap().symbol(), "│");
    assert_eq!(buf.cell((12, 2)).unwrap().symbol(), "│");
    assert_eq!(buf.cell((2, 2)).unwrap().symbol(), " ");
}

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
    assert!(!buf.cell((6, 1)).unwrap().modifier.contains(Modifier::DIM));
    assert!(!buf.cell((5, 0)).unwrap().modifier.contains(Modifier::DIM));
    assert!(buf.cell((2, 1)).unwrap().modifier.contains(Modifier::DIM));
    assert!(buf.cell((17, 1)).unwrap().modifier.contains(Modifier::DIM));
}
