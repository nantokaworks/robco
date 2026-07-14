use ratatui::style::{Color, Modifier, Style};

use crate::model::Status;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub muted: Color,
    pub hint: Color,
    pub selection_fg: Color,
    pub selection_bg: Color,
    pub dialog_border: Color,
    pub input: Color,
    pub running: Color,
    pub waiting: Color,
    pub done: Color,
    pub idle: Color,
    pub dead: Color,
    pub branch_only: Color,
    /// Colour of the companion TERM (shell) session's working mark.
    pub term: Color,
    pub subagent: Color,
}

pub const DEFAULT: Theme = Theme {
    accent: Color::Green,
    muted: Color::DarkGray,
    hint: Color::Gray,
    selection_fg: Color::Black,
    selection_bg: Color::Green,
    dialog_border: Color::Green,
    input: Color::White,
    running: Color::Green,
    waiting: Color::Yellow,
    done: Color::Cyan,
    idle: Color::Gray,
    dead: Color::Red,
    branch_only: Color::DarkGray,
    term: Color::Blue,
    subagent: Color::LightCyan,
};

impl Theme {
    pub fn accent_style(self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn accent_bold_style(self) -> Style {
        self.accent_style().add_modifier(Modifier::BOLD)
    }

    pub fn muted_style(self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn backdrop_style(self) -> Style {
        Style::default().fg(self.muted).add_modifier(Modifier::DIM)
    }

    pub fn hint_style(self) -> Style {
        Style::default().fg(self.hint)
    }

    pub fn selection_style(self) -> Style {
        Style::default().fg(self.selection_fg).bg(self.selection_bg)
    }

    pub fn dialog_border_style(self) -> Style {
        Style::default().fg(self.dialog_border)
    }

    pub fn dialog_label_style(self) -> Style {
        self.accent_style().add_modifier(Modifier::BOLD)
    }

    pub fn input_style(self) -> Style {
        Style::default().fg(self.input)
    }

    fn status_color(self, status: Status) -> Color {
        match status {
            Status::Running => self.running,
            Status::Waiting => self.waiting,
            Status::Done => self.done,
            Status::Idle => self.idle,
            Status::Dead => self.dead,
            Status::BranchOnly => self.branch_only,
        }
    }

    /// Style for the companion TERM (shell) working mark.
    pub fn term_style(self) -> Style {
        Style::default().fg(self.term)
    }

    pub fn subagent_style(self) -> Style {
        Style::default().fg(self.subagent)
    }

    pub fn status_style(self, status: Status) -> Style {
        Style::default().fg(self.status_color(status))
    }

    pub fn selected_status_style(self, status: Status) -> Style {
        Style::default()
            .fg(self.status_color(status))
            .add_modifier(Modifier::BOLD)
    }

    pub fn worktree_missing_style(self, selected: bool) -> Style {
        let style = Style::default().fg(Color::Red);
        if selected {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }
}
