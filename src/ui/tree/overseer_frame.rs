use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::model::{OverseerCategory, Selection};

use super::{IndicatorState, indicator, label, select};
use crate::ui::{App, theme::DEFAULT as THEME};

/// The separator between a label and the value that describes it — the header's
/// status glyph and a category's summary both sit one of these off their label.
const GAP: &str = "  ";

/// Indent of an expanded category's detail rows. It matches the column
/// `category_line` puts the category label at, so detail text reads as sitting
/// under its label instead of being outdented past it. On a 24-column sidebar
/// every column spent here is a column of content lost, so the frame nests in
/// one step and stops.
const DETAIL_INDENT: &str = "    ";

pub(in crate::ui) struct FrameContent {
    pub(in crate::ui) lines: Vec<Line<'static>>,
    pub(in crate::ui) selected_row: u16,
}

impl FrameContent {
    pub(in crate::ui) fn scroll_offset(&self, inner_height: u16) -> u16 {
        self.selected_row
            .saturating_add(1)
            .saturating_sub(inner_height)
    }
}

pub(in crate::ui) fn content_lines(app: &App) -> FrameContent {
    build_content(app, None)
}

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content_area = Rect {
        width: area.width.saturating_sub(1),
        height: area.height.saturating_sub(1),
        ..area
    };
    let content = build_content(app, Some(content_area.width));
    let offset = content.scroll_offset(content_area.height);

    frame.render_widget(
        Paragraph::new(content.lines).scroll((offset, 0)),
        content_area,
    );
}

fn build_content(app: &App, width: Option<u16>) -> FrameContent {
    let health_warnings = crate::ui::overseer::health_warnings(app);
    build_content_with_warnings(app, width, &health_warnings)
}

fn build_content_with_warnings(
    app: &App,
    width: Option<u16>,
    health_warnings: &[&'static str],
) -> FrameContent {
    let selected = app.selected_item();
    let summaries =
        OverseerCategory::ALL.map(|category| crate::ui::overseer::category_summary(app, category));
    // The header is a plain label, never a selection target, so the selected
    // row only ever tracks a category row below it.
    let mut lines = vec![header_line(app, health_warnings.len(), width)];
    lines.extend(warning_lines(health_warnings));
    let mut selected_row = 0;

    for category in OverseerCategory::ALL {
        let category_selected = selected == Some(Selection::OverseerCategory(category));
        if category_selected {
            selected_row = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        }
        let (summary, warn) = &summaries[category.index()];
        lines.push(category_line(
            app,
            category,
            category_selected,
            summary,
            *warn,
        ));
        if app.overseer_category_expanded(category) {
            lines.extend(
                crate::ui::overseer::category_detail(app, category)
                    .into_iter()
                    .map(indent_detail),
            );
        }
    }

    FrameContent {
        lines,
        selected_row,
    }
}

/// Plain section header, styled like the `PROJECTS` label in the tree frame:
/// no arrow and no selection marker, because it is not a focus target.
fn header_line(app: &App, warning_count: usize, width: Option<u16>) -> Line<'static> {
    let style = THEME.accent_bold_style();
    let status = select(IndicatorState::with_status(Some(
        app.overseer_snapshot.status(),
    )));
    let indicator = indicator::primary_span(status, false, app.started.elapsed(), 1);

    // The glyph follows the label after the same two-column gap `category_line`
    // puts between a label and its summary. Padding it out to the frame width
    // instead would strand it ~37 columns away on a wide sidebar, where it no
    // longer reads as the status of the name beside it.
    let mut spans = vec![Span::styled("OVERSEER", style)];
    if let Some(width) = width {
        // Reserve the gap and the glyph up front so a narrow frame trims the
        // label rather than the status it describes.
        label::trim_spans_to_width(&mut spans, usize::from(width).saturating_sub(GAP.len() + 1));
    }
    spans.push(Span::styled(GAP, style));
    spans.push(indicator);
    // Warnings trail the indicator so the glyph stays adjacent to the label
    // whatever the warning count is, and so they are what a narrow frame drops.
    if warning_count > 0 {
        spans.push(Span::styled(
            format!("{GAP}⚠×{warning_count}"),
            warning_style(false),
        ));
    }
    if let Some(width) = width {
        label::trim_spans_to_width(&mut spans, usize::from(width));
    }
    Line::from(spans)
}

fn warning_lines<'a>(warnings: &'a [&'static str]) -> impl Iterator<Item = Line<'static>> + 'a {
    warnings
        .iter()
        .map(|warning| Line::styled(format!("⚠ {warning}"), warning_style(false)))
}

fn category_line(
    app: &App,
    category: OverseerCategory,
    selected: bool,
    summary: &str,
    warn: bool,
) -> Line<'static> {
    let arrow = if app.overseer_category_expanded(category) {
        "▾"
    } else {
        "▸"
    };
    Line::from(vec![
        Span::styled(
            format!("{} {arrow} {}{GAP}", marker(selected), category.label()),
            row_style(selected),
        ),
        Span::styled(
            summary.to_string(),
            if warn {
                warning_style(selected)
            } else if selected {
                THEME.selection_style()
            } else {
                THEME.muted_style()
            },
        ),
    ])
}

/// Nests a category's detail rows directly under its label, which
/// [`category_line`] puts at column 4. The detail rows carry no indent of their
/// own, so this is the frame's single indent origin for them.
fn indent_detail(line: Line<'static>) -> Line<'static> {
    let mut spans = Vec::with_capacity(line.spans.len() + 1);
    spans.push(Span::styled(DETAIL_INDENT, THEME.muted_style()));
    spans.extend(line.spans);
    Line::from(spans)
}

fn marker(selected: bool) -> &'static str {
    if selected { ">" } else { " " }
}

fn row_style(selected: bool) -> Style {
    if selected {
        THEME.selection_style()
    } else {
        THEME.accent_style()
    }
}

fn warning_style(selected: bool) -> Style {
    let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    if selected {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    }
}

#[cfg(test)]
mod tests;
