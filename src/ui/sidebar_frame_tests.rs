use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier};

use super::*;

const WIDTH: u16 = 100;
const HEIGHT: u16 = 40;

fn test_app(registry: Registry) -> App {
    let temp = tempfile::tempdir().unwrap();
    let mut app = App::new(registry, Config::default(), temp.path().into());
    app.orphans = Vec::new();
    app.set_overseer_visibility(false);
    app.set_overseer_visibility(true);
    app
}

fn panes(app: &App, area: Rect) -> layout::PaneLayout {
    let root = layout::root(area);
    layout::panes(root.body, app.overseer_frame_height())
}

fn row_text(buffer: &Buffer, x: u16, width: u16, y: u16) -> String {
    (x..x.saturating_add(width))
        .map(|column| buffer.cell((column, y)).unwrap().symbol())
        .collect()
}

#[test]
fn sidebar_has_no_frame_chrome_and_uses_inline_headers() {
    let app = test_app(Registry::default());
    let visible = app.visible();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();

    terminal
        .draw(|frame| tree::draw(frame, &app, &visible, None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let panes = panes(&app, buffer.area);
    let chrome = ['│', '─', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼'];
    for y in panes.overseer.y..panes.tree.bottom() {
        for x in panes.tree.x..panes.tree.right() {
            let symbol = buffer.cell((x, y)).unwrap().symbol();
            assert!(
                !chrome.iter().any(|character| symbol.contains(*character)),
                "box-drawing chrome {symbol:?} at ({x}, {y})"
            );
        }
    }

    assert!(row_text(buffer, panes.tree.x, panes.tree.width, panes.tree.y).starts_with("PROJECTS"));
    for x in panes.tree.x..panes.tree.x + "PROJECTS".len() as u16 {
        assert!(
            buffer
                .cell((x, panes.tree.y))
                .unwrap()
                .modifier
                .contains(Modifier::BOLD),
            "PROJECTS cell at x={x} is not bold"
        );
    }
    let rendered = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert_eq!(rendered.matches("OVERSEER").count(), 1);
}

#[test]
fn overseer_ends_with_one_blank_row_before_projects_header() {
    let app = test_app(Registry::default());
    let visible = app.visible();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();

    terminal
        .draw(|frame| tree::draw(frame, &app, &visible, None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let panes = panes(&app, buffer.area);
    let spacer_y = panes.overseer.bottom() - 1;
    assert!(
        row_text(buffer, panes.overseer.x, panes.overseer.width, spacer_y)
            .chars()
            .all(|character| character == ' '),
        "OVERSEER spacer row is not blank"
    );
    assert_eq!(spacer_y + 1, panes.tree.y);
    assert!(row_text(buffer, panes.tree.x, panes.tree.width, spacer_y + 1).starts_with("PROJECTS"));
}
