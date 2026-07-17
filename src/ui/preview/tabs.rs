use ratatui::text::{Line, Span};

use crate::model::Selection;
use crate::ui::{PreviewPane, panes_for, theme::DEFAULT as THEME};

pub(in crate::ui) fn preview_tabs_line(
    active: PreviewPane,
    selection: Option<Selection>,
    ai_label: &str,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, pane) in panes_for(selection).iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" │ ", THEME.muted_style()));
        }
        let label = match pane {
            PreviewPane::Info => "INFO",
            PreviewPane::Claude => ai_label,
            PreviewPane::Diff => "DIFF",
            PreviewPane::Terminal => "TERM",
        };
        let is_active = *pane == active;
        let text = if is_active {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        };
        let style = if is_active {
            THEME.selection_style()
        } else {
            THEME.muted_style()
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}
