//! Shared rendering helpers for `tree`'s test modules. Split out of `tests.rs`
//! so a second test module (`task_number_tests`) can reuse the same harness
//! without pushing either file over the source-size limit.

use ratatui::{Terminal, backend::TestBackend};

use super::*;

pub(super) const WIDTH: u16 = 60;
pub(super) const HEIGHT: u16 = 12;

pub(super) fn rendered_rows(app: &App) -> Vec<String> {
    rendered_rows_at_width(app, WIDTH)
}

pub(super) fn rendered_rows_at_width(app: &App, width: u16) -> Vec<String> {
    let visible = app.visible();
    let mut terminal = Terminal::new(TestBackend::new(width, HEIGHT)).unwrap();
    terminal
        .draw(|frame| draw(frame, app, &visible, None))
        .unwrap();
    let buffer = terminal.backend().buffer();
    // Rows are read from the tree pane's own left edge so a row string starts
    // with the cursor cell, not with the frame margin.
    let tree = layout::panes(layout::root(buffer.area).body, app.overseer_frame_height()).tree;
    (tree.y..tree.bottom())
        .map(|y| {
            (tree.x..tree.right())
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect()
        })
        .collect()
}

pub(super) fn row_containing(rows: &[String], title: &str) -> String {
    rows.iter()
        .find(|row| row.contains(title))
        .unwrap_or_else(|| panic!("no rendered row for {title}"))
        .clone()
}

/// The column the title starts at, counted in cells rather than bytes or display
/// width so neither the multi-byte marker nor a two-column project icon distorts
/// it. A wide glyph lives in a single cell and its second column is left blank,
/// so one cell is one column here.
pub(super) fn title_column(rows: &[String], title: &str) -> usize {
    let row = row_containing(rows, title);
    let byte = row.find(title).unwrap();
    row[..byte].chars().count()
}
