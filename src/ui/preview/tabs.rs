use ratatui::text::{Line, Span};

use crate::model::Selection;
use crate::ui::{App, PreviewPane, merge_dialog, panes_for, theme::DEFAULT as THEME};

impl App {
    /// Preview tabs currently offered for `selection`, in display order: the
    /// selection type's fixed tabs plus the error tab, which exists only while
    /// that selection carries a merge failure nobody has dismissed yet.
    ///
    /// The error tab is appended last on purpose. It keeps the default tab (the
    /// first entry) unchanged, so a failure announces itself in red without
    /// stealing the pane the operator was reading.
    pub(in crate::ui) fn preview_panes(&self, selection: Option<Selection>) -> Vec<PreviewPane> {
        let mut panes = panes_for(selection).to_vec();
        if merge_dialog::has_error(self, selection) {
            panes.push(PreviewPane::Error);
        }
        panes
    }
}

/// Render `panes` as the tab bar drawn into the preview block's top border.
/// The caller supplies the list — see [`App::preview_panes`] — because it is not
/// a property of the selection alone.
pub(in crate::ui) fn preview_tabs_line(
    active: PreviewPane,
    panes: &[PreviewPane],
    ai_label: &str,
) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, pane) in panes.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" │ ", THEME.muted_style()));
        }
        let label = match pane {
            PreviewPane::Info => "INFO",
            PreviewPane::Claude => ai_label,
            PreviewPane::Diff => "DIFF",
            PreviewPane::Terminal => "TERM",
            PreviewPane::Error => "ERR",
        };
        let is_active = *pane == active;
        let text = if is_active {
            format!("[{label}]")
        } else {
            format!(" {label} ")
        };
        // The error tab keeps its red whether or not it is the active tab: an
        // undismissed failure has to read as a failure from the tab bar alone.
        let style = match (pane, is_active) {
            (PreviewPane::Error, selected) => THEME.merge_failed_style(selected),
            (_, true) => THEME.selection_style(),
            (_, false) => THEME.muted_style(),
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}
