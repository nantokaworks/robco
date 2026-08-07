//! Every modal dialog now renders at the center of the body instead of
//! anchored below the selected tree row — this file checks that geometric
//! property directly, from the rendered corner glyphs rather than the
//! layout math under test.

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

use crate::{config::Config, registry::Registry};

use super::super::{App, Mode, layout};
use super::draw;

fn test_app() -> App {
    let temp = tempfile::tempdir().unwrap();
    App::new(Registry::default(), Config::default(), temp.path().into())
}

fn rendered_dialog_rect(buffer: &Buffer, area: Rect) -> Rect {
    let corner = |symbol: &str| {
        (area.y..area.bottom())
            .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
            .find(|&(x, y)| buffer.cell((x, y)).unwrap().symbol() == symbol)
            .unwrap_or_else(|| panic!("no {symbol:?} corner found"))
    };
    let (x1, y1) = corner("┌");
    let (x2, y2) = corner("┘");
    Rect {
        x: x1,
        y: y1,
        width: x2 - x1 + 1,
        height: y2 - y1 + 1,
    }
}

/// A dialog centered in the body has equal left/right and top/bottom margins
/// (within one cell for an odd remainder).
fn assert_dialog_is_centered(mode: Mode) {
    let mut app = test_app();
    app.mode = mode;
    let mut terminal = Terminal::new(TestBackend::new(70, 24)).unwrap();

    terminal.draw(|frame| _ = draw(frame, &app)).unwrap();

    let buffer = terminal.backend().buffer();
    let body = layout::root(buffer.area).body;
    let dialog = rendered_dialog_rect(buffer, body);

    let left_margin = dialog.x - body.x;
    let right_margin = body.right() - dialog.right();
    let top_margin = dialog.y - body.y;
    let bottom_margin = body.bottom() - dialog.bottom();
    assert!(
        left_margin.abs_diff(right_margin) <= 1,
        "not centered horizontally: left={left_margin} right={right_margin}"
    );
    assert!(
        top_margin.abs_diff(bottom_margin) <= 1,
        "not centered vertically: top={top_margin} bottom={bottom_margin}"
    );
}

#[test]
fn pr_precheck_modal_is_centered() {
    assert_dialog_is_centered(Mode::PrPrecheck {
        repo_path: "/repo".into(),
        agent_id: "agent".into(),
        branch: "feature/agent".into(),
    });
}

#[test]
fn confirm_dialog_is_centered() {
    assert_dialog_is_centered(Mode::ConfirmKillOrphan {
        session: "orphan-session".into(),
    });
}

#[test]
fn error_dialog_is_centered() {
    assert_dialog_is_centered(Mode::ErrorDialog {
        title: "failed".into(),
        lines: vec!["something went wrong".into()],
        force_kill: None,
    });
}

#[test]
fn help_dialog_is_centered() {
    assert_dialog_is_centered(Mode::Help { scroll: 0 });
}
