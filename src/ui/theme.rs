use ratatui::style::{Color, Modifier, Style};

use crate::model::{MergeLifecycle, Status};

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
    pub mcp: Color,
    pub subagent: Color,
    /// Colour of the "needs a human decision" badge — deliberately not
    /// `waiting`'s or `dead`'s colour, since neither meaning applies: the
    /// worker is not sitting at an interactive prompt and it is not broken.
    pub needs_decision: Color,
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
    mcp: Color::Magenta,
    subagent: Color::LightCyan,
    needs_decision: Color::LightMagenta,
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

    /// Selection-aware style for the leading indicator column (primary +
    /// supplementary spans) so it joins the solid selection bar instead of
    /// leaving a hollow gap. Reversed (`selection_fg` on `selection_bg`)
    /// because a status colour such as `running` (green) would vanish on the
    /// green selection background; bold keeps the glyph emphasised.
    pub fn selected_indicator_style(self) -> Style {
        self.selection_style().add_modifier(Modifier::BOLD)
    }

    /// The tree's structural prefix: indentation, expand handles, connectors.
    /// Drawn dimmer than the row it decorates so the state marker sitting in
    /// the same prefix reads as the part carrying meaning. A selected row keeps
    /// the selection bar solid instead.
    pub fn tree_structure_style(self, selected: bool) -> Style {
        if selected {
            self.selection_style()
        } else {
            self.muted_style()
        }
    }

    /// The Overseer management marker. Accented and bold against the dim
    /// structure beside it, so the marker and the expand handle differ in
    /// weight as well as in shape.
    pub fn management_marker_style(self, selected: bool) -> Style {
        if selected {
            self.selection_style().add_modifier(Modifier::BOLD)
        } else {
            self.accent_bold_style()
        }
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

    /// The character a text prompt's caret sits on. Reversed rather than
    /// recoloured so the caret reads on any glyph, and so it costs no extra
    /// column — inserting a caret glyph mid-string would shift the text after it.
    pub fn caret_style(self) -> Style {
        self.input_style().add_modifier(Modifier::REVERSED)
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

    pub fn mcp_style(self) -> Style {
        Style::default().fg(self.mcp)
    }

    pub fn subagent_style(self) -> Style {
        Style::default().fg(self.subagent)
    }

    /// Style for a panel admitting it could not answer. Shares the `dead`
    /// colour: both say "do not read this as normal".
    pub fn failure_style(self) -> Style {
        Style::default().fg(self.dead)
    }

    pub fn status_style(self, status: Status) -> Style {
        Style::default().fg(self.status_color(status))
    }

    /// Style for a merge-lifecycle glyph. Checks failing reuses `dead`'s
    /// colour — both mean "something is wrong, look at this" — while the two
    /// pending states and the generic hold reuse `waiting`'s: all three mean
    /// "nothing is wrong, something else has to happen first". The glyphs
    /// carry the distinction the colour does not.
    pub fn merge_lifecycle_style(self, lifecycle: MergeLifecycle) -> Style {
        let color = match lifecycle {
            MergeLifecycle::ChecksFailing => self.dead,
            MergeLifecycle::ChecksRunning
            | MergeLifecycle::WaitingJudge
            | MergeLifecycle::OnHold => self.waiting,
        };
        Style::default().fg(color)
    }

    pub fn worktree_missing_style(self, selected: bool) -> Style {
        let style = Style::default().fg(Color::Red);
        if selected {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub fn merge_failed_style(self, selected: bool) -> Style {
        let style = Style::default().fg(Color::Red);
        if selected {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    /// Style for the "needs a human decision" badge. See
    /// [`Theme::needs_decision`] for why it is its own colour.
    pub fn needs_decision_style(self, selected: bool) -> Style {
        let style = Style::default().fg(self.needs_decision);
        if selected {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }
}
