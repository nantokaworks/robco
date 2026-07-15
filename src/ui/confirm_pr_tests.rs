use ratatui::{Terminal, backend::TestBackend};

use super::*;

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

#[test]
fn prompt_renders_wrapped_with_editing_end_visible() {
    let mut app = test_app();
    let prompt = app.config.pr_prompt.clone();
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "agent".to_string(),
        branch: "feature/agent".to_string(),
        input: prompt.clone(),
    };
    let mut terminal = Terminal::new(TestBackend::new(50, 20)).unwrap();

    terminal
        .draw(|frame| dialog::draw(frame, &app, &[]))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ");
    for word in prompt.split_whitespace() {
        assert!(rendered.contains(word), "missing prompt word: {word}");
    }
    assert!(rendered.contains("conventions._"));
}

#[test]
fn cursor_stays_visible_in_a_short_terminal() {
    let mut app = test_app();
    app.mode = Mode::ConfirmPr {
        repo_path: "/repo".into(),
        agent_id: "agent".to_string(),
        branch: "feature/agent".to_string(),
        input: "editing end".to_string(),
    };
    let mut terminal = Terminal::new(TestBackend::new(50, 5)).unwrap();

    terminal
        .draw(|frame| dialog::draw(frame, &app, &[]))
        .unwrap();

    assert!((0..terminal.backend().buffer().area.width).any(|x| {
        terminal
            .backend()
            .buffer()
            .cell((x, 2))
            .is_some_and(|cell| cell.symbol() == "_")
    }));
}
