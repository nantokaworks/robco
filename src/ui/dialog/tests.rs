use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

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
