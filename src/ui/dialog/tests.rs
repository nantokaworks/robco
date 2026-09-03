use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{config::Config, registry::Registry, ui::text_input::TextInput};

use super::{App, Mode, draw};

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

/// `text` with the cursor walked `lefts` characters back from the end.
fn edited(text: &str, lefts: usize) -> TextInput {
    let mut input = TextInput::from(text);
    for _ in 0..lefts {
        input.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    }
    input
}

/// Draw `mode` and return the terminal caret the dialog asked for.
fn draw_cursor(mode: Mode) -> ((u16, u16), ratatui::buffer::Buffer) {
    let mut app = test_app();
    app.mode = mode;
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let mut cursor = None;

    terminal.draw(|frame| cursor = draw(frame, &app)).unwrap();

    (
        cursor.expect("text input mode should return a cursor"),
        terminal.backend().buffer().clone(),
    )
}

fn assert_cursor_on_trailing_caret(mode: Mode) {
    let (cursor, buffer) = draw_cursor(mode);

    assert_eq!(buffer.cell(cursor).unwrap().symbol(), "_");
}

/// The terminal caret must land on the character the cursor is in front of, and
/// that cell must be the one the dialog marked — otherwise the operator sees the
/// caret in one place and their next keystroke lands in another.
fn assert_caret_marks(mode: Mode, expected: &str) {
    let (cursor, buffer) = draw_cursor(mode);
    let cell = buffer.cell(cursor).unwrap();

    assert_eq!(cell.symbol(), expected);
    assert!(
        cell.modifier.contains(Modifier::REVERSED),
        "caret cell is not marked"
    );
}

#[test]
fn prompt_agent_cursor_uses_input_row_two() {
    assert_cursor_on_trailing_caret(Mode::PromptAgent {
        repo: 0,
        input: "worker".into(),
    });
}

#[test]
fn prompt_repo_cursor_uses_input_row_zero() {
    assert_cursor_on_trailing_caret(Mode::PromptRepo {
        input: "/repo".into(),
    });
}

#[test]
fn wrapped_prompt_cursor_uses_last_input_line() {
    assert_cursor_on_trailing_caret(Mode::PromptOverseer {
        input: "send this instruction".into(),
    });
}

#[test]
fn prompt_agent_caret_tracks_a_mid_string_cursor() {
    assert_caret_marks(
        Mode::PromptAgent {
            repo: 0,
            input: edited("worker", 4),
        },
        "r",
    );
}

#[test]
fn prompt_repo_caret_tracks_a_mid_string_cursor() {
    assert_caret_marks(
        Mode::PromptRepo {
            input: edited("/repo", 3),
        },
        "e",
    );
}

#[test]
fn wrapped_prompt_caret_tracks_a_mid_string_cursor() {
    assert_caret_marks(
        Mode::PromptOverseer {
            input: edited("send this instruction", 11),
        },
        "i",
    );
}

#[test]
fn confirm_pr_caret_tracks_a_mid_string_cursor() {
    assert_caret_marks(
        Mode::ConfirmPr {
            repo_path: "/repo".into(),
            agent_id: "agent".to_string(),
            branch: "feature/agent".to_string(),
            input: edited("open a pull request", 8),
            approval_head: None,
        },
        " ",
    );
}

#[test]
fn inbox_prompt_caret_tracks_a_mid_string_cursor() {
    assert_caret_marks(
        Mode::PromptInbox {
            item: crate::ui::inbox::InboxItem {
                kind: crate::ui::inbox::InboxKind::Escalation,
                repo: None,
                agent_id: None,
                target_session: Some("robco-agent".to_string()),
                target_id: "agent".to_string(),
                label: "agent — worker".to_string(),
                detail: "worker_blocked".to_string(),
                at: chrono::Utc::now(),
                pr_url: None,
                pr_facts: None,
                sentence: None,
            },
            input: edited("ship it", 4),
        },
        "p",
    );
}

#[test]
fn caret_column_accounts_for_full_width_glyphs() {
    let (cursor, _) = draw_cursor(Mode::PromptRepo {
        input: edited("日本語", 1),
    });
    let (wide_start, _) = draw_cursor(Mode::PromptRepo {
        input: edited("日本語", 3),
    });

    // Two full-width glyphs precede the cursor, so the caret advances four
    // columns rather than the two a per-character count would give.
    assert_eq!(cursor.0 - wide_start.0, 4);
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
