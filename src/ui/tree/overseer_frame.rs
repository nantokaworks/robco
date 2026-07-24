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
    let mut spans = vec![Span::styled("OVERSEER", style)];
    if warning_count > 0 {
        spans.push(Span::styled(
            format!("  ⚠×{warning_count}"),
            warning_style(false),
        ));
    }

    let status = select(IndicatorState::with_status(Some(
        app.overseer_snapshot.status(),
    )));
    let indicator = indicator::primary_span(status, false, app.started.elapsed(), 1);
    if let Some(width) = width {
        label::trim_spans_to_width(&mut spans, usize::from(width.saturating_sub(1)));
        let used = Line::from(spans.clone()).width().saturating_add(1);
        let padding = usize::from(width).saturating_sub(used);
        spans.push(Span::styled(" ".repeat(padding), style));
    } else {
        spans.push(Span::styled("  ", style));
    }
    spans.push(indicator);
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
            format!("{}   {arrow} {}  ", marker(selected), category.label()),
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

fn indent_detail(line: Line<'static>) -> Line<'static> {
    let mut spans = Vec::with_capacity(line.spans.len() + 1);
    spans.push(Span::styled("      ", THEME.muted_style()));
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
mod tests {
    use super::*;
    use crate::{
        config::Config,
        overseer::{config::OverseerConfig, ledger::Ledger},
        registry::Registry,
    };

    fn warning_state() -> (Vec<&'static str>, App) {
        let config = OverseerConfig {
            dispatch_enabled: true,
            failure_circuit_threshold: 2,
            ..OverseerConfig::default()
        };
        let mut ledger = Ledger::default();
        ledger.counters.consecutive_failures = 2;
        let warnings = crate::ui::overseer::health_warnings_from(&config, &ledger, false);
        let temp = tempfile::tempdir().unwrap();
        let app = App::new(Registry::default(), Config::default(), temp.path().into());
        (warnings, app)
    }

    #[test]
    fn active_health_warnings_have_dedicated_narrow_rows() {
        let (warnings, app) = warning_state();
        assert_eq!(
            warnings,
            ["STALE/OFFLINE", "circuit OPEN", "dispatch/no daemon"]
        );

        for tree_width in [24, 48] {
            let content = build_content_with_warnings(&app, Some(tree_width - 1), &warnings);
            for warning in &warnings {
                let expected = format!("⚠ {warning}");
                let rows = content
                    .lines
                    .iter()
                    .filter(|line| line.to_string() == expected)
                    .collect::<Vec<_>>();
                assert_eq!(rows.len(), 1);
                assert!(rows[0].width() <= 23);
            }
        }
    }

    #[test]
    fn the_header_is_a_plain_label_with_no_arrow_or_marker() {
        let (warnings, app) = warning_state();
        let content = build_content_with_warnings(&app, Some(23), &warnings);
        let header = content.lines[0].to_string();

        assert!(header.starts_with("OVERSEER"), "header row: {header:?}");
        assert!(!header.contains('▾') && !header.contains('▸'));
        assert!(header.contains("⚠×3"));
    }

    #[test]
    fn warning_rows_are_included_in_selected_category_scroll_position() {
        let (warnings, mut app) = warning_state();
        app.overseer_visible = true;
        app.selected = OverseerCategory::Decisions.index();

        let content = build_content_with_warnings(&app, Some(23), &warnings);

        assert_eq!(content.selected_row, 7);
        assert_eq!(content.scroll_offset(6), 2);
        assert!(content.selected_row - content.scroll_offset(6) < 6);
    }
}
