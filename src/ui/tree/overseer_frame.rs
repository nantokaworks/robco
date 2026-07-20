use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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
    let content = build_content(app, Some(area.width.saturating_sub(2)));
    let offset = content.scroll_offset(area.height.saturating_sub(2));
    let block = Block::default()
        .title("OVERSEER")
        .borders(Borders::ALL)
        .border_style(THEME.accent_style())
        .title_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(
        Paragraph::new(content.lines)
            .block(block)
            .scroll((offset, 0)),
        area,
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
    let root_selected = selected == Some(Selection::Overseer);
    let summaries =
        OverseerCategory::ALL.map(|category| crate::ui::overseer::category_summary(app, category));
    let mut lines = vec![root_line(app, root_selected, health_warnings.len(), width)];
    lines.extend(warning_lines(health_warnings));
    let mut selected_row = 0;

    if !app.overseer_collapsed {
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
    }

    FrameContent {
        lines,
        selected_row,
    }
}

fn root_line(app: &App, selected: bool, warning_count: usize, width: Option<u16>) -> Line<'static> {
    let style = row_style(selected);
    let arrow = if app.overseer_collapsed { "▸" } else { "▾" };
    let mut spans = vec![Span::styled(
        format!("{} {arrow} OVERSEER", marker(selected)),
        style,
    )];
    if warning_count > 0 {
        spans.push(Span::styled(
            format!("  ⚠×{warning_count}"),
            warning_style(selected),
        ));
    }

    let status = select(IndicatorState::with_status(Some(
        app.overseer_snapshot.status(),
    )));
    let indicator = indicator::primary_span(status, selected, app.started.elapsed(), 1);
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
            format!("  {} {arrow} {}  ", marker(selected), category.label()),
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
    fn active_health_warnings_have_dedicated_narrow_rows_when_collapsed_or_expanded() {
        let (warnings, mut app) = warning_state();
        assert_eq!(
            warnings,
            ["STALE/OFFLINE", "circuit OPEN", "dispatch/no daemon"]
        );

        for collapsed in [true, false] {
            app.overseer_collapsed = collapsed;
            for tree_width in [24, 48] {
                let content = build_content_with_warnings(&app, Some(tree_width - 2), &warnings);
                for warning in &warnings {
                    let expected = format!("⚠ {warning}");
                    let rows = content
                        .lines
                        .iter()
                        .filter(|line| line.to_string() == expected)
                        .collect::<Vec<_>>();
                    assert_eq!(rows.len(), 1);
                    assert!(rows[0].width() <= 22);
                }
            }
        }
    }

    #[test]
    fn warning_rows_are_included_in_selected_category_scroll_position() {
        let (warnings, mut app) = warning_state();
        app.overseer_visible = true;
        app.overseer_collapsed = false;
        app.selected = OverseerCategory::Decisions.index() + 1;

        let content = build_content_with_warnings(&app, Some(22), &warnings);

        assert_eq!(content.selected_row, 7);
        assert_eq!(content.scroll_offset(6), 2);
        assert!(content.selected_row - content.scroll_offset(6) < 6);
    }
}
