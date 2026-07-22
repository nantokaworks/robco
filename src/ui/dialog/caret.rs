use ratatui::{layout::Rect, text::Line};

pub(super) fn caret_position(area: Rect, line: &Line<'_>, row: usize, scroll: u16) -> (u16, u16) {
    let line_width = u16::try_from(line.width().saturating_sub(1)).unwrap_or(u16::MAX);
    let row = u16::try_from(row).unwrap_or(u16::MAX);
    let x = area.x.saturating_add(1).saturating_add(line_width);
    let y = area
        .y
        .saturating_add(1)
        .saturating_add(row.saturating_sub(scroll));
    let max_x = area.right().saturating_sub(2).max(area.x);
    let max_y = area.bottom().saturating_sub(2).max(area.y);
    (x.clamp(area.x, max_x), y.clamp(area.y, max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_uses_display_width_and_scroll_offset() {
        let area = Rect::new(3, 5, 30, 8);
        let line = Line::from(" prompt: 日本_");

        assert_eq!(caret_position(area, &line, 4, 2), (17, 8));
    }

    #[test]
    fn caret_is_clamped_to_popup_area() {
        let area = Rect::new(3, 5, 5, 3);
        let line = Line::from(" input beyond popup_");

        assert_eq!(caret_position(area, &line, 20, 0), (6, 6));
    }
}
