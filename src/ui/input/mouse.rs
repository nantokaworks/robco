use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::ui::{App, Mode, layout};

/// Lines moved per wheel notch; smaller than PageUp/PageDown for fine scrubbing.
const WHEEL_SCROLL_STEP: u16 = 3;

impl App {
    /// Wheel-only mouse handling: the tree stack scrolls the selection, while
    /// the preview scrolls its capture. Other mouse input is unhandled.
    pub(crate) fn handle_mouse(&mut self, event: MouseEvent, area: Rect) {
        if !matches!(self.mode, Mode::Normal) {
            return;
        }
        let up = match event.kind {
            MouseEventKind::ScrollUp => true,
            MouseEventKind::ScrollDown => false,
            _ => return,
        };

        let panes = layout::panes(layout::root(area).body, self.overseer_frame_height());
        let position = Position::new(event.column, event.row);
        if panes.tree.contains(position) || panes.overseer.contains(position) {
            if up {
                self.move_selection_up();
            } else {
                self.move_selection_down();
            }
            self.clamp_selection();
        } else if panes.preview.contains(position) {
            self.scroll_preview(up, WHEEL_SCROLL_STEP);
        }
    }
}
